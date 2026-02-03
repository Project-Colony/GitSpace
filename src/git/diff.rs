use std::collections::HashMap;

use git2::{Diff, DiffFormat, DiffOptions, Oid, Repository, Tree};

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

fn collect_diff_files(diff: Diff) -> Result<Vec<FileDiff>, git2::Error> {
    let mut files: HashMap<String, FileDiff> = HashMap::new();

    diff.print(DiffFormat::Patch, |delta, _hunk, line| {
        let path = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .and_then(|p| p.to_str())
            .unwrap_or("(unknown)")
            .to_string();

        let entry = files.entry(path.clone()).or_insert_with(|| FileDiff {
            path: path.clone(),
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
            entry.patch.push_str("\n... [truncated - file too large] ...\n");
            entry.truncated = true;
            return true;
        }

        match line.origin() {
            '\\' => entry.patch.push(' '),
            other => entry.patch.push(other),
        }
        entry.patch.push_str(content);

        true
    })?;

    // For files without textual output (e.g., binary), gather stats separately.
    let stats = diff.stats()?;
    if stats.files_changed() > files.len() {
        for delta in diff.deltas() {
            let path = delta
                .new_file()
                .path()
                .or_else(|| delta.old_file().path())
                .and_then(|p| p.to_str())
                .unwrap_or("(unknown)")
                .to_string();

            files.entry(path.clone()).or_insert_with(|| FileDiff {
                path: path.clone(),
                additions: 0,
                deletions: 0,
                patch: String::from("Binary file change\n"),
                truncated: false,
            });
        }
    }

    Ok(files.into_values().collect())
}

/// Collect only diff summaries without patch content (for lazy loading)
fn collect_diff_summaries(diff: &Diff) -> Result<Vec<FileDiffSummary>, git2::Error> {
    let mut summaries: HashMap<String, FileDiffSummary> = HashMap::new();

    for delta in diff.deltas() {
        let path = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .and_then(|p| p.to_str())
            .unwrap_or("(unknown)")
            .to_string();

        let is_binary = delta.flags().is_binary();

        summaries.entry(path.clone()).or_insert_with(|| FileDiffSummary {
            path,
            additions: 0,
            deletions: 0,
            is_binary,
        });
    }

    // Count additions/deletions from stats
    let stats = diff.stats()?;
    for i in 0..stats.files_changed() {
        if let Some(delta) = diff.get_delta(i) {
            let path = delta
                .new_file()
                .path()
                .or_else(|| delta.old_file().path())
                .and_then(|p| p.to_str())
                .unwrap_or("(unknown)")
                .to_string();

            if let Some(summary) = summaries.get_mut(&path) {
                // We'll get more accurate counts from the diff print if needed
                summary.additions = delta.new_file().size() as usize;
                summary.deletions = delta.old_file().size() as usize;
            }
        }
    }

    Ok(summaries.into_values().collect())
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

/// Get full diff for all files in a commit (prefer commit_diff_summaries + commit_diff_file for lazy loading)
#[allow(dead_code)]
pub fn commit_diff(repo_path: &str, oid: &str) -> Result<Vec<FileDiff>, git2::Error> {
    let repo = Repository::open(repo_path)?;
    let oid = Oid::from_str(oid)?;
    let commit = repo.find_commit(oid)?;
    let tree = commit.tree()?;

    let parent_tree = if let Ok(parent) = commit.parent(0) {
        Some(parent.tree()?)
    } else {
        None
    };

    let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)?;

    collect_diff_files(diff)
}

/// Get only summaries for a commit (for lazy loading - no patch content)
pub fn commit_diff_summaries(repo_path: &str, oid: &str) -> Result<Vec<FileDiffSummary>, git2::Error> {
    let repo = Repository::open(repo_path)?;
    let oid = Oid::from_str(oid)?;
    let commit = repo.find_commit(oid)?;
    let tree = commit.tree()?;

    let parent_tree = if let Ok(parent) = commit.parent(0) {
        Some(parent.tree()?)
    } else {
        None
    };

    let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)?;
    collect_diff_summaries(&diff)
}

/// Get the diff for a single file in a commit (for lazy loading)
pub fn commit_diff_file(repo_path: &str, oid: &str, file_path: &str) -> Result<Option<FileDiff>, git2::Error> {
    let repo = Repository::open(repo_path)?;
    let oid = Oid::from_str(oid)?;
    let commit = repo.find_commit(oid)?;
    let tree = commit.tree()?;

    let parent_tree = if let Ok(parent) = commit.parent(0) {
        Some(parent.tree()?)
    } else {
        None
    };

    let mut options = DiffOptions::new();
    options.pathspec(file_path);

    let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), Some(&mut options))?;
    Ok(collect_diff_files(diff)?.into_iter().next())
}

pub fn working_tree_diff(repo_path: &str) -> Result<Vec<FileDiff>, git2::Error> {
    let repo = Repository::open(repo_path)?;
    let mut index = repo.index()?;
    index.read(true)?;
    let mut options = DiffOptions::new();
    options.include_untracked(true);
    let diff = repo.diff_index_to_workdir(Some(&index), Some(&mut options))?;

    collect_diff_files(diff)
}

pub fn staged_diff(repo_path: &str) -> Result<Vec<FileDiff>, git2::Error> {
    let repo = Repository::open(repo_path)?;
    let mut index = repo.index()?;
    index.read(true)?;
    let base_tree = head_tree(&repo)?;
    let diff = repo.diff_tree_to_index(base_tree.as_ref(), Some(&index), None)?;

    collect_diff_files(diff)
}

pub fn diff_file(
    repo_path: &str,
    path: &str,
    staged: bool,
) -> Result<Option<FileDiff>, git2::Error> {
    let repo = Repository::open(repo_path)?;
    let mut options = DiffOptions::new();
    options.pathspec(path).context_lines(3);

    let diff = if staged {
        let mut index = repo.index()?;
        index.read(true)?;
        let base_tree = head_tree(&repo)?;
        repo.diff_tree_to_index(base_tree.as_ref(), Some(&index), Some(&mut options))?
    } else {
        let mut index = repo.index()?;
        index.read(true)?;
        options.include_untracked(true);
        repo.diff_index_to_workdir(Some(&index), Some(&mut options))?
    };

    Ok(collect_diff_files(diff)?.into_iter().next())
}
