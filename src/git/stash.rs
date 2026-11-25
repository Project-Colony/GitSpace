use git2::{Diff, DiffOptions, Oid, Repository, Signature, StashApplyOptions, StatusOptions};

use crate::git::diff::{
    collect_diff_files, collect_diff_summaries, commit_against_parent, single_path_options,
    FileDiff, FileDiffSummary,
};
use crate::i18n::t;

#[derive(Debug, Clone)]
pub struct StashEntry {
    pub index: usize,
    pub message: String,
    /// The stash commit id, used to check that `index` still points to this stash.
    pub id: String,
    /// The branch the stash was made on; `None` for a detached HEAD.
    pub branch: Option<String>,
    /// Commit time of the stash, in seconds since the Unix epoch.
    pub time: i64,
    /// Files the stash changes against the commit it was made on, untracked files included.
    pub file_count: usize,
}

pub fn list_stashes(repo_path: &str) -> Result<Vec<StashEntry>, git2::Error> {
    let mut repo = Repository::open(repo_path)?;
    let mut found = Vec::new();
    repo.stash_foreach(|index, name, oid| {
        found.push((index, name.to_string(), *oid));
        true
    })?;

    found
        .into_iter()
        .map(|(index, message, oid)| {
            let file_count = stash_diff(&repo, oid, None)?.deltas().len();
            Ok(StashEntry {
                index,
                branch: stash_branch(&message),
                time: repo.find_commit(oid)?.time().seconds(),
                file_count,
                message,
                id: oid.to_string(),
            })
        })
        .collect()
}

/// The branch in git's `WIP on <branch>: ...` or `On <branch>: ...` stash message. Branch
/// names cannot contain `:`, so the first one ends the name.
fn stash_branch(message: &str) -> Option<String> {
    let rest = message
        .strip_prefix("WIP on ")
        .or_else(|| message.strip_prefix("On "))?;
    let (branch, _) = rest.split_once(':')?;
    (branch != "(no branch)").then(|| branch.to_string())
}

/// The stash `oid` against the commit it was made on. "Include untracked files" keeps the
/// untracked files in the stash's third parent, so they are added as new files.
fn stash_diff<'r>(
    repo: &'r Repository,
    oid: Oid,
    mut options: Option<&mut DiffOptions>,
) -> Result<Diff<'r>, git2::Error> {
    let mut diff = commit_against_parent(repo, oid, options.as_deref_mut())?;
    if let Ok(untracked) = repo.find_commit(oid)?.parent(2) {
        diff.merge(&repo.diff_tree_to_tree(None, Some(&untracked.tree()?), options)?)?;
    }
    Ok(diff)
}

/// Files of the stash at `index` against the commit it was made on, if it is still the
/// stash `id`.
pub fn stash_files(
    repo_path: &str,
    index: usize,
    id: &str,
) -> Result<Vec<FileDiffSummary>, git2::Error> {
    let mut repo = Repository::open(repo_path)?;
    ensure_stash_id(&mut repo, index, id)?;
    let diff = stash_diff(&repo, Oid::from_str(id)?, None)?;
    collect_diff_summaries(&diff)
}

/// The patch of `path` in the stash at `index`, if it is still the stash `id`.
pub fn stash_file_patch(
    repo_path: &str,
    index: usize,
    id: &str,
    path: &str,
) -> Result<Option<FileDiff>, git2::Error> {
    let mut repo = Repository::open(repo_path)?;
    ensure_stash_id(&mut repo, index, id)?;
    let mut options = single_path_options(path);
    let diff = stash_diff(&repo, Oid::from_str(id)?, Some(&mut options))?;
    Ok(collect_diff_files(diff)?.into_iter().next())
}

/// How many files a stash made now would take; ignored files never count.
pub fn count_stashable(repo_path: &str, include_untracked: bool) -> Result<usize, git2::Error> {
    let repo = Repository::open(repo_path)?;
    let mut options = StatusOptions::new();
    options
        .include_untracked(include_untracked)
        .recurse_untracked_dirs(include_untracked)
        .include_ignored(false);
    let count = repo.statuses(Some(&mut options))?.len();
    Ok(count)
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

    // With no message, git writes its own "WIP on <branch>: <commit>" one.
    let message = (!message.is_empty()).then_some(message);
    repo.stash_save2(&signature, message, Some(flags))?;
    Ok(())
}

/// Applies the stash at `index`, if it is still the stash `id`, and returns whether the
/// apply left conflicts. libgit2's default safe checkout fails instead of overwriting a
/// modified file, but a conflicting change is applied with markers and still succeeds.
pub fn apply_stash(repo_path: &str, index: usize, id: &str) -> Result<bool, git2::Error> {
    let mut repo = Repository::open(repo_path)?;
    ensure_stash_id(&mut repo, index, id)?;
    repo.stash_apply(index, Some(&mut StashApplyOptions::new()))?;
    let conflicts = repo.index()?.has_conflicts();
    Ok(conflicts)
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
        return Err(git2::Error::from_str(t("stash_list_changed")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::stash_branch;

    #[test]
    fn stash_branch_reads_both_message_forms() {
        assert_eq!(stash_branch("On main: msg").as_deref(), Some("main"));
        assert_eq!(
            stash_branch("WIP on feature/x: 1a2b3c4 summary").as_deref(),
            Some("feature/x")
        );
        assert_eq!(stash_branch("WIP on (no branch): 1a2b3c4 summary"), None);
        assert_eq!(stash_branch("custom message"), None);
    }
}
