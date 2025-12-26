use git2::{Diff, DiffOptions, Oid, Repository};

#[derive(Debug, Clone)]
pub struct BranchComparison {
    pub commit: Option<BranchCommit>,
    pub diff: Option<DiffSummary>,
}

#[derive(Debug, Clone)]
pub struct BranchCommit {
    pub id: String,
    pub summary: String,
    pub author: String,
    pub time: git2::Time,
}

#[derive(Debug, Clone)]
pub struct DiffSummary {
    pub files_changed: usize,
    pub additions: usize,
    pub deletions: usize,
}

pub fn compare_branch_with_head(
    repo_path: &str,
    branch_name: &str,
) -> Result<BranchComparison, git2::Error> {
    let repo = Repository::open(repo_path)?;
    let branch_commit = branch_commit(&repo, branch_name)?;
    let diff = match branch_commit
        .as_ref()
        .and_then(|commit| Oid::from_str(&commit.id).ok())
    {
        Some(oid) => {
            diff_summary_between(&repo, Some(oid), repo.head().ok().and_then(|h| h.target()))?
        }
        None => None,
    };

    Ok(BranchComparison {
        commit: branch_commit,
        diff,
    })
}

fn branch_commit(
    repo: &Repository,
    branch_name: &str,
) -> Result<Option<BranchCommit>, git2::Error> {
    let reference_name = format!("refs/heads/{branch_name}");
    let commit = if let Ok(reference) = repo.find_reference(&reference_name) {
        reference.peel_to_commit()?
    } else {
        match repo.revparse_single(branch_name) {
            Ok(object) => object.peel_to_commit()?,
            Err(_) => return Ok(None),
        }
    };

    Ok(Some(BranchCommit {
        id: commit.id().to_string(),
        summary: commit.summary().unwrap_or_default().to_string(),
        author: commit.author().name().unwrap_or("Unknown").to_string(),
        time: commit.time(),
    }))
}

fn diff_summary_between(
    repo: &Repository,
    from: Option<Oid>,
    to: Option<Oid>,
) -> Result<Option<DiffSummary>, git2::Error> {
    if from.is_none() && to.is_none() {
        return Ok(None);
    }
    let from_tree = match from {
        Some(oid) => Some(repo.find_commit(oid)?.tree()?),
        None => None,
    };
    let to_tree = match to {
        Some(oid) => Some(repo.find_commit(oid)?.tree()?),
        None => None,
    };

    let diff = repo.diff_tree_to_tree(
        from_tree.as_ref(),
        to_tree.as_ref(),
        Some(&mut diff_options()),
    )?;
    Ok(Some(diff_summary(diff)?))
}

fn diff_options() -> DiffOptions {
    let mut options = DiffOptions::new();
    options.include_untracked(true);
    options
}

fn diff_summary(diff: Diff) -> Result<DiffSummary, git2::Error> {
    let stats = diff.stats()?;
    Ok(DiffSummary {
        files_changed: stats.files_changed(),
        additions: stats.insertions(),
        deletions: stats.deletions(),
    })
}
