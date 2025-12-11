use git2::{Repository, Signature};

#[derive(Debug, Clone)]
pub struct StashEntry {
    pub index: usize,
    pub message: String,
}

pub fn list_stashes(repo_path: &str) -> Result<Vec<StashEntry>, git2::Error> {
    let mut repo = Repository::open(repo_path)?;
    let mut entries = Vec::new();

    repo.stash_foreach(|index, name, _oid| {
        entries.push(StashEntry {
            index: index as usize,
            message: name.to_string(),
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
    let signature = Signature::now("GitSpace", "gitspace@example.com")?;
    let mut flags = git2::StashFlags::DEFAULT;
    if include_untracked {
        flags |= git2::StashFlags::INCLUDE_UNTRACKED;
    }

    repo.stash_save2(&signature, Some(message), Some(flags))?;
    Ok(())
}

pub fn apply_stash(repo_path: &str, index: usize) -> Result<(), git2::Error> {
    let mut repo = Repository::open(repo_path)?;
    repo.stash_apply(index, None)
}

pub fn drop_stash(repo_path: &str, index: usize) -> Result<(), git2::Error> {
    let mut repo = Repository::open(repo_path)?;
    repo.stash_drop(index)
}
