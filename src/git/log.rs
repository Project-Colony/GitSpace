use git2::{BranchType, Repository, Time};

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
    pub summary: String,
    pub message: String,
    pub author: String,
    pub email: Option<String>,
    pub time: Time,
    pub parents: Vec<String>,
    pub files_changed: Option<usize>,
    pub additions: Option<usize>,
    pub deletions: Option<usize>,
}

pub fn list_local_branches(repo_path: &str) -> Result<Vec<String>, git2::Error> {
    let repo = Repository::open(repo_path)?;
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
}

pub fn read_commit_log(
    repo_path: &str,
    filter: &CommitFilter,
    limit: usize,
    include_stats: bool,
) -> Result<Vec<CommitInfo>, git2::Error> {
    let repo = Repository::open(repo_path)?;
    let mut revwalk = repo.revwalk()?;
    revwalk.set_sorting(git2::Sort::TIME | git2::Sort::TOPOLOGICAL)?;

    if let Some(branch) = &filter.branch {
        let reference_name = format!("refs/heads/{branch}");
        if let Ok(reference) = repo.find_reference(&reference_name)
            && let Some(oid) = reference.target()
        {
            revwalk.push(oid)?;
        }
    } else {
        revwalk.push_head()?;
    }

    let mut commits = Vec::new();

    for oid_result in revwalk.take(limit) {
        let oid = oid_result?;
        let commit = repo.find_commit(oid)?;

        if let Some(author_filter) = &filter.author {
            let author_text = format!(
                "{} {}",
                commit.author().name().unwrap_or_default(),
                commit.author().email().unwrap_or_default()
            )
            .to_lowercase();
            if !author_text.contains(&author_filter.to_lowercase()) {
                continue;
            }
        }

        if let Some(search) = &filter.search {
            let search_lower = search.to_lowercase();
            let message = commit.message().unwrap_or_default().to_lowercase();
            let summary = commit.summary().unwrap_or_default().to_lowercase();
            if !message.contains(&search_lower) && !summary.contains(&search_lower) {
                continue;
            }
        }

        let timestamp = commit.time().seconds();
        if let Some(since) = filter.since
            && timestamp < since
        {
            continue;
        }

        if let Some(until) = filter.until
            && timestamp > until
        {
            continue;
        }

        let parents = commit
            .parents()
            .map(|p| p.id().to_string())
            .collect::<Vec<_>>();

        let (files_changed, additions, deletions) = if include_stats {
            let tree = commit.tree()?;
            let parent_tree = commit
                .parent(0)
                .ok()
                .map(|parent| parent.tree())
                .transpose()?;
            let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)?;
            let stats = diff.stats()?;
            (
                Some(stats.files_changed()),
                Some(stats.insertions()),
                Some(stats.deletions()),
            )
        } else {
            (None, None, None)
        };

        commits.push(CommitInfo {
            id: oid.to_string(),
            summary: commit.summary().unwrap_or_default().to_string(),
            message: commit.message().unwrap_or_default().to_string(),
            author: commit.author().name().unwrap_or("Unknown").to_string(),
            email: commit.author().email().map(|s| s.to_string()),
            time: commit.time(),
            parents,
            files_changed,
            additions,
            deletions,
        });
    }

    Ok(commits)
}

pub fn latest_commit_for_branch(
    repo_path: &str,
    branch_name: &str,
) -> Result<Option<CommitInfo>, git2::Error> {
    let repo = Repository::open(repo_path)?;
    let oid = resolve_branch_oid(&repo, branch_name)?;
    let Some(oid) = oid else {
        return Ok(None);
    };
    let commit = repo.find_commit(oid)?;
    Ok(Some(commit_info_from_commit(&commit)))
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

fn commit_info_from_commit(commit: &git2::Commit<'_>) -> CommitInfo {
    CommitInfo {
        id: commit.id().to_string(),
        summary: commit.summary().unwrap_or_default().to_string(),
        message: commit.message().unwrap_or_default().to_string(),
        author: commit.author().name().unwrap_or("Unknown").to_string(),
        email: commit.author().email().map(|s| s.to_string()),
        time: commit.time(),
        parents: commit.parents().map(|p| p.id().to_string()).collect(),
        files_changed: None,
        additions: None,
        deletions: None,
    }
}
