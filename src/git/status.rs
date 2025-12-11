use std::path::Path;

use git2::{BranchType, Repository};

#[derive(Debug, Clone, Default)]
pub struct RepoStatus {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: Option<usize>,
    pub behind: Option<usize>,
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
