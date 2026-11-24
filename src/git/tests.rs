use std::fs;
use std::path::Path;

use git2::build::CheckoutBuilder;
use git2::{BranchType, Commit, Oid, Repository, RepositoryInitOptions, ResetType, Signature};

use crate::config::NetworkOptions;
use crate::git::branch;
use crate::git::branch::{list_branches, rename_branch, BranchKind};
use crate::git::diff::{commit_diff_file, commit_diff_summaries, diff_file};
use crate::git::log::{read_commit_log, CommitFilter};
use crate::git::merge::detect_conflicts;
use crate::git::remote::{fetch_remote, list_remotes, pull_branch, push_branch, PullOutcome};
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
    index.write().expect("write index");
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

    branch::create_branch(root, "feature/test", None).expect("create branch");
    let mut branches = list_branches(root).expect("list branches");
    branches.sort_by(|a, b| a.name.cmp(&b.name));
    assert!(branches
        .iter()
        .any(|entry| entry.name == "feature/test" && entry.kind == BranchKind::Local));

    rename_branch(root, "feature/test", "feature/renamed").expect("rename");
    let branches = list_branches(root).expect("list branches");
    assert!(branches
        .iter()
        .any(|entry| entry.name == "feature/renamed" && entry.kind == BranchKind::Local));

    branch::delete_branch(root, "feature/renamed").expect("delete");
    let remaining = list_branches(root).expect("list branches");
    assert!(!remaining.iter().any(|entry| entry.name.contains("feature")));
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
    let commits = read_commit_log(
        repo.path().parent().unwrap().to_str().unwrap(),
        &filter,
        10,
        false,
    )
    .expect("log");

    assert_eq!(commits.len(), 1);
    assert!(commits[0].summary.to_lowercase().contains("beta"));
}

#[test]
fn commit_logs_can_include_diff_stats() {
    let (_dir, repo) = init_temp_repo();
    write_commit(&repo, "stats.txt", "one\n", "initial");
    write_commit(&repo, "stats.txt", "one\ntwo\n", "add line");

    let commits = read_commit_log(
        repo.path().parent().unwrap().to_str().unwrap(),
        &CommitFilter::default(),
        1,
        true,
    )
    .expect("log");

    assert_eq!(commits.len(), 1);
    let stats = &commits[0];
    assert_eq!(stats.files_changed, Some(1));
    assert_eq!(stats.additions, Some(1));
    assert_eq!(stats.deletions, Some(0));
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
fn fetch_push_and_pull_work_with_local_remote() {
    let (_dir, repo) = init_temp_repo();
    let commit = write_commit(&repo, "push.txt", "content", "initial");

    let remote_dir = tempfile::tempdir().expect("create remote dir");
    let _remote_repo = Repository::init_bare(remote_dir.path()).expect("init bare");

    repo.remote("origin", remote_dir.path().to_str().expect("remote path"))
        .expect("add remote");

    let network = NetworkOptions::default();
    push_branch(
        repo.path().parent().unwrap(),
        "origin",
        "main",
        "main",
        &network,
        None,
    )
    .expect("push");

    let fetch_dir = tempfile::tempdir().expect("create fetch dir");
    let mut init = RepositoryInitOptions::new();
    init.initial_head("main");
    let fetch_repo = Repository::init_opts(fetch_dir.path(), &init).expect("init fetch repo");
    fetch_repo
        .remote("origin", remote_dir.path().to_str().expect("remote path"))
        .expect("add remote");

    fetch_remote(fetch_dir.path(), "origin", &network, None).expect("fetch");
    let fetch_repo = Repository::open(fetch_dir.path()).expect("open fetch repo");
    let remote_ref = fetch_repo
        .find_reference("refs/remotes/origin/main")
        .expect("remote ref");
    assert_eq!(remote_ref.target(), Some(commit));

    let outcome =
        pull_branch(fetch_dir.path(), "origin", "main", "main", &network, None).expect("pull");
    assert_eq!(outcome, PullOutcome::FastForward);
    let head = fetch_repo.head().expect("head");
    assert_eq!(head.target(), Some(commit));
}

/// A clone of a bare remote on `main`, with a `feature` branch checked out that tracks
/// `origin/main`, and the remote one commit ahead.
fn clone_with_feature_tracking_main() -> (tempfile::TempDir, tempfile::TempDir, Repository) {
    let (work_dir, work) = init_temp_repo();
    write_commit(&work, "shared.txt", "one\n", "initial");
    let remote_dir = tempfile::tempdir().expect("remote dir");
    let mut init = RepositoryInitOptions::new();
    init.bare(true).initial_head("main");
    Repository::init_opts(remote_dir.path(), &init).expect("init bare");
    let remote_url = remote_dir.path().to_str().unwrap();
    work.remote("origin", remote_url).expect("add remote");
    let network = NetworkOptions::default();
    let work_root = work.path().parent().unwrap().to_path_buf();
    push_branch(&work_root, "origin", "main", "main", &network, None).expect("push");
    fetch_remote(&work_root, "origin", &network, None).expect("fetch");

    let head = work.head().unwrap().peel_to_commit().unwrap().id();
    work.branch("feature", &work.find_commit(head).unwrap(), false)
        .expect("feature");
    work.set_head("refs/heads/feature").expect("switch");
    ensure_remote_tracking(&work, "feature", "origin/main", head);

    // Advance origin/main from a second clone.
    let other_dir = tempfile::tempdir().expect("other dir");
    let other = Repository::clone(remote_url, other_dir.path()).expect("clone");
    write_commit(&other, "shared.txt", "one\ntwo\n", "remote change");
    push_branch(other_dir.path(), "origin", "main", "main", &network, None).expect("push");
    (work_dir, remote_dir, work)
}

#[test]
fn pull_fast_forwards_the_checked_out_branch_only() {
    let (_work_dir, _remote_dir, work) = clone_with_feature_tracking_main();
    let root = work.path().parent().unwrap();
    let main_before = work.refname_to_id("refs/heads/main").unwrap();
    let network = NetworkOptions::default();

    let outcome = pull_branch(root, "origin", "feature", "main", &network, None).expect("pull");
    assert_eq!(outcome, PullOutcome::FastForward);
    assert_eq!(
        work.find_reference("HEAD")
            .unwrap()
            .symbolic_target()
            .unwrap(),
        Some("refs/heads/feature")
    );
    assert_eq!(
        work.refname_to_id("refs/heads/feature").unwrap(),
        work.refname_to_id("refs/remotes/origin/main").unwrap()
    );
    assert_eq!(work.refname_to_id("refs/heads/main").unwrap(), main_before);
    assert_eq!(
        fs::read_to_string(root.join("shared.txt")).unwrap(),
        "one\ntwo\n"
    );

    // A branch that is not checked out is refused.
    assert!(pull_branch(root, "origin", "main", "main", &network, None).is_err());
}

#[test]
fn pull_refuses_to_overwrite_uncommitted_changes() {
    let (_work_dir, _remote_dir, work) = clone_with_feature_tracking_main();
    let root = work.path().parent().unwrap();
    let feature_before = work.refname_to_id("refs/heads/feature").unwrap();
    fs::write(root.join("shared.txt"), "local edit\n").unwrap();

    let result = pull_branch(
        root,
        "origin",
        "feature",
        "main",
        &NetworkOptions::default(),
        None,
    );
    assert!(result.is_err());
    assert_eq!(
        fs::read_to_string(root.join("shared.txt")).unwrap(),
        "local edit\n"
    );
    assert_eq!(
        work.refname_to_id("refs/heads/feature").unwrap(),
        feature_before
    );
}

#[test]
fn push_maps_the_local_branch_to_the_upstream_name() {
    let (_dir, repo) = init_temp_repo();
    let commit = write_commit(&repo, "a.txt", "a", "initial");
    let remote_dir = tempfile::tempdir().expect("remote dir");
    let remote = Repository::init_bare(remote_dir.path()).expect("init bare");
    repo.remote("origin", remote_dir.path().to_str().unwrap())
        .expect("add remote");

    push_branch(
        repo.path().parent().unwrap(),
        "origin",
        "main",
        "release",
        &NetworkOptions::default(),
        None,
    )
    .expect("push");

    assert_eq!(remote.refname_to_id("refs/heads/release").unwrap(), commit);
    assert!(remote.find_reference("refs/heads/main").is_err());
}

#[test]
fn remotes_report_a_separate_push_url() {
    let (_dir, repo) = init_temp_repo();
    repo.remote("origin", "https://example.com/fetch.git")
        .expect("create remote");
    repo.remote_set_pushurl("origin", Some("https://push.example.com/push.git"))
        .expect("set push url");

    let remotes = list_remotes(repo.path().parent().unwrap()).expect("list remotes");
    assert_eq!(
        remotes[0].push_url.as_deref(),
        Some("https://push.example.com/push.git")
    );
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

    apply_stash(working_dir.to_str().unwrap(), 0, &stashes[0].id).expect("apply stash");
    drop_stash(working_dir.to_str().unwrap(), 0, &stashes[0].id).expect("drop stash");

    let restored = fs::read_to_string(&file_path).expect("read file");
    assert_eq!(restored, "modified");
}

#[test]
fn stash_drop_refuses_a_changed_stash_list() {
    let (_dir, repo) = init_temp_repo();
    let root = repo.path().parent().unwrap();
    let path = root.to_str().unwrap();
    write_commit(&repo, "a.txt", "base", "initial");
    fs::write(root.join("a.txt"), "first").unwrap();
    create_stash(path, "first", false).expect("stash");
    let first = list_stashes(path).unwrap().remove(0);

    // Another stash pushes the first one to index 1.
    fs::write(root.join("a.txt"), "second").unwrap();
    create_stash(path, "second", false).expect("stash");

    let err = drop_stash(path, first.index, &first.id).expect_err("stale index");
    assert!(err.message().contains("refresh"), "{}", err.message());
    assert_eq!(list_stashes(path).unwrap().len(), 2);
    assert!(apply_stash(path, first.index, &first.id).is_err());
}

#[test]
fn stash_apply_keeps_edits_in_other_files() {
    let (_dir, repo) = init_temp_repo();
    let root = repo.path().parent().unwrap();
    let path = root.to_str().unwrap();
    write_commit(&repo, "a.txt", "a", "add a");
    write_commit(&repo, "b.txt", "b", "add b");
    fs::write(root.join("a.txt"), "stashed").unwrap();
    create_stash(path, "a", false).expect("stash");
    fs::write(root.join("b.txt"), "unsaved").unwrap();

    let stash = list_stashes(path).unwrap().remove(0);
    apply_stash(path, stash.index, &stash.id).expect("apply");

    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "stashed");
    assert_eq!(fs::read_to_string(root.join("b.txt")).unwrap(), "unsaved");
}

#[test]
fn stash_uses_the_configured_signature() {
    let (_dir, repo) = init_temp_repo();
    let root = repo.path().parent().unwrap();
    let mut config = repo.config().unwrap();
    config.set_str("user.name", "Ada").unwrap();
    config.set_str("user.email", "ada@example.com").unwrap();
    write_commit(&repo, "a.txt", "a", "initial");
    fs::write(root.join("a.txt"), "changed").unwrap();

    create_stash(root.to_str().unwrap(), "sig", false).expect("stash");
    let stash = repo.refname_to_id("refs/stash").unwrap();
    let commit = repo.find_commit(stash).unwrap();
    assert_eq!(commit.author().email().unwrap(), "ada@example.com");
}

#[test]
fn checkout_refuses_to_overwrite_a_modified_file() {
    let (_dir, repo) = init_temp_repo();
    let root = repo.path().parent().unwrap();
    let first = write_commit(&repo, "a.txt", "one", "initial");
    repo.branch("other", &repo.find_commit(first).unwrap(), false)
        .unwrap();
    write_commit(&repo, "a.txt", "two", "change");
    fs::write(root.join("a.txt"), "dirty").unwrap();

    assert!(branch::checkout_branch(root, "other").is_err());
    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "dirty");
    assert_eq!(repo.head().unwrap().shorthand().unwrap(), "main");
}

#[test]
fn commit_diff_summaries_count_lines() {
    let (_dir, repo) = init_temp_repo();
    let path = repo.path().parent().unwrap().to_str().unwrap().to_string();
    write_commit(&repo, "lines.txt", "one\nold\n", "initial");
    let second = write_commit(&repo, "lines.txt", "one\ntwo\nthree\n", "edit");

    let summaries = commit_diff_summaries(&path, &second.to_string()).expect("summaries");
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].path, "lines.txt");
    assert_eq!((summaries[0].additions, summaries[0].deletions), (2, 1));
    assert!(!summaries[0].is_binary);
}

#[test]
fn patch_text_prefixes_only_content_lines() {
    let (_dir, repo) = init_temp_repo();
    let path = repo.path().parent().unwrap().to_str().unwrap().to_string();
    write_commit(&repo, "p.txt", "a\n", "initial");
    let second = write_commit(&repo, "p.txt", "a\nb", "no newline at end");

    let diff = commit_diff_file(&path, &second.to_string(), "p.txt")
        .expect("diff")
        .expect("entry");
    let lines: Vec<&str> = diff.patch.lines().collect();
    assert!(lines[0].starts_with("diff --git"), "{}", diff.patch);
    assert!(lines.iter().any(|line| line.starts_with("@@")));
    assert!(lines.contains(&"+b"));
    assert!(lines.contains(&"\\ No newline at end of file"));
}

#[test]
fn single_file_diffs_take_the_path_literally() {
    let (_dir, repo) = init_temp_repo();
    let root = repo.path().parent().unwrap();
    let path = root.to_str().unwrap();
    write_commit(&repo, "d.md", "other\n", "decoy");
    let commit = write_commit(&repo, "[draft].md", "draft\n", "add draft");

    let in_commit = commit_diff_file(path, &commit.to_string(), "[draft].md")
        .expect("commit diff")
        .expect("entry");
    assert!(
        in_commit.patch.contains("[draft].md"),
        "{}",
        in_commit.patch
    );

    fs::write(root.join("[draft].md"), "draft\nmore\n").unwrap();
    fs::write(root.join("d.md"), "changed\n").unwrap();
    let in_workdir = diff_file(path, "[draft].md", false)
        .expect("diff")
        .expect("entry");
    assert!(
        in_workdir.patch.contains("[draft].md"),
        "{}",
        in_workdir.patch
    );
}

#[test]
fn log_limit_counts_matching_commits() {
    let (_dir, repo) = init_temp_repo();
    let path = repo.path().parent().unwrap().to_str().unwrap().to_string();
    write_commit(&repo, "f.txt", "0", "needle in the oldest commit");
    for i in 1..5 {
        write_commit(&repo, "f.txt", &i.to_string(), &format!("hay {i}"));
    }

    let filter = CommitFilter {
        search: Some("needle".to_string()),
        ..Default::default()
    };
    let commits = read_commit_log(&path, &filter, 2, false).expect("log");
    assert_eq!(commits.len(), 1);
    assert!(commits[0].summary.contains("needle"));
}

#[test]
fn log_branch_filter_accepts_remote_branches() {
    let (_dir, repo) = init_temp_repo();
    let path = repo.path().parent().unwrap().to_str().unwrap().to_string();
    let first = write_commit(&repo, "f.txt", "1", "first");
    write_commit(&repo, "f.txt", "2", "second");
    repo.reference("refs/remotes/origin/x", first, true, "test")
        .unwrap();

    let filter = CommitFilter {
        branch: Some("origin/x".to_string()),
        ..Default::default()
    };
    let commits = read_commit_log(&path, &filter, 10, false).expect("log");
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].id, first.to_string());
}

#[test]
fn unborn_repository_has_a_branch_and_an_empty_log() {
    let (_dir, repo) = init_temp_repo();
    let root = repo.path().parent().unwrap();

    let status = read_repo_status(root).expect("status");
    assert_eq!(status.branch.as_deref(), Some("main"));
    assert!(status.upstream.is_none());

    let log =
        read_commit_log(root.to_str().unwrap(), &CommitFilter::default(), 10, false).expect("log");
    assert!(log.is_empty());
}

#[test]
fn conflicts_written_by_another_handle_are_detected() {
    let (_dir, repo) = init_temp_repo();
    let root = repo.path().parent().unwrap();
    let base = write_commit(&repo, "c.txt", "base\n", "base");
    repo.branch("other", &repo.find_commit(base).unwrap(), false)
        .unwrap();
    write_commit(&repo, "c.txt", "ours\n", "ours");

    // Caches the repository with its clean index.
    assert!(detect_conflicts(root).unwrap().is_empty());

    // Commit on `other` and merge it from a separate handle, as `git merge` would.
    let mut tree = repo.treebuilder(None).unwrap();
    tree.insert("c.txt", repo.blob(b"theirs\n").unwrap(), 0o100644)
        .unwrap();
    let tree = repo.find_tree(tree.write().unwrap()).unwrap();
    let sig = Signature::now("Tester", "tester@example.com").unwrap();
    let parent = repo.find_commit(base).unwrap();
    let theirs = repo
        .commit(
            Some("refs/heads/other"),
            &sig,
            &sig,
            "theirs",
            &tree,
            &[&parent],
        )
        .unwrap();
    let annotated = repo.find_annotated_commit(theirs).unwrap();
    repo.merge(&[&annotated], None, None).unwrap();

    assert_eq!(detect_conflicts(root).unwrap(), vec!["c.txt".to_string()]);
}
