use std::path::Path;

use git2::{BranchType, Error, Repository, build::CheckoutBuilder};

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
    pub upstream: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct TrackingBranch {
    pub local: String,
    pub upstream: String,
}

pub fn list_branches<P: AsRef<Path>>(repo_path: P) -> Result<Vec<BranchEntry>, Error> {
    let repo = Repository::open(repo_path)?;
    let head_name = repo
        .head()
        .ok()
        .and_then(|head| head.shorthand().map(|s| s.to_string()));

    let mut entries = Vec::new();
    for branch_result in repo.branches(None)? {
        let (branch, branch_type) = branch_result?;
        if let Some(name) = branch.name()? {
            let upstream = branch
                .upstream()
                .ok()
                .and_then(|upstream| upstream.name().ok().flatten().map(|s| s.to_string()));

            let kind = match branch_type {
                BranchType::Local => BranchKind::Local,
                BranchType::Remote => BranchKind::Remote,
            };

            let is_head = head_name
                .as_ref()
                .map(|current| current == name)
                .unwrap_or(false);

            entries.push(BranchEntry {
                name: name.to_string(),
                kind,
                is_head,
                upstream,
            });
        }
    }

    entries.sort_by(|a, b| (branch_sort_key(a), &a.name).cmp(&(branch_sort_key(b), &b.name)));
    Ok(entries)
}

pub fn create_branch<P: AsRef<Path>>(
    repo_path: P,
    name: &str,
    start_point: Option<&str>,
) -> Result<(), Error> {
    let repo = Repository::open(repo_path)?;
    let commit = match start_point {
        Some(start_point) => repo.revparse_single(start_point)?.peel_to_commit()?,
        None => repo.head()?.peel_to_commit()?,
    };
    repo.branch(name, &commit, false)?;
    Ok(())
}

pub fn create_tracking_branch<P: AsRef<Path>>(
    repo_path: P,
    remote_branch: &str,
) -> Result<String, Error> {
    let repo = Repository::open(repo_path)?;
    let remote = repo.find_branch(remote_branch, BranchType::Remote)?;
    let commit = remote.into_reference().peel_to_commit()?;
    let local_name = local_branch_name(remote_branch);
    repo.branch(&local_name, &commit, false)?;
    let mut local_branch = repo.find_branch(&local_name, BranchType::Local)?;
    local_branch.set_upstream(Some(remote_branch))?;
    Ok(local_name)
}

#[allow(dead_code)]
pub fn set_upstream<P: AsRef<Path>>(
    repo_path: P,
    local: &str,
    upstream: &str,
) -> Result<(), Error> {
    let repo = Repository::open(repo_path)?;
    let mut branch = repo.find_branch(local, BranchType::Local)?;
    branch.set_upstream(Some(upstream))?;
    Ok(())
}

#[allow(dead_code)]
pub fn unset_upstream<P: AsRef<Path>>(repo_path: P, local: &str) -> Result<(), Error> {
    let repo = Repository::open(repo_path)?;
    let mut branch = repo.find_branch(local, BranchType::Local)?;
    branch.set_upstream(None)?;
    Ok(())
}

#[allow(dead_code)]
pub fn list_tracking_branches<P: AsRef<Path>>(
    repo_path: P,
) -> Result<Vec<TrackingBranch>, Error> {
    let repo = Repository::open(repo_path)?;
    let mut entries = Vec::new();
    for branch_result in repo.branches(Some(BranchType::Local))? {
        let (branch, _) = branch_result?;
        let Some(name) = branch.name()? else {
            continue;
        };
        let upstream = branch
            .upstream()
            .ok()
            .and_then(|upstream| upstream.name().ok().flatten().map(|s| s.to_string()));
        if let Some(upstream) = upstream {
            entries.push(TrackingBranch {
                local: name.to_string(),
                upstream,
            });
        }
    }
    entries.sort_by(|a, b| a.local.cmp(&b.local));
    Ok(entries)
}

pub fn delete_branch<P: AsRef<Path>>(repo_path: P, name: &str) -> Result<(), Error> {
    let repo = Repository::open(repo_path)?;
    let mut branch = repo.find_branch(name, BranchType::Local)?;
    branch.delete()?;
    Ok(())
}

pub fn rename_branch<P: AsRef<Path>>(repo_path: P, old: &str, new: &str) -> Result<(), Error> {
    let repo = Repository::open(repo_path)?;
    let mut branch = repo.find_branch(old, BranchType::Local)?;
    branch.rename(new, false)?;
    Ok(())
}

pub fn checkout_branch<P: AsRef<Path>>(repo_path: P, name: &str) -> Result<(), Error> {
    let repo = Repository::open(repo_path)?;
    let reference_name = format!("refs/heads/{name}");
    let (object, reference) = match repo.revparse_ext(&reference_name) {
        Ok(result) => result,
        Err(_) => repo.revparse_ext(name)?,
    };

    repo.checkout_tree(&object, Some(CheckoutBuilder::default().force()))?;
    if let Some(reference) = reference {
        if let Some(name) = reference.name() {
            repo.set_head(name)?;
        } else {
            repo.set_head_detached(object.id())?;
        }
    } else {
        repo.set_head_detached(object.id())?;
    }

    Ok(())
}

fn branch_sort_key(entry: &BranchEntry) -> usize {
    match entry.kind {
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
