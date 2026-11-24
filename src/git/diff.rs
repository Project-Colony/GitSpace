use std::collections::HashMap;

use git2::{Diff, DiffDelta, DiffFormat, DiffOptions, Oid, Patch, Repository, Tree};

use crate::git::repo_cache::with_cached_repo;

/// Maximum patch size in bytes before truncation (64 KB)
const MAX_PATCH_SIZE: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct FileDiff {
    pub path: String,
    pub additions: usize,
    pub deletions: usize,
    pub patch: String,
    /// True if patch was truncated due to size
    pub truncated: bool,
}

/// Lightweight summary without patch content - for lazy loading
#[derive(Debug, Clone)]
pub struct FileDiffSummary {
    pub path: String,
    pub additions: usize,
    pub deletions: usize,
    pub is_binary: bool,
}

/// The path shown for a delta: the new path, or the old one for a deletion.
fn delta_path(delta: &DiffDelta<'_>) -> String {
    delta
        .new_file()
        .path()
        .or_else(|| delta.old_file().path())
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| "(unknown)".to_string())
}

fn collect_diff_files(diff: Diff) -> Result<Vec<FileDiff>, git2::Error> {
    let mut files: HashMap<String, FileDiff> = HashMap::new();

    diff.print(DiffFormat::Patch, |delta, _hunk, line| {
        let path = delta_path(&delta);
        let entry = files.entry(path.clone()).or_insert_with(|| FileDiff {
            path,
            additions: 0,
            deletions: 0,
            patch: String::with_capacity(1024),
            truncated: false,
        });

        match line.origin() {
            '+' => entry.additions += 1,
            '-' => entry.deletions += 1,
            _ => {}
        }

        // Skip appending if already truncated
        if entry.truncated {
            return true;
        }

        let content = std::str::from_utf8(line.content()).unwrap_or("");

        // Check if adding this line would exceed the limit
        if entry.patch.len() + content.len() + 1 > MAX_PATCH_SIZE {
            entry
                .patch
                .push_str("\n... [truncated - file too large] ...\n");
            entry.truncated = true;
            return true;
        }

        // Only content lines carry an origin prefix; headers and the "no newline" marker
        // (origins F, H, B, =, >, <) already hold their full text.
        let origin = line.origin();
        if matches!(origin, '+' | '-' | ' ') {
            entry.patch.push(origin);
        }
        entry.patch.push_str(content);

        true
    })?;

    // Files without textual output (e.g., binary) get a placeholder.
    for delta in diff.deltas() {
        let path = delta_path(&delta);
        files.entry(path.clone()).or_insert_with(|| FileDiff {
            path,
            additions: 0,
            deletions: 0,
            patch: String::from("Binary file change\n"),
            truncated: false,
        });
    }

    Ok(files.into_values().collect())
}

/// Collect only diff summaries without patch content (for lazy loading)
fn collect_diff_summaries(diff: &Diff) -> Result<Vec<FileDiffSummary>, git2::Error> {
    let mut summaries = Vec::with_capacity(diff.deltas().len());
    for (index, delta) in diff.deltas().enumerate() {
        // `None` for a binary file, which has no lines to count.
        let patch = Patch::from_diff(diff, index)?;
        let (additions, deletions) = match &patch {
            Some(patch) => {
                let (_context, additions, deletions) = patch.line_stats()?;
                (additions, deletions)
            }
            None => (0, 0),
        };
        let is_binary = patch
            .as_ref()
            .is_none_or(|patch| patch.delta().flags().is_binary());
        summaries.push(FileDiffSummary {
            path: delta_path(&delta),
            additions,
            deletions,
            is_binary,
        });
    }
    Ok(summaries)
}

fn head_tree(repo: &Repository) -> Result<Option<Tree<'_>>, git2::Error> {
    let head = match repo.head() {
        Ok(head) => head,
        Err(_) => return Ok(None),
    };

    let oid = match head.target() {
        Some(oid) => oid,
        None => return Ok(None),
    };

    let commit = repo.find_commit(oid)?;
    Ok(Some(commit.tree()?))
}

/// Diff of commit `oid` against its first parent, or against nothing for a root commit.
pub(crate) fn commit_against_parent<'r>(
    repo: &'r Repository,
    oid: Oid,
    options: Option<&mut DiffOptions>,
) -> Result<Diff<'r>, git2::Error> {
    let commit = repo.find_commit(oid)?;
    let parent_tree = commit
        .parent(0)
        .ok()
        .map(|parent| parent.tree())
        .transpose()?;
    repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&commit.tree()?), options)
}

/// Options that limit a diff to exactly `path`, without glob matching, so names such as
/// `[draft].md` work.
fn single_path_options(path: &str) -> DiffOptions {
    let mut options = DiffOptions::new();
    options.pathspec(path).disable_pathspec_match(true);
    options
}

/// Get full diff for all files in a commit (prefer commit_diff_summaries + commit_diff_file for lazy loading)
#[allow(dead_code)] // tested, not used by the iced UI yet
pub fn commit_diff(repo_path: &str, oid: &str) -> Result<Vec<FileDiff>, git2::Error> {
    let oid = Oid::from_str(oid)?;
    with_cached_repo(repo_path, |repo| {
        collect_diff_files(commit_against_parent(repo, oid, None)?)
    })
}

/// Get only summaries for a commit (for lazy loading - no patch content)
pub fn commit_diff_summaries(
    repo_path: &str,
    oid: &str,
) -> Result<Vec<FileDiffSummary>, git2::Error> {
    let oid = Oid::from_str(oid)?;
    with_cached_repo(repo_path, |repo| {
        collect_diff_summaries(&commit_against_parent(repo, oid, None)?)
    })
}

/// Get the diff for a single file in a commit (for lazy loading)
pub fn commit_diff_file(
    repo_path: &str,
    oid: &str,
    file_path: &str,
) -> Result<Option<FileDiff>, git2::Error> {
    let oid = Oid::from_str(oid)?;
    with_cached_repo(repo_path, |repo| {
        let mut options = single_path_options(file_path);
        let diff = commit_against_parent(repo, oid, Some(&mut options))?;
        Ok(collect_diff_files(diff)?.into_iter().next())
    })
}

// Diff de l'arbre de travail — comparaison index/répertoire de travail
pub fn working_tree_diff(repo_path: &str) -> Result<Vec<FileDiff>, git2::Error> {
    with_cached_repo(repo_path, |repo| {
        let mut index = repo.index()?;
        index.read(true)?;
        let mut options = DiffOptions::new();
        options.include_untracked(true);
        let diff = repo.diff_index_to_workdir(Some(&index), Some(&mut options))?;

        collect_diff_files(diff)
    })
}

// Diff des fichiers indexés (staged) — comparaison HEAD/index
pub fn staged_diff(repo_path: &str) -> Result<Vec<FileDiff>, git2::Error> {
    with_cached_repo(repo_path, |repo| {
        let mut index = repo.index()?;
        index.read(true)?;
        let base_tree = head_tree(repo)?;
        let diff = repo.diff_tree_to_index(base_tree.as_ref(), Some(&index), None)?;

        collect_diff_files(diff)
    })
}

// Diff d'un fichier unique, indexé ou non — utilise le cache
pub fn diff_file(
    repo_path: &str,
    path: &str,
    staged: bool,
) -> Result<Option<FileDiff>, git2::Error> {
    with_cached_repo(repo_path, |repo| {
        let mut options = single_path_options(path);
        options.context_lines(3);
        let mut index = repo.index()?;
        index.read(true)?;

        let diff = if staged {
            let base_tree = head_tree(repo)?;
            repo.diff_tree_to_index(base_tree.as_ref(), Some(&index), Some(&mut options))?
        } else {
            options.include_untracked(true);
            repo.diff_index_to_workdir(Some(&index), Some(&mut options))?
        };

        Ok(collect_diff_files(diff)?.into_iter().next())
    })
}
