use std::fs;
use std::path::Path;

use git2::build::CheckoutBuilder;
use git2::{BranchType, Commit, Oid, Repository, RepositoryInitOptions, ResetType, Signature};

use crate::git::branch;
use crate::git::branch::{BranchKind, list_branches, rename_branch};
use crate::git::diff::commit_diff;
use crate::git::log::{CommitFilter, read_commit_log};
use crate::git::remote::list_remotes;
use crate::git::stash::{apply_stash, create_stash, drop_stash, list_stashes};
use crate::git::status::read_repo_status;

fn init_temp_repo() -> (tempfile::TempDir, Repository) {
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let mut options = RepositoryInitOptions::new();
    options.initial_head("main");
    let repo = Repository::init_opts(temp_dir.path(), &options).expect("init repo");
    (temp_dir, repo)
}

fn write_commit(repo: &Repository, path: impl AsRef<Path>, contents: &str, message: &str) -> Oid {
    let file_path = repo.path().parent().unwrap().join(path.as_ref());
    fs::write(&file_path, contents).expect("write file");

    let mut index = repo.index().expect("index");
    index.add_path(path.as_ref()).expect("add path");
    let tree_id = index.write_tree().expect("write tree");
    let tree = repo.find_tree(tree_id).expect("find tree");

    let sig = Signature::now("Tester", "tester@example.com").expect("signature");
    let parents: Vec<Commit> = repo
        .head()
        .ok()
        .and_then(|head| head.target())
        .and_then(|oid| repo.find_commit(oid).ok())
        .into_iter()
        .collect();

    let parent_refs: Vec<&Commit> = parents.iter().collect();

    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parent_refs)
        .expect("commit")
}

fn ensure_remote_tracking(repo: &Repository, branch: &str, upstream: &str, commit: Oid) {
    if repo.find_remote("origin").is_err() {
        repo.remote("origin", "https://example.com/remote.git")
            .expect("create remote");
    }

    let reference_name = format!("refs/remotes/{upstream}");
    repo.reference(&reference_name, commit, true, "create remote ref")
        .expect("remote ref");

    let mut local_branch = repo
        .find_branch(branch, BranchType::Local)
        .expect("local branch");
    local_branch
        .set_upstream(Some(upstream))
        .expect("set upstream");
}

#[test]
fn repo_status_reports_tracking_progress() {
    let (_dir, repo) = init_temp_repo();

    let first = write_commit(&repo, "README.md", "hello", "initial");
    ensure_remote_tracking(&repo, "main", "origin/main", first);

    let _second = write_commit(&repo, "README.md", "hello world", "ahead");

    let status = read_repo_status(repo.path().parent().unwrap()).expect("status");

    assert_eq!(status.branch.as_deref(), Some("main"));
    assert_eq!(status.upstream.as_deref(), Some("origin/main"));
    assert_eq!(status.ahead, Some(1));
    assert_eq!(status.behind, Some(0));
}

#[test]
fn branch_lifecycle_is_managed() {
    let (_dir, repo) = init_temp_repo();
    write_commit(&repo, "file.txt", "content", "base");
    let root = repo.path().parent().unwrap();

    branch::create_branch(root, "feature/test").expect("create branch");
    let mut branches = list_branches(root).expect("list branches");
    branches.sort_by(|a, b| a.name.cmp(&b.name));
    assert!(
        branches
            .iter()
            .any(|entry| entry.name == "feature/test" && entry.kind == BranchKind::Local)
    );

    rename_branch(root, "feature/test", "feature/renamed").expect("rename");
    let branches = list_branches(root).expect("list branches");
    assert!(
        branches
            .iter()
            .any(|entry| entry.name == "feature/renamed" && entry.kind == BranchKind::Local)
    );

    branch::delete_branch(root, "feature/renamed").expect("delete");
    let remaining = list_branches(root).expect("list branches");
    assert!(!remaining.iter().any(|entry| entry.name.contains("feature")));
}

#[test]
fn commit_diff_tracks_file_changes() {
    let (_dir, repo) = init_temp_repo();
    write_commit(&repo, "diff.txt", "one", "initial");
    let second = write_commit(&repo, "diff.txt", "one\ntwo\nthree\n", "expand");

    let diffs = commit_diff(
        repo.path().parent().unwrap().to_str().unwrap(),
        &second.to_string(),
    )
    .expect("diffs");
    let entry = diffs
        .iter()
        .find(|diff| diff.path.ends_with("diff.txt"))
        .expect("diff entry");

    assert!(entry.additions >= 2);
    assert!(entry.deletions <= 1);
    assert!(entry.patch.contains("+two"));
    assert!(entry.patch.contains("diff.txt"));
}

#[test]
fn commit_logs_respect_filters() {
    let (_dir, repo) = init_temp_repo();
    write_commit(&repo, "log.txt", "alpha", "alpha commit");
    write_commit(&repo, "log.txt", "beta", "beta feature");

    let filter = CommitFilter {
        search: Some("beta".to_string()),
        ..Default::default()
    };
    let commits =
        read_commit_log(repo.path().parent().unwrap().to_str().unwrap(), &filter, 10).expect("log");

    assert_eq!(commits.len(), 1);
    assert!(commits[0].summary.to_lowercase().contains("beta"));
}

#[test]
fn remotes_are_discovered() {
    let (_dir, repo) = init_temp_repo();
    repo.remote("origin", "https://example.com/remote.git")
        .expect("create remote");

    let remotes = list_remotes(repo.path().parent().unwrap()).expect("list remotes");
    assert_eq!(remotes.len(), 1);
    assert_eq!(remotes[0].name, "origin");
}

#[test]
fn stashes_round_trip_changes() {
    let (_dir, repo) = init_temp_repo();
    let working_dir = repo.path().parent().unwrap();
    write_commit(&repo, "stash.txt", "first", "initial");

    let file_path = working_dir.join("stash.txt");
    fs::write(&file_path, "modified").expect("modify file");

    create_stash(working_dir.to_str().unwrap(), "test stash", true).expect("stash create");
    let mut checkout = CheckoutBuilder::new();
    repo.checkout_head(Some(&mut checkout.force()))
        .expect("clean checkout");
    let head = repo.head().expect("head").peel_to_commit().expect("commit");
    repo.reset(head.as_object(), ResetType::Hard, None)
        .expect("reset head");

    let stashes = list_stashes(working_dir.to_str().unwrap()).expect("list stashes");
    assert_eq!(stashes.len(), 1);
    assert!(stashes[0].message.contains("test stash"));

    apply_stash(working_dir.to_str().unwrap(), 0).expect("apply stash");
    drop_stash(working_dir.to_str().unwrap(), 0).expect("drop stash");

    let restored = fs::read_to_string(&file_path).expect("read file");
    assert_eq!(restored, "modified");
}
