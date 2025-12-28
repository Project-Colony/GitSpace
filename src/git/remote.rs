use std::path::Path;
use std::time::Instant;

use git2::build::CheckoutBuilder;
use git2::{
    AnnotatedCommit, Cred, ErrorCode, FetchOptions, FetchPrune, ProxyOptions, PushOptions,
    RemoteCallbacks, Repository,
};

use crate::config::NetworkOptions;
use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct RemoteInfo {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullOutcome {
    UpToDate,
    FastForward,
}

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

#[allow(dead_code)]
pub fn fetch_remote<P: AsRef<Path>>(
    path: P,
    remote_name: &str,
    network: &NetworkOptions,
    token: Option<String>,
) -> Result<(), AppError> {
    let repo = Repository::open(path)?;
    let mut remote = repo.find_remote(remote_name)?;
    if let Some(url) = remote.url() {
        validate_transport_url(url, network)?;
    }

    let mut callbacks = RemoteCallbacks::new();
    let start = Instant::now();
    let timeout_secs = network.network_timeout_secs;

    callbacks.credentials(move |_url, username_from_url, _allowed| {
        if let Some(token) = token.clone() {
            let username = username_from_url.unwrap_or("git");
            Cred::userpass_plaintext(username, &token)
        } else {
            Cred::default()
        }
    });

    callbacks.transfer_progress(move |_stats| {
        if timeout_secs > 0 && start.elapsed().as_secs() >= timeout_secs {
            return false;
        }
        true
    });

    let mut fetch = FetchOptions::new();
    fetch.remote_callbacks(callbacks);
    fetch.proxy_options(configure_proxy_options(network));

    remote.fetch(&[] as &[&str], Some(&mut fetch), None)?;
    Ok(())
}

#[allow(dead_code)]
pub fn pull_branch<P: AsRef<Path>>(
    path: P,
    remote_name: &str,
    branch: &str,
    network: &NetworkOptions,
    token: Option<String>,
) -> Result<PullOutcome, AppError> {
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

#[allow(dead_code)]
pub fn push_branch<P: AsRef<Path>>(
    path: P,
    remote_name: &str,
    branch: &str,
    network: &NetworkOptions,
    token: Option<String>,
) -> Result<(), AppError> {
    let repo = Repository::open(path)?;
    let mut remote = repo.find_remote(remote_name)?;
    if let Some(url) = remote.pushurl().or_else(|| remote.url()) {
        validate_transport_url(url, network)?;
    }

    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(move |_url, username_from_url, _allowed| {
        if let Some(token) = token.clone() {
            let username = username_from_url.unwrap_or("git");
            Cred::userpass_plaintext(username, &token)
        } else {
            Cred::default()
        }
    });

    callbacks.push_transfer_progress(|_current, _total, _bytes| {});

    let mut push_options = PushOptions::new();
    push_options.remote_callbacks(callbacks);
    push_options.proxy_options(configure_proxy_options(network));

    let refspec = format!("refs/heads/{branch}:refs/heads/{branch}");
    remote.push(&[refspec], Some(&mut push_options))?;
    Ok(())
}

#[allow(dead_code)]
pub fn prune_remotes<P: AsRef<Path>>(
    path: P,
    remote_name: &str,
    network: &NetworkOptions,
    token: Option<String>,
) -> Result<(), AppError> {
    let repo = Repository::open(path)?;
    let mut remote = repo.find_remote(remote_name)?;
    if let Some(url) = remote.url() {
        validate_transport_url(url, network)?;
    }

    let mut callbacks = RemoteCallbacks::new();
    let start = Instant::now();
    let timeout_secs = network.network_timeout_secs;

    callbacks.credentials(move |_url, username_from_url, _allowed| {
        if let Some(token) = token.clone() {
            let username = username_from_url.unwrap_or("git");
            Cred::userpass_plaintext(username, &token)
        } else {
            Cred::default()
        }
    });

    callbacks.transfer_progress(move |_stats| {
        if timeout_secs > 0 && start.elapsed().as_secs() >= timeout_secs {
            return false;
        }
        true
    });

    let mut fetch = FetchOptions::new();
    fetch.remote_callbacks(callbacks);
    fetch.proxy_options(configure_proxy_options(network));
    fetch.prune(FetchPrune::On);

    remote.fetch(&[] as &[&str], Some(&mut fetch), None)?;
    Ok(())
}

#[allow(dead_code)]
fn configure_proxy_options(network: &NetworkOptions) -> ProxyOptions<'_> {
    let mut proxy_options = ProxyOptions::new();

    if !network.https_proxy.is_empty() {
        proxy_options.url(&network.https_proxy);
    } else if !network.http_proxy.is_empty() {
        proxy_options.url(&network.http_proxy);
    }

    proxy_options
}

#[allow(dead_code)]
fn validate_transport_url(url: &str, network: &NetworkOptions) -> Result<(), AppError> {
    let url = url.to_lowercase();

    let is_ssh = url.starts_with("ssh://") || url.contains('@');
    if is_ssh && !network.allow_ssh {
        return Err(AppError::Validation(
            "SSH access is disabled in your network preferences.".to_string(),
        ));
    }

    if url.starts_with("https://") && !network.use_https {
        return Err(AppError::Validation(
            "HTTPS connections are disabled in your network preferences.".to_string(),
        ));
    }

    if url.starts_with("http://") && network.use_https {
        return Err(AppError::Validation(
            "Plain HTTP is blocked. Enable HTTP in settings or use HTTPS.".to_string(),
        ));
    }

    Ok(())
}

#[allow(dead_code)]
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
