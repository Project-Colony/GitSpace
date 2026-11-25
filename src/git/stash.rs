use git2::{Repository, Signature, StashApplyOptions};

#[derive(Debug, Clone)]
pub struct StashEntry {
    pub index: usize,
    pub message: String,
    /// The stash commit id, used to check that `index` still points to this stash.
    pub id: String,
}

pub fn list_stashes(repo_path: &str) -> Result<Vec<StashEntry>, git2::Error> {
    let mut repo = Repository::open(repo_path)?;
    let mut entries = Vec::new();

    repo.stash_foreach(|index, name, oid| {
        entries.push(StashEntry {
            index,
            message: name.to_string(),
            id: oid.to_string(),
        });
        true
    })?;

    Ok(entries)
}

pub fn create_stash(
    repo_path: &str,
    message: &str,
    include_untracked: bool,
) -> Result<(), git2::Error> {
    let mut repo = Repository::open(repo_path)?;
    let signature = repo
        .signature()
        .or_else(|_| Signature::now("GitSpace", "gitspace@example.com"))?;
    let mut flags = git2::StashFlags::DEFAULT;
    if include_untracked {
        flags |= git2::StashFlags::INCLUDE_UNTRACKED;
    }

    repo.stash_save2(&signature, Some(message), Some(flags))?;
    Ok(())
}

/// Applies the stash at `index`, if it is still the stash `id`. libgit2's default safe
/// checkout fails instead of overwriting a modified file.
pub fn apply_stash(repo_path: &str, index: usize, id: &str) -> Result<(), git2::Error> {
    let mut repo = Repository::open(repo_path)?;
    ensure_stash_id(&mut repo, index, id)?;
    repo.stash_apply(index, Some(&mut StashApplyOptions::new()))
}

/// Drops the stash at `index`, if it is still the stash `id`.
pub fn drop_stash(repo_path: &str, index: usize, id: &str) -> Result<(), git2::Error> {
    let mut repo = Repository::open(repo_path)?;
    ensure_stash_id(&mut repo, index, id)?;
    repo.stash_drop(index)
}

/// Fails when the stash list changed since `id` was read at `index`.
fn ensure_stash_id(repo: &mut Repository, index: usize, id: &str) -> Result<(), git2::Error> {
    let mut matches = false;
    repo.stash_foreach(|i, _, oid| {
        matches |= i == index && oid.to_string() == id;
        true
    })?;
    if !matches {
        return Err(git2::Error::from_str(
            "The stash list changed; refresh and try again.",
        ));
    }
    Ok(())
}
