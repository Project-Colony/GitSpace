use std::fs;
use std::path::Path;

use git2::build::CheckoutBuilder;
use git2::{
    BranchType, Commit, Oid, Repository, RepositoryInitOptions, ResetType, Signature,
    WorktreeAddOptions,
};

use crate::config::NetworkOptions;
use crate::git::branch;
use crate::git::branch::{
    BranchKind, list_branches, list_tracking_branches, rename_branch, set_upstream,
    unset_upstream,
};
use crate::git::discovery::{find_repo_root, is_git_repo, list_submodules, list_worktrees};
use crate::git::diff::{commit_diff, diff_file, staged_diff, working_tree_diff};
use crate::git::log::{CommitFilter, read_commit_log};
use crate::git::remote::{fetch_remote, list_remotes, pull_branch, prune_remotes, push_branch};
use crate::git::stash::{apply_stash, create_stash, drop_stash, list_stashes};
use crate::git::status::{read_repo_status, read_working_tree_status};

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
fn upstreams_are_set_and_listed() {
    let (_dir, repo) = init_temp_repo();
    let base = write_commit(&repo, "upstream.txt", "base", "base");
    let root = repo.path().parent().unwrap();

    branch::create_branch(root, "feature/upstream", Some("main")).expect("create branch");

    repo.remote("origin", "https://example.com/remote.git")
        .expect("create remote");
    let reference_name = "refs/remotes/origin/feature/upstream";
    repo.reference(reference_name, base, true, "create remote ref")
        .expect("remote ref");

    set_upstream(root, "feature/upstream", "origin/feature/upstream").expect("set upstream");
    let tracking = list_tracking_branches(root).expect("list tracking");
    assert!(tracking.iter().any(|entry| {
        entry.local == "feature/upstream" && entry.upstream == "origin/feature/upstream"
    }));

    unset_upstream(root, "feature/upstream").expect("unset upstream");
    let tracking = list_tracking_branches(root).expect("list tracking");
    assert!(!tracking.iter().any(|entry| entry.local == "feature/upstream"));
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
fn working_tree_and_staged_diffs_are_reported() {
    let (_dir, repo) = init_temp_repo();
    write_commit(&repo, "change.txt", "one\n", "initial");
    let repo_root = repo.path().parent().unwrap();
    let file_path = repo_root.join("change.txt");
    fs::write(&file_path, "one\ntwo\n").expect("update file");

    let unstaged = working_tree_diff(repo_root.to_str().unwrap()).expect("working tree diff");
    let unstaged_entry = unstaged
        .iter()
        .find(|diff| diff.path.ends_with("change.txt"))
        .expect("unstaged entry");
    assert!(unstaged_entry.patch.contains("+two"));

    let unstaged_file =
        diff_file(repo_root.to_str().unwrap(), "change.txt", false).expect("file diff");
    assert!(unstaged_file.is_some());

    let mut index = repo.index().expect("index");
    index.add_path(Path::new("change.txt")).expect("stage file");
    index.write().expect("write index");

    let staged = staged_diff(repo_root.to_str().unwrap()).expect("staged diff");
    let staged_entry = staged
        .iter()
        .find(|diff| diff.path.ends_with("change.txt"))
        .expect("staged entry");
    assert!(staged_entry.patch.contains("+two"));

    let staged_file =
        diff_file(repo_root.to_str().unwrap(), "change.txt", true).expect("staged file diff");
    assert!(staged_file.is_some());
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
fn fetch_push_pull_and_prune_work_with_local_remote() {
    let (_dir, repo) = init_temp_repo();
    let commit = write_commit(&repo, "push.txt", "content", "initial");

    let remote_dir = tempfile::tempdir().expect("create remote dir");
    let _remote_repo = Repository::init_bare(remote_dir.path()).expect("init bare");

    repo.remote(
        "origin",
        remote_dir.path().to_str().expect("remote path"),
    )
    .expect("add remote");

    let network = NetworkOptions::default();
    push_branch(
        repo.path().parent().unwrap(),
        "origin",
        "main",
        &network,
        None,
    )
    .expect("push");

    let fetch_dir = tempfile::tempdir().expect("create fetch dir");
    let fetch_repo = Repository::init(fetch_dir.path()).expect("init fetch repo");
    fetch_repo
        .remote(
            "origin",
            remote_dir.path().to_str().expect("remote path"),
        )
        .expect("add remote");

    fetch_remote(fetch_dir.path(), "origin", &network, None).expect("fetch");
    let fetch_repo = Repository::open(fetch_dir.path()).expect("open fetch repo");
    let remote_ref = fetch_repo
        .find_reference("refs/remotes/origin/main")
        .expect("remote ref");
    assert_eq!(remote_ref.target(), Some(commit));

    fetch_repo
        .reference("refs/remotes/origin/obsolete", commit, true, "obsolete")
        .expect("create obsolete ref");
    prune_remotes(fetch_dir.path(), "origin", &network, None).expect("prune");
    assert!(fetch_repo
        .find_reference("refs/remotes/origin/obsolete")
        .is_err());

    pull_branch(fetch_dir.path(), "origin", "main", &network, None).expect("pull");
    let head = fetch_repo.head().expect("head");
    assert_eq!(head.target(), Some(commit));
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

#[test]
fn working_tree_status_groups_files() {
    let (_dir, repo) = init_temp_repo();
    let root = repo.path().parent().unwrap();

    write_commit(&repo, "status.txt", "base", "initial");
    let head = repo.head().expect("head").peel_to_commit().expect("commit");
    repo.reset(head.as_object(), ResetType::Hard, None)
        .expect("reset head");

    let tracked_path = root.join("status.txt");
    fs::write(&tracked_path, "updated").expect("modify tracked file");

    let new_path = root.join("new_file.txt");
    fs::write(&new_path, "new content").expect("write new file");

    let status = read_working_tree_status(root).expect("working tree status");

    assert!(status.staged.is_empty());
    assert_eq!(status.unstaged, vec!["status.txt".to_string()]);
    assert_eq!(status.untracked, vec!["new_file.txt".to_string()]);
    assert!(status.conflicted.is_empty());
}

#[test]
fn repo_discovery_and_contents_are_reported() {
    let (_dir, repo) = init_temp_repo();
    write_commit(&repo, "README.md", "hello", "initial");
    repo.set_head("refs/heads/main").expect("set head");

    let repo_root = repo.path().parent().unwrap();
    let nested = repo_root.join("nested");
    fs::create_dir_all(&nested).expect("create nested");

    assert!(is_git_repo(repo_root));
    assert!(is_git_repo(&nested));

    let other_dir = tempfile::tempdir().expect("other temp dir");
    assert!(!is_git_repo(other_dir.path()));

    let discovered = find_repo_root(&nested).expect("find repo root");
    assert_eq!(discovered.as_deref(), Some(repo_root));
    let none = find_repo_root(other_dir.path()).expect("no repo root");
    assert!(none.is_none());
}

#[test]
fn worktrees_and_submodules_are_listed() {
    let (_dir, repo) = init_temp_repo();
    write_commit(&repo, "README.md", "hello", "initial");

    let sub_repo_dir = tempfile::tempdir().expect("submodule repo dir");
    let sub_repo = Repository::init(sub_repo_dir.path()).expect("init submodule repo");
    write_commit(&sub_repo, "lib.txt", "submodule", "init submodule");

    let submodule_path = repo.path().parent().unwrap().join("vendor/submodule");
    fs::create_dir_all(&submodule_path).expect("create submodule path");

    let gitmodules = format!(
        "[submodule \"vendor/submodule\"]\n\tpath = vendor/submodule\n\turl = {}\n",
        sub_repo_dir.path().to_str().expect("submodule url")
    );
    write_commit(&repo, ".gitmodules", &gitmodules, "add submodule");

    let submodules = list_submodules(repo.path().parent().unwrap()).expect("list submodules");
    assert_eq!(submodules.len(), 1);
    assert_eq!(submodules[0].path, "vendor/submodule");
    assert_eq!(
        submodules[0].url.as_deref(),
        Some(sub_repo_dir.path().to_str().expect("submodule url"))
    );

    let worktree_parent = tempfile::tempdir().expect("worktree dir");
    let worktree_dir = worktree_parent.path().join("worktree");
    let opts = WorktreeAddOptions::new();
    repo.worktree("worktree-feature", &worktree_dir, Some(&opts))
        .expect("create worktree");

    let worktrees = list_worktrees(repo.path().parent().unwrap()).expect("list worktrees");
    let worktree_path = worktree_dir.to_string_lossy().to_string();
    assert!(worktrees.iter().any(|path| path == &worktree_path));
}
