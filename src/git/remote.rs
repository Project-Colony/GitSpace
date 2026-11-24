//! Remote operations: fetch, pull, push, and prune.

use std::path::Path;

use git2::build::CheckoutBuilder;
use git2::{AnnotatedCommit, Repository};

use crate::config::NetworkOptions;
use crate::error::AppError;
// Utilise le cache thread-local pour les opérations en lecture seule
use crate::git::repo_cache::with_cached_repo;
use crate::git::transport::{
    configure_fetch_options, configure_push_options, create_push_callbacks,
    create_remote_callbacks, validate_transport_url,
};

/// Information about a configured remote.
#[derive(Debug, Clone)]
pub struct RemoteInfo {
    pub name: String,
    pub url: String,
    /// The separate push URL, when one is configured.
    pub push_url: Option<String>,
}

/// Outcome of a pull operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullOutcome {
    /// The local branch was already up to date.
    UpToDate,
    /// A fast-forward merge was performed.
    FastForward,
}

/// Lists all configured remotes for a repository.
pub fn list_remotes<P: AsRef<Path>>(path: P) -> Result<Vec<RemoteInfo>, git2::Error> {
    // Lecture seule des remotes — utilise le cache thread-local
    with_cached_repo(path, |repo| {
        let mut remotes = Vec::new();

        if let Ok(names) = repo.remotes() {
            for name in names.iter().flatten().flatten() {
                if let Ok(remote) = repo.find_remote(name) {
                    let url = remote.url().unwrap_or("(no url)").to_string();
                    let push_url = remote.pushurl().ok().flatten().map(str::to_string);
                    remotes.push(RemoteInfo {
                        name: name.to_string(),
                        url,
                        push_url,
                    });
                }
            }
        }

        Ok(remotes)
    })
}

/// Fetches updates from a remote.
///
/// # Arguments
/// * `path` - Path to the repository
/// * `remote_name` - Name of the remote (e.g., "origin")
/// * `network` - Network configuration options
/// * `token` - Optional authentication token
pub fn fetch_remote<P: AsRef<Path>>(
    path: P,
    remote_name: &str,
    network: &NetworkOptions,
    token: Option<String>,
) -> Result<(), AppError> {
    let repo = Repository::open(path)?;
    let mut remote = repo.find_remote(remote_name)?;

    // Validate URL against network policy
    let url = remote.url().unwrap_or_default().to_string();
    validate_transport_url(&url, network)?;

    let callbacks = create_remote_callbacks(token, network.network_timeout_secs);
    let mut fetch = configure_fetch_options(network, &url, callbacks);

    remote.fetch(&[] as &[&str], Some(&mut fetch), None)?;
    Ok(())
}

/// Fetches a remote, then fast-forwards the checked-out branch to its upstream.
///
/// # Arguments
/// * `path` - Path to the repository
/// * `remote_name` - Name of the remote (e.g., "origin")
/// * `local_branch` - The branch HEAD points to, and the only ref this moves
/// * `upstream_branch` - The branch on the remote that `local_branch` tracks
/// * `network` - Network configuration options
/// * `token` - Optional authentication token
///
/// # Returns
/// * `PullOutcome::UpToDate` if already current
/// * `PullOutcome::FastForward` if fast-forward was performed
///
/// # Errors
/// Returns an error if HEAD is not `local_branch`, if a non-fast-forward merge is
/// required, or if the checkout would overwrite uncommitted changes.
pub fn pull_branch<P: AsRef<Path>>(
    path: P,
    remote_name: &str,
    local_branch: &str,
    upstream_branch: &str,
    network: &NetworkOptions,
    token: Option<String>,
) -> Result<PullOutcome, AppError> {
    fetch_remote(&path, remote_name, network, token)?;

    let repo = Repository::open(path)?;
    let local_ref_name = format!("refs/heads/{local_branch}");
    if repo.find_reference("HEAD")?.symbolic_target()? != Some(local_ref_name.as_str()) {
        return Err(AppError::Git(format!(
            "Pull only updates the checked-out branch, and {local_branch} is not checked out."
        )));
    }

    let remote_ref_name = format!("refs/remotes/{remote_name}/{upstream_branch}");
    let remote_ref = repo.find_reference(&remote_ref_name)?;
    let annotated = repo.reference_to_annotated_commit(&remote_ref)?;
    let (analysis, _) = repo.merge_analysis(&[&annotated])?;

    if analysis.is_up_to_date() {
        return Ok(PullOutcome::UpToDate);
    }

    if analysis.is_fast_forward() || analysis.is_unborn() {
        fast_forward(&repo, &local_ref_name, &annotated)?;
        return Ok(PullOutcome::FastForward);
    }

    Err(AppError::Git(
        "Non-fast-forward pull required. Please merge or rebase manually.".to_string(),
    ))
}

/// Pushes a local branch to a branch on a remote.
///
/// # Arguments
/// * `path` - Path to the repository
/// * `remote_name` - Name of the remote (e.g., "origin")
/// * `local_branch` - Name of the local branch to push
/// * `upstream_branch` - Name of the branch to update on the remote
/// * `network` - Network configuration options
/// * `token` - Optional authentication token
pub fn push_branch<P: AsRef<Path>>(
    path: P,
    remote_name: &str,
    local_branch: &str,
    upstream_branch: &str,
    network: &NetworkOptions,
    token: Option<String>,
) -> Result<(), AppError> {
    let repo = Repository::open(path)?;
    let mut remote = repo.find_remote(remote_name)?;

    // Validate URL against network policy
    let url = remote
        .pushurl()
        .ok()
        .flatten()
        .or_else(|| remote.url().ok())
        .unwrap_or_default()
        .to_string();
    validate_transport_url(&url, network)?;

    let callbacks = create_push_callbacks(token);
    let mut push_options = configure_push_options(network, &url, callbacks);

    let refspec = format!("refs/heads/{local_branch}:refs/heads/{upstream_branch}");
    remote.push(&[refspec], Some(&mut push_options))?;
    Ok(())
}

/// Moves the checked-out branch `local_ref_name` to `annotated`.
///
/// The checkout is libgit2's safe one: it fails instead of overwriting a modified file, and
/// the ref only moves once the working tree was updated.
fn fast_forward(
    repo: &Repository,
    local_ref_name: &str,
    annotated: &AnnotatedCommit<'_>,
) -> Result<(), AppError> {
    let target = repo.find_object(annotated.id(), None)?;
    repo.checkout_tree(&target, Some(&mut CheckoutBuilder::new()))?;
    repo.reference(local_ref_name, annotated.id(), true, "pull: fast-forward")?;
    Ok(())
}
