use git2::{ErrorCode, Oid, Repository, RepositoryState};

use crate::i18n::{t, tf};

/// Commits the index on HEAD with the identity from the Git config. A first commit on an
/// unborn branch has no parent. Refuses an empty message, an index that matches HEAD, and a
/// merge or rebase in progress (this commit would drop the second parent).
pub fn commit_index(repo_path: &str, message: &str) -> Result<Oid, git2::Error> {
    if message.trim().is_empty() {
        return Err(git2::Error::from_str(t("commit_message_empty")));
    }
    let repo = Repository::open(repo_path)?;
    if repo.state() != RepositoryState::Clean {
        return Err(git2::Error::from_str(t("commit_operation_in_progress")));
    }

    let tree = repo.find_tree(repo.index()?.write_tree()?)?;
    let parent = match repo.head() {
        Ok(head) => Some(head.peel_to_commit()?),
        Err(err) if err.code() == ErrorCode::UnbornBranch => None,
        Err(err) => return Err(err),
    };
    let unchanged = match &parent {
        Some(parent) => parent.tree_id() == tree.id(),
        None => tree.is_empty(),
    };
    if unchanged {
        return Err(git2::Error::from_str(t("commit_nothing_staged")));
    }

    let signature = repo.signature().map_err(|err| {
        git2::Error::from_str(&tf("commit_identity_missing", &[("error", err.message())]))
    })?;
    let message = git2::message_prettify(message, None)?;
    let parents: Vec<_> = parent.iter().collect();
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        &message,
        &tree,
        &parents,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn repo_with_identity() -> (tempfile::TempDir, Repository) {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "Test User").unwrap();
        config.set_str("user.email", "test@example.com").unwrap();
        (dir, repo)
    }

    fn stage(repo: &Repository, name: &str, contents: &str) {
        std::fs::write(repo.workdir().unwrap().join(name), contents).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(name)).unwrap();
        index.write().unwrap();
    }

    #[test]
    fn first_commit_has_no_parent_and_second_has_one() {
        let (dir, repo) = repo_with_identity();
        let path = dir.path().to_str().unwrap();

        stage(&repo, "a.txt", "one");
        let first = commit_index(path, "first").unwrap();
        let first = repo.find_commit(first).unwrap();
        assert_eq!(first.parent_count(), 0);
        assert_eq!(first.message().ok(), Some("first\n"));
        assert_eq!(first.author().name().ok(), Some("Test User"));

        stage(&repo, "a.txt", "two");
        let second = repo
            .find_commit(commit_index(path, "second").unwrap())
            .unwrap();
        assert_eq!(second.parent_id(0).unwrap(), first.id());
        assert_eq!(repo.head().unwrap().target(), Some(second.id()));
    }

    #[test]
    fn empty_message_and_empty_change_are_refused() {
        let (dir, repo) = repo_with_identity();
        let path = dir.path().to_str().unwrap();

        // Unborn branch with an empty index.
        assert!(commit_index(path, "nothing").is_err());

        stage(&repo, "a.txt", "one");
        let err = commit_index(path, "  \n").unwrap_err();
        assert_eq!(err.message(), t("commit_message_empty"));
        commit_index(path, "first").unwrap();

        // The index matches HEAD.
        let err = commit_index(path, "again").unwrap_err();
        assert_eq!(err.message(), t("commit_nothing_staged"));
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(head.message().ok(), Some("first\n"));
    }
}
