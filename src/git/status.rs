use std::path::Path;

use git2::{BranchType, Repository, Status, StatusOptions};

#[derive(Debug, Clone, Default)]
pub struct RepoStatus {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: Option<usize>,
    pub behind: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkingTreeStatus {
    pub staged: Vec<String>,
    pub unstaged: Vec<String>,
    pub untracked: Vec<String>,
    pub conflicted: Vec<String>,
}

pub fn read_repo_status<P: AsRef<Path>>(path: P) -> Result<RepoStatus, git2::Error> {
    let repo = Repository::open(path)?;
    let head = repo.head()?;

    if !head.is_branch() {
        return Ok(RepoStatus {
            branch: None,
            upstream: None,
            ahead: None,
            behind: None,
        });
    }

    let branch_name = head.shorthand().map(|name| name.to_string());
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

pub fn read_working_tree_status<P: AsRef<Path>>(
    path: P,
) -> Result<WorkingTreeStatus, git2::Error> {
    let repo = Repository::open(path)?;
    let mut options = StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(false)
        .include_unmodified(false)
        .renames_head_to_index(true)
        .renames_index_to_workdir(true);

    let statuses = repo.statuses(Some(&mut options))?;
    let mut staged = std::collections::BTreeSet::new();
    let mut unstaged = std::collections::BTreeSet::new();
    let mut untracked = std::collections::BTreeSet::new();
    let mut conflicted = std::collections::BTreeSet::new();

    for entry in statuses.iter() {
        let status = entry.status();
        let Some(path) = entry.path() else {
            continue;
        };

        if status.is_conflicted() {
            conflicted.insert(path.to_string());
        }

        if status.intersects(
            Status::INDEX_NEW
                | Status::INDEX_MODIFIED
                | Status::INDEX_DELETED
                | Status::INDEX_RENAMED
                | Status::INDEX_TYPECHANGE,
        ) {
            staged.insert(path.to_string());
        }

        if status.intersects(
            Status::WT_MODIFIED
                | Status::WT_DELETED
                | Status::WT_TYPECHANGE
                | Status::WT_RENAMED,
        ) {
            unstaged.insert(path.to_string());
        }

        if status.contains(Status::WT_NEW) {
            untracked.insert(path.to_string());
        }
    }

    Ok(WorkingTreeStatus {
        staged: staged.into_iter().collect(),
        unstaged: unstaged.into_iter().collect(),
        untracked: untracked.into_iter().collect(),
        conflicted: conflicted.into_iter().collect(),
    })
}
