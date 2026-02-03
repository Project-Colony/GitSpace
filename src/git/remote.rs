//! Remote operations: fetch, pull, push, and prune.

// Public API functions designed for future use
#![allow(dead_code)]

use std::path::Path;

use git2::build::CheckoutBuilder;
use git2::{AnnotatedCommit, ErrorCode, FetchOptions, FetchPrune, PushOptions, Repository};

use crate::config::NetworkOptions;
use crate::error::AppError;
use crate::git::transport::{
    configure_proxy_options, create_push_callbacks, create_remote_callbacks, validate_transport_url,
};

/// Information about a configured remote.
#[derive(Debug, Clone)]
pub struct RemoteInfo {
    pub name: String,
    pub url: String,
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
    let repo = Repository::open(path)?;
    let mut remotes = Vec::new();

    if let Ok(names) = repo.remotes() {
        for name in names.iter().flatten() {
            if let Ok(remote) = repo.find_remote(name) {
                let url = remote.url().unwrap_or("(no url)").to_string();
                remotes.push(RemoteInfo {
                    name: name.to_string(),
                    url,
                });
            }
        }
    }

    Ok(remotes)
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
    if let Some(url) = remote.url() {
        validate_transport_url(url, network)?;
    }

    // Create fetch options with callbacks
    let callbacks = create_remote_callbacks(token, network.network_timeout_secs);
    let mut fetch = FetchOptions::new();
    fetch.remote_callbacks(callbacks);
    fetch.proxy_options(configure_proxy_options(network));

    remote.fetch(&[] as &[&str], Some(&mut fetch), None)?;
    Ok(())
}

/// Pulls updates from a remote and fast-forwards the local branch.
///
/// # Arguments
/// * `path` - Path to the repository
/// * `remote_name` - Name of the remote (e.g., "origin")
/// * `branch` - Name of the branch to pull
/// * `network` - Network configuration options
/// * `token` - Optional authentication token
///
/// # Returns
/// * `PullOutcome::UpToDate` if already current
/// * `PullOutcome::FastForward` if fast-forward was performed
///
/// # Errors
/// Returns an error if a non-fast-forward merge is required.
pub fn pull_branch<P: AsRef<Path>>(
    path: P,
    remote_name: &str,
    branch: &str,
    network: &NetworkOptions,
    token: Option<String>,
) -> Result<PullOutcome, AppError> {
    // First fetch the remote
    fetch_remote(&path, remote_name, network, token)?;

    let repo = Repository::open(path)?;
    let remote_ref_name = format!("refs/remotes/{remote_name}/{branch}");
    let remote_ref = repo.find_reference(&remote_ref_name)?;
    let annotated = repo.reference_to_annotated_commit(&remote_ref)?;
    let (analysis, _) = repo.merge_analysis(&[&annotated])?;

    if analysis.is_up_to_date() {
        return Ok(PullOutcome::UpToDate);
    }

    if analysis.is_fast_forward() {
        let local_ref_name = format!("refs/heads/{branch}");
        fast_forward(&repo, &local_ref_name, &annotated)?;
        return Ok(PullOutcome::FastForward);
    }

    Err(AppError::Git(
        "Non-fast-forward pull required. Please merge or rebase manually.".to_string(),
    ))
}

/// Pushes a branch to a remote.
///
/// # Arguments
/// * `path` - Path to the repository
/// * `remote_name` - Name of the remote (e.g., "origin")
/// * `branch` - Name of the branch to push
/// * `network` - Network configuration options
/// * `token` - Optional authentication token
pub fn push_branch<P: AsRef<Path>>(
    path: P,
    remote_name: &str,
    branch: &str,
    network: &NetworkOptions,
    token: Option<String>,
) -> Result<(), AppError> {
    let repo = Repository::open(path)?;
    let mut remote = repo.find_remote(remote_name)?;

    // Validate URL against network policy
    if let Some(url) = remote.pushurl().or_else(|| remote.url()) {
        validate_transport_url(url, network)?;
    }

    // Create push options with callbacks
    let callbacks = create_push_callbacks(token);
    let mut push_options = PushOptions::new();
    push_options.remote_callbacks(callbacks);
    push_options.proxy_options(configure_proxy_options(network));

    let refspec = format!("refs/heads/{branch}:refs/heads/{branch}");
    remote.push(&[refspec], Some(&mut push_options))?;
    Ok(())
}

/// Prunes stale remote-tracking references.
///
/// # Arguments
/// * `path` - Path to the repository
/// * `remote_name` - Name of the remote (e.g., "origin")
/// * `network` - Network configuration options
/// * `token` - Optional authentication token
pub fn prune_remotes<P: AsRef<Path>>(
    path: P,
    remote_name: &str,
    network: &NetworkOptions,
    token: Option<String>,
) -> Result<(), AppError> {
    let repo = Repository::open(path)?;
    let mut remote = repo.find_remote(remote_name)?;

    // Validate URL against network policy
    if let Some(url) = remote.url() {
        validate_transport_url(url, network)?;
    }

    // Create fetch options with callbacks and prune enabled
    let callbacks = create_remote_callbacks(token, network.network_timeout_secs);
    let mut fetch = FetchOptions::new();
    fetch.remote_callbacks(callbacks);
    fetch.proxy_options(configure_proxy_options(network));
    fetch.prune(FetchPrune::On);

    remote.fetch(&[] as &[&str], Some(&mut fetch), None)?;
    Ok(())
}

/// Performs a fast-forward merge to update a local ref.
fn fast_forward(
    repo: &Repository,
    local_ref_name: &str,
    annotated: &AnnotatedCommit<'_>,
) -> Result<(), AppError> {
    let target = annotated.id();
    let mut local_ref = match repo.find_reference(local_ref_name) {
        Ok(reference) => reference,
        Err(err) => {
            if err.code() == ErrorCode::NotFound {
                repo.reference(local_ref_name, target, true, "create branch")?
            } else {
                return Err(AppError::from(err));
            }
        }
    };

    local_ref.set_target(target, "fast-forward")?;
    repo.set_head(local_ref_name)?;
    let mut checkout = CheckoutBuilder::new();
    repo.checkout_head(Some(checkout.force()))?;
    Ok(())
}
