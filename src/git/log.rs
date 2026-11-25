use std::collections::HashMap;

use git2::{BranchType, ErrorCode, Oid, Repository, Time};

use crate::git::diff::commit_against_parent;
// Utilise le cache thread-local pour éviter de réouvrir le dépôt à chaque appel
use crate::git::repo_cache::with_cached_repo;

#[derive(Debug, Clone, Default)]
pub struct CommitFilter {
    pub branch: Option<String>,
    pub author: Option<String>,
    pub search: Option<String>,
    pub since: Option<i64>,
    pub until: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub id: String,
    /// Pre-computed short ID (first 8 chars) to avoid repeated string slicing in UI
    pub short_id: String,
    pub summary: String,
    pub message: String,
    pub author: String,
    pub email: Option<String>,
    pub time: Time,
    pub files_changed: Option<usize>,
    pub additions: Option<usize>,
    pub deletions: Option<usize>,
    /// More than one parent marks a merge commit.
    #[cfg_attr(not(test), expect(dead_code, reason = "the History tab will read it"))]
    pub parent_count: usize,
    /// Branches and tags pointing at this commit; filled by `read_commit_log` only.
    pub refs: Vec<RefLabel>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefLabel {
    pub name: String,
    pub kind: RefKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RefKind {
    Local,
    Remote,
    Tag,
    /// The checked-out branch, or `HEAD` itself when detached.
    Head,
}

pub fn list_local_branches(repo_path: &str) -> Result<Vec<String>, git2::Error> {
    // Lecture seule des branches locales via le cache
    with_cached_repo(repo_path, |repo| {
        let mut branches = Vec::new();

        for branch in repo.branches(Some(BranchType::Local))? {
            let (branch, _) = branch?;
            if let Some(name) = branch.name()? {
                branches.push(name.to_string());
            }
        }

        branches.sort();
        branches.dedup();
        Ok(branches)
    })
}

pub fn read_commit_log(
    repo_path: &str,
    filter: &CommitFilter,
    limit: usize,
    include_stats: bool,
) -> Result<Vec<CommitInfo>, git2::Error> {
    // Parcours du journal de commits en lecture seule via le cache
    with_cached_repo(repo_path, |repo| {
        let mut revwalk = repo.revwalk()?;
        revwalk.set_sorting(git2::Sort::TIME | git2::Sort::TOPOLOGICAL)?;

        if let Some(branch) = &filter.branch {
            // Local branches first, then anything git can resolve, such as `origin/x`.
            match resolve_branch_oid(repo, branch)? {
                Some(oid) => revwalk.push(oid)?,
                None => return Ok(Vec::new()),
            }
        } else if repo
            .head()
            .is_err_and(|err| err.code() == ErrorCode::UnbornBranch)
        {
            // A fresh `git init` has no commit yet.
            return Ok(Vec::new());
        } else {
            revwalk.push_head()?;
        }

        let mut ref_map = build_ref_map(repo)?;

        // `limit` counts matching commits, so filters may walk further back.
        let mut commits = Vec::with_capacity(limit.min(256));
        for oid_result in revwalk {
            if commits.len() >= limit {
                break;
            }
            let commit = repo.find_commit(oid_result?)?;
            if !matches_filter(&commit, filter) {
                continue;
            }

            let mut info = commit_info_from_commit(&commit);
            // Each commit comes out of the walk once, so its labels can be moved out.
            info.refs = ref_map.remove(&commit.id()).unwrap_or_default();
            if include_stats {
                let stats = commit_against_parent(repo, commit.id(), None)?.stats()?;
                info.files_changed = Some(stats.files_changed());
                info.additions = Some(stats.insertions());
                info.deletions = Some(stats.deletions());
            }
            commits.push(info);
        }

        Ok(commits)
    })
}

/// Maps every commit some branch, remote branch or tag points at to its labels.
fn build_ref_map(repo: &Repository) -> Result<HashMap<Oid, Vec<RefLabel>>, git2::Error> {
    let head = repo.head().ok();
    let current_branch = head
        .as_ref()
        .filter(|head| head.is_branch())
        .and_then(|head| head.name().ok().map(str::to_string));

    let mut map: HashMap<Oid, Vec<RefLabel>> = HashMap::new();
    for reference in repo.references()? {
        let reference = reference?;
        // `refs/remotes/origin/HEAD` only repeats another remote branch.
        if reference.kind() == Some(git2::ReferenceType::Symbolic) {
            continue;
        }
        let kind = if reference.is_branch() {
            if reference.name().ok() == current_branch.as_deref() {
                RefKind::Head
            } else {
                RefKind::Local
            }
        } else if reference.is_remote() {
            RefKind::Remote
        } else if reference.is_tag() {
            RefKind::Tag
        } else {
            continue;
        };
        let (Ok(name), Ok(commit)) = (reference.shorthand(), reference.peel_to_commit()) else {
            continue;
        };
        map.entry(commit.id()).or_default().push(RefLabel {
            name: name.to_string(),
            kind,
        });
    }

    if let Some(oid) = head
        .filter(|head| !head.is_branch())
        .and_then(|head| head.target())
    {
        map.entry(oid).or_default().push(RefLabel {
            name: "HEAD".to_string(),
            kind: RefKind::Head,
        });
    }

    for labels in map.values_mut() {
        labels.sort_by(|a, b| (a.kind, &a.name).cmp(&(b.kind, &b.name)));
    }
    Ok(map)
}

fn matches_filter(commit: &git2::Commit<'_>, filter: &CommitFilter) -> bool {
    if let Some(author_filter) = &filter.author {
        let author_text = format!(
            "{} {}",
            commit.author().name().unwrap_or_default(),
            commit.author().email().unwrap_or_default()
        )
        .to_lowercase();
        if !author_text.contains(&author_filter.to_lowercase()) {
            return false;
        }
    }

    if let Some(search) = &filter.search {
        let search_lower = search.to_lowercase();
        let message = commit.message().unwrap_or_default().to_lowercase();
        let summary = commit
            .summary()
            .ok()
            .flatten()
            .unwrap_or_default()
            .to_lowercase();
        if !message.contains(&search_lower) && !summary.contains(&search_lower) {
            return false;
        }
    }

    let timestamp = commit.time().seconds();
    filter.since.is_none_or(|since| timestamp >= since)
        && filter.until.is_none_or(|until| timestamp <= until)
}

pub fn latest_commit_for_branch(
    repo_path: &str,
    branch_name: &str,
) -> Result<Option<CommitInfo>, git2::Error> {
    // Récupération du dernier commit d'une branche via le cache
    with_cached_repo(repo_path, |repo| {
        let oid = resolve_branch_oid(repo, branch_name)?;
        let Some(oid) = oid else {
            return Ok(None);
        };
        let commit = repo.find_commit(oid)?;
        Ok(Some(commit_info_from_commit(&commit)))
    })
}

pub fn commits_between_refs(
    repo_path: &str,
    from_ref: &str,
    to_ref: &str,
    limit: usize,
) -> Result<Vec<CommitInfo>, git2::Error> {
    // Comparaison de commits entre deux références via le cache
    with_cached_repo(repo_path, |repo| {
        let Some(to_oid) = resolve_ref_oid(repo, to_ref)? else {
            return Ok(Vec::new());
        };
        let from_oid = resolve_ref_oid(repo, from_ref)?;

        let mut revwalk = repo.revwalk()?;
        revwalk.set_sorting(git2::Sort::TIME | git2::Sort::TOPOLOGICAL)?;
        revwalk.push(to_oid)?;
        if let Some(from_oid) = from_oid {
            revwalk.hide(from_oid)?;
        }

        let mut commits = Vec::with_capacity(limit.min(64));
        for oid_result in revwalk.take(limit) {
            let oid = oid_result?;
            let commit = repo.find_commit(oid)?;
            commits.push(commit_info_from_commit(&commit));
        }

        Ok(commits)
    })
}

fn resolve_branch_oid(
    repo: &Repository,
    branch_name: &str,
) -> Result<Option<git2::Oid>, git2::Error> {
    let reference_names = [
        format!("refs/heads/{branch_name}"),
        format!("refs/remotes/{branch_name}"),
    ];
    for reference_name in reference_names {
        if let Ok(reference) = repo.find_reference(&reference_name) {
            if let Some(oid) = reference.target() {
                return Ok(Some(oid));
            }
        }
    }

    if let Ok(object) = repo.revparse_single(branch_name) {
        return Ok(Some(object.id()));
    }

    Ok(None)
}

fn resolve_ref_oid(repo: &Repository, reference: &str) -> Result<Option<git2::Oid>, git2::Error> {
    if let Ok(object) = repo.revparse_single(reference) {
        return Ok(Some(object.id()));
    }
    Ok(None)
}

fn commit_info_from_commit(commit: &git2::Commit<'_>) -> CommitInfo {
    let id = commit.id().to_string();
    let short_id = id.chars().take(8).collect();
    CommitInfo {
        id,
        short_id,
        summary: commit
            .summary()
            .ok()
            .flatten()
            .unwrap_or_default()
            .to_string(),
        message: commit.message().unwrap_or_default().to_string(),
        author: commit.author().name().unwrap_or("Unknown").to_string(),
        email: commit.author().email().ok().map(|s| s.to_string()),
        time: commit.time(),
        files_changed: None,
        additions: None,
        deletions: None,
        parent_count: commit.parent_count(),
        refs: Vec::new(),
    }
}
