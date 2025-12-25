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

pub fn create_branch<P: AsRef<Path>>(repo_path: P, name: &str) -> Result<(), Error> {
    let repo = Repository::open(repo_path)?;
    let head = repo.head()?;
    let commit = head.peel_to_commit()?;
    repo.branch(name, &commit, false)?;
    Ok(())
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
