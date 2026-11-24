use std::path::Path;

use git2::{BranchType, ErrorCode};
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
    with_cached_repo(path, |repo| {
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
    })
}
