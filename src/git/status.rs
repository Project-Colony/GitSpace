use std::path::{Component, Path};

use git2::build::CheckoutBuilder;
use git2::{
    BranchType, Delta, Diff, DiffFindOptions, DiffOptions, ErrorCode, Patch, Repository, Status,
    StatusOptions,
};

use crate::git::diff::head_tree;
// Utilise le cache thread-local pour éviter de réouvrir le dépôt à chaque appel
use crate::git::repo_cache::with_cached_repo;

#[derive(Debug, Clone, Default)]
pub struct RepoStatus {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: Option<usize>,
    pub behind: Option<usize>,
}

pub fn read_repo_status<P: AsRef<Path>>(path: P) -> Result<RepoStatus, git2::Error> {
    // Lecture seule du statut — utilise le cache thread-local
    with_cached_repo(path, repo_status)
}

fn repo_status(repo: &Repository) -> Result<RepoStatus, git2::Error> {
    let head = match repo.head() {
        Ok(head) => head,
        // A fresh `git init`: HEAD names a branch with no commit yet.
        Err(err) if err.code() == ErrorCode::UnbornBranch => {
            let head = repo.find_reference("HEAD")?;
            let branch = head
                .symbolic_target()?
                .map(|target| target.strip_prefix("refs/heads/").unwrap_or(target));
            return Ok(RepoStatus {
                branch: branch.map(str::to_string),
                ..RepoStatus::default()
            });
        }
        Err(err) => return Err(err),
    };

    if !head.is_branch() {
        return Ok(RepoStatus {
            branch: None,
            upstream: None,
            ahead: None,
            behind: None,
        });
    }

    let branch_name = head.shorthand().ok().map(|name| name.to_string());
    let mut status = RepoStatus {
        branch: branch_name.clone(),
        upstream: None,
        ahead: None,
        behind: None,
    };

    if let Some(name) = branch_name {
        let branch = repo.find_branch(&name, BranchType::Local)?;

        if let Ok(upstream) = branch.upstream() {
            status.upstream = upstream.name()?.map(|name| name.to_string());

            if let (Some(local_oid), Some(upstream_oid)) =
                (branch.get().target(), upstream.get().target())
            {
                if let Ok((ahead, behind)) = repo.graph_ahead_behind(local_oid, upstream_oid) {
                    status.ahead = Some(ahead);
                    status.behind = Some(behind);
                }
            }
        }
    }

    Ok(status)
}

/// The cheap status shown next to a repository in the sidebar.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QuickStatus {
    pub branch: Option<String>,
    pub ahead: usize,
    pub behind: usize,
    /// Changed and untracked entries; an untracked folder counts once.
    pub changed: usize,
    /// The folder is gone or no longer opens as a repository.
    pub missing: bool,
}

/// Branch, divergence from upstream and changed-entry count, without building patches.
/// Never fails: a path that does not open as a repository is reported as `missing`.
pub fn quick_status(path: &str) -> QuickStatus {
    // Opened fresh, not cached: a cached handle would outlive a deleted folder, and a scan
    // of many repositories would only churn the small cache.
    let Ok(repo) = Repository::open(path) else {
        return QuickStatus {
            missing: true,
            ..QuickStatus::default()
        };
    };
    let status = repo_status(&repo).unwrap_or_default();
    let mut options = StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(false)
        .include_ignored(false);
    let changed = repo
        .statuses(Some(&mut options))
        .map(|statuses| statuses.len())
        .unwrap_or(0);
    QuickStatus {
        branch: status.branch,
        ahead: status.ahead.unwrap_or(0),
        behind: status.behind.unwrap_or(0),
        changed,
        missing: false,
    }
}

/// Untracked files above this size are listed without counting their lines.
const MAX_UNTRACKED_COUNT_SIZE: u64 = 1024 * 1024;

#[cfg_attr(not(test), expect(dead_code, reason = "the Changes tab will call it"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Untracked,
    Conflicted,
    TypeChange,
}

#[cfg_attr(not(test), expect(dead_code, reason = "the Changes tab will call it"))]
#[derive(Debug, Clone)]
pub struct ChangedFile {
    pub path: String,
    /// The previous path of a renamed file.
    pub old_path: Option<String>,
    pub kind: ChangeKind,
    pub additions: usize,
    pub deletions: usize,
    pub binary: bool,
}

#[cfg_attr(not(test), expect(dead_code, reason = "the Changes tab will call it"))]
#[derive(Debug, Clone, Default)]
pub struct Changes {
    pub staged: Vec<ChangedFile>,
    pub unstaged: Vec<ChangedFile>,
}

/// Staged (HEAD to index) and unstaged (index to work tree, with untracked files) changes,
/// with line counts per file. No patch text is kept.
#[cfg_attr(not(test), expect(dead_code, reason = "the Changes tab will call it"))]
pub fn read_changes(repo_path: &str) -> Result<Changes, git2::Error> {
    with_cached_repo(repo_path, |repo| {
        let mut index = repo.index()?;
        index.read(true)?;

        let base_tree = head_tree(repo)?;
        let mut staged = repo.diff_tree_to_index(base_tree.as_ref(), Some(&index), None)?;
        staged.find_similar(Some(DiffFindOptions::new().renames(true)))?;

        let mut options = DiffOptions::new();
        options
            .include_untracked(true)
            .recurse_untracked_dirs(true)
            .show_untracked_content(true);
        let unstaged = repo.diff_index_to_workdir(Some(&index), Some(&mut options))?;
        let workdir = repo.workdir();

        Ok(Changes {
            staged: changed_files(&staged, None)?,
            unstaged: changed_files(&unstaged, workdir)?,
        })
    })
}

/// One entry per delta. `workdir` is set for the work tree diff, to size untracked files.
fn changed_files(diff: &Diff<'_>, workdir: Option<&Path>) -> Result<Vec<ChangedFile>, git2::Error> {
    let mut files = Vec::with_capacity(diff.deltas().len());
    for (index, delta) in diff.deltas().enumerate() {
        let kind = match delta.status() {
            Delta::Added | Delta::Copied => ChangeKind::Added,
            Delta::Modified | Delta::Unreadable => ChangeKind::Modified,
            Delta::Deleted => ChangeKind::Deleted,
            Delta::Renamed => ChangeKind::Renamed,
            Delta::Untracked => ChangeKind::Untracked,
            Delta::Conflicted => ChangeKind::Conflicted,
            Delta::Typechange => ChangeKind::TypeChange,
            Delta::Unmodified | Delta::Ignored => continue,
        };
        let path_of =
            |file: git2::DiffFile<'_>| file.path().map(|path| path.to_string_lossy().into_owned());
        let path = path_of(delta.new_file())
            .or_else(|| path_of(delta.old_file()))
            .unwrap_or_default();
        let old_path = (kind == ChangeKind::Renamed)
            .then(|| path_of(delta.old_file()))
            .flatten();

        let too_large = kind == ChangeKind::Untracked
            && workdir
                .and_then(|root| std::fs::metadata(root.join(&path)).ok())
                .is_some_and(|meta| meta.len() > MAX_UNTRACKED_COUNT_SIZE);
        // Conflicted and unreadable entries have no single content to compare.
        let counted =
            !too_large && !matches!(delta.status(), Delta::Conflicted | Delta::Unreadable);
        let patch = if counted {
            // A file that cannot be read (no permission, say) is listed without counts.
            Patch::from_diff(diff, index).ok().flatten()
        } else {
            None
        };
        let (additions, deletions) = match &patch {
            Some(patch) => {
                let (_context, additions, deletions) = patch.line_stats()?;
                (additions, deletions)
            }
            None => (0, 0),
        };
        let binary = delta.flags().is_binary()
            || patch
                .as_ref()
                .is_some_and(|patch| patch.delta().flags().is_binary());

        files.push(ChangedFile {
            path,
            old_path,
            kind,
            additions,
            deletions,
            binary,
        });
    }
    Ok(files)
}

/// Throws away the work tree changes of one file: a tracked file goes back to its index
/// version, an untracked file is removed. `path` is relative to the work tree.
#[cfg_attr(not(test), expect(dead_code, reason = "the Changes tab will call it"))]
pub fn discard_file(repo_path: &str, path: &str) -> Result<(), git2::Error> {
    let relative = Path::new(path);
    if path.is_empty()
        || !relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(git2::Error::from_str("the path is outside the repository"));
    }

    with_cached_repo(repo_path, |repo| {
        let workdir = repo
            .workdir()
            .ok_or_else(|| git2::Error::from_str("the repository has no work tree"))?;
        let mut index = repo.index()?;
        index.read(true)?;

        if index.get_path(relative, 0).is_some() {
            let mut checkout = CheckoutBuilder::new();
            checkout.force().disable_pathspec_match(true).path(path);
            return repo.checkout_index(Some(&mut index), Some(&mut checkout));
        }
        // Only a file git reports as untracked is removed, never an ignored or .git file.
        if repo.status_file(relative)?.contains(Status::WT_NEW) {
            std::fs::remove_file(workdir.join(relative))
                .map_err(|err| git2::Error::from_str(&err.to_string()))
        } else {
            Err(git2::Error::from_str("the file has no changes to discard"))
        }
    })
}
