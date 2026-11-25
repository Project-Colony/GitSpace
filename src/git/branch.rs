use std::path::Path;
use std::process::Command;

use chrono::Utc;
use git2::{build::CheckoutBuilder, BranchType, Error, ErrorCode, Repository};

use crate::git::repo_cache::with_cached_repo;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchKind {
    Local,
    Remote,
}

#[derive(Debug, Clone)]
pub struct BranchEntry {
    pub name: String,
    pub kind: BranchKind,
    pub is_head: bool,
}

pub fn list_branches<P: AsRef<Path>>(repo_path: P) -> Result<Vec<BranchEntry>, Error> {
    // Utilise le cache thread-local au lieu de Repository::open()
    with_cached_repo(repo_path, |repo| {
        let mut entries = Vec::new();
        for branch_result in repo.branches(None)? {
            let (branch, branch_type) = branch_result?;
            if let Some(name) = branch.name()? {
                let kind = match branch_type {
                    BranchType::Local => BranchKind::Local,
                    BranchType::Remote => BranchKind::Remote,
                };

                entries.push(BranchEntry {
                    name: name.to_string(),
                    kind,
                    is_head: branch.is_head(),
                });
            }
        }

        entries.sort_by(|a, b| (branch_sort_key(a), &a.name).cmp(&(branch_sort_key(b), &b.name)));
        Ok(entries)
    })
}

/// A branch with what the Branches tab shows next to it.
#[derive(Debug, Clone)]
#[cfg_attr(not(test), expect(dead_code, reason = "the Branches tab will call it"))]
pub struct BranchOverview {
    pub name: String,
    pub kind: BranchKind,
    pub is_head: bool,
    pub upstream: Option<String>,
    pub upstream_ahead: usize,
    pub upstream_behind: usize,
    pub ahead_of_head: usize,
    pub behind_head: usize,
    pub merged_into_head: bool,
    pub last_commit_time: Option<i64>,
}

/// Lists branches with their divergence from HEAD and from their upstream.
///
/// Runs one `graph_ahead_behind` against HEAD and one `merge_base` per branch, so it belongs
/// in a background task. With an unborn HEAD the counts against HEAD stay 0 and no branch is
/// marked merged.
#[cfg_attr(not(test), expect(dead_code, reason = "the Branches tab will call it"))]
pub fn list_branch_overview(repo_path: &str) -> Result<Vec<BranchOverview>, Error> {
    with_cached_repo(repo_path, |repo| {
        let head = repo.head().ok().and_then(|head| head.target());
        let mut entries = Vec::new();
        for branch_result in repo.branches(None)? {
            let (branch, branch_type) = branch_result?;
            let Some(name) = branch.name()? else {
                continue;
            };
            let kind = match branch_type {
                BranchType::Local => BranchKind::Local,
                BranchType::Remote => BranchKind::Remote,
            };
            let is_head = branch.is_head();
            // Symbolic refs such as origin/HEAD resolve to their target.
            let tip = branch.get().peel_to_commit().ok();
            let upstream = branch.upstream().ok();
            let upstream_name = match &upstream {
                Some(upstream) => upstream.name()?.map(str::to_string),
                None => None,
            };

            let mut entry = BranchOverview {
                name: name.to_string(),
                kind,
                is_head,
                upstream: upstream_name,
                upstream_ahead: 0,
                upstream_behind: 0,
                ahead_of_head: 0,
                behind_head: 0,
                merged_into_head: false,
                last_commit_time: tip.as_ref().map(|commit| commit.time().seconds()),
            };
            if let Some(tip) = tip.map(|commit| commit.id()) {
                if let Some(upstream_tip) = upstream.and_then(|upstream| upstream.get().target()) {
                    (entry.upstream_ahead, entry.upstream_behind) =
                        repo.graph_ahead_behind(tip, upstream_tip)?;
                }
                if let Some(head) = head {
                    (entry.ahead_of_head, entry.behind_head) =
                        repo.graph_ahead_behind(tip, head)?;
                    // A branch with no history in common with HEAD (gh-pages) has no merge base.
                    entry.merged_into_head = !is_head
                        && match repo.merge_base(tip, head) {
                            Ok(base) => base == tip,
                            Err(error) if error.code() == ErrorCode::NotFound => false,
                            Err(error) => return Err(error),
                        };
                }
            }
            entries.push(entry);
        }

        entries.sort_by(|a, b| (kind_order(&a.kind), &a.name).cmp(&(kind_order(&b.kind), &b.name)));
        Ok(entries)
    })
}

/// Sets the upstream of a local branch (`origin/main`), or removes it with `None`.
#[cfg_attr(not(test), expect(dead_code, reason = "the Branches tab will call it"))]
pub fn set_upstream(repo_path: &str, branch: &str, upstream: Option<&str>) -> Result<(), Error> {
    with_cached_repo(repo_path, |repo| {
        repo.find_branch(branch, BranchType::Local)?
            .set_upstream(upstream)
    })
}

pub fn create_branch<P: AsRef<Path>>(
    repo_path: P,
    name: &str,
    start_point: Option<&str>,
) -> Result<(), Error> {
    // Crée une branche via le dépôt mis en cache
    with_cached_repo(repo_path, |repo| {
        let commit = match start_point {
            Some(start_point) => repo.revparse_single(start_point)?.peel_to_commit()?,
            None => repo.head()?.peel_to_commit()?,
        };
        repo.branch(name, &commit, false)?;
        Ok(())
    })
}

pub fn create_tracking_branch<P: AsRef<Path>>(
    repo_path: P,
    remote_branch: &str,
) -> Result<String, Error> {
    // Crée une branche de suivi via le dépôt mis en cache
    with_cached_repo(repo_path, |repo| {
        let remote = repo.find_branch(remote_branch, BranchType::Remote)?;
        let commit = remote.into_reference().peel_to_commit()?;
        let local_name = local_branch_name(remote_branch);
        repo.branch(&local_name, &commit, false)?;
        let mut local_branch = repo.find_branch(&local_name, BranchType::Local)?;
        local_branch.set_upstream(Some(remote_branch))?;
        Ok(local_name)
    })
}

pub fn delete_branch<P: AsRef<Path>>(repo_path: P, name: &str) -> Result<(), Error> {
    // Supprime la branche via le dépôt mis en cache
    with_cached_repo(repo_path, |repo| {
        let mut branch = repo.find_branch(name, BranchType::Local)?;
        branch.delete()?;
        Ok(())
    })
}

pub fn rename_branch<P: AsRef<Path>>(repo_path: P, old: &str, new: &str) -> Result<(), Error> {
    // Renomme la branche via le dépôt mis en cache
    with_cached_repo(repo_path, |repo| {
        let mut branch = repo.find_branch(old, BranchType::Local)?;
        branch.rename(new, false)?;
        Ok(())
    })
}

pub fn checkout_branch<P: AsRef<Path>>(repo_path: P, name: &str) -> Result<(), Error> {
    // Effectue le checkout via le dépôt mis en cache
    with_cached_repo(repo_path, |repo| {
        let reference_name = format!("refs/heads/{name}");
        let (object, reference) = match repo.revparse_ext(&reference_name) {
            Ok(result) => result,
            Err(_) => repo.revparse_ext(name)?,
        };

        // The cached handle keeps its index: reload it, or libgit2 writes the old one back.
        repo.index()?.read(false)?;
        // Safe checkout: libgit2 fails instead of overwriting a modified file.
        repo.checkout_tree(&object, Some(&mut CheckoutBuilder::new()))?;
        if let Some(reference) = reference {
            if let Ok(name) = reference.name() {
                repo.set_head(name)?;
            } else {
                repo.set_head_detached(object.id())?;
            }
        } else {
            repo.set_head_detached(object.id())?;
        }

        Ok(())
    })
}

pub fn archive_branch<P: AsRef<Path>>(repo_path: P, name: &str) -> Result<String, Error> {
    // Archive la branche en créant un tag léger via le dépôt mis en cache
    with_cached_repo(repo_path, |repo| {
        let reference_name = format!("refs/heads/{name}");
        let reference = repo.find_reference(&reference_name)?;
        let commit = reference.peel_to_commit()?;
        let date_suffix = Utc::now().format("%Y%m%d").to_string();
        let base_tag = format!("archive/{name}-{date_suffix}");
        let tag_name = next_available_tag_name(repo, &base_tag)?;
        repo.tag_lightweight(&tag_name, commit.as_object(), false)?;
        Ok(tag_name)
    })
}

pub fn restore_file_from_branch<P: AsRef<Path>>(
    repo_path: P,
    branch: &str,
    path: &str,
) -> Result<(), String> {
    let output = Command::new("git")
        // Literal pathspecs: a file named `*.md` must not also restore README.md.
        .args(["--literal-pathspecs", "checkout", branch, "--", path])
        .current_dir(repo_path.as_ref())
        .output()
        .map_err(|err| err.to_string())?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Err(if !stderr.is_empty() { stderr } else { stdout })
    }
}

fn branch_sort_key(entry: &BranchEntry) -> usize {
    kind_order(&entry.kind)
}

fn kind_order(kind: &BranchKind) -> usize {
    match kind {
        BranchKind::Local => 0,
        BranchKind::Remote => 1,
    }
}

fn local_branch_name(remote_branch: &str) -> String {
    remote_branch
        .split_once('/')
        .map(|(_, name)| name.to_string())
        .unwrap_or_else(|| remote_branch.to_string())
}

fn next_available_tag_name(repo: &Repository, base: &str) -> Result<String, Error> {
    if repo.find_reference(&format!("refs/tags/{base}")).is_err() {
        return Ok(base.to_string());
    }

    let mut index = 2;
    loop {
        let candidate = format!("{base}-{index}");
        if repo
            .find_reference(&format!("refs/tags/{candidate}"))
            .is_err()
        {
            return Ok(candidate);
        }
        index += 1;
    }
}
