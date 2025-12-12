use git2::{Cred, FetchOptions, ProxyOptions, RemoteCallbacks, build::RepoBuilder};
use std::path::PathBuf;
use std::time::Instant;

use crate::config::NetworkOptions;
use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct CloneRequest {
    pub url: String,
    pub destination: PathBuf,
    pub token: Option<String>,
    pub network: NetworkOptions,
}

#[derive(Debug, Clone, Default)]
pub struct CloneProgress {
    pub received_objects: usize,
    pub total_objects: usize,
    pub total_deltas: usize,
    pub indexed_deltas: usize,
    pub received_bytes: usize,
}

pub fn clone_repository(
    request: CloneRequest,
    mut on_progress: impl FnMut(CloneProgress) + Send + 'static,
) -> Result<(), AppError> {
    validate_transport(&request, &request.network)?;
    let start = Instant::now();
    let timeout_secs = request.network.network_timeout_secs;
    let mut callbacks = RemoteCallbacks::new();
    let token = request.token.clone();

    callbacks.credentials(move |_url, username_from_url, _allowed| {
        if let Some(token) = token.clone() {
            let username = username_from_url.unwrap_or("git");
            Cred::userpass_plaintext(username, &token)
        } else {
            Cred::default()
        }
    });

    callbacks.transfer_progress(move |stats| {
        on_progress(CloneProgress {
            received_objects: stats.received_objects(),
            total_objects: stats.total_objects(),
            total_deltas: stats.total_deltas(),
            indexed_deltas: stats.indexed_deltas(),
            received_bytes: stats.received_bytes(),
        });
        if timeout_secs > 0 && start.elapsed().as_secs() >= timeout_secs {
            return false;
        }
        true
    });

    let mut fetch = FetchOptions::new();
    configure_proxies(&mut fetch, &request.network)?;
    fetch.remote_callbacks(callbacks);

    let mut builder = RepoBuilder::new();
    builder.fetch_options(fetch);

    builder
        .clone(&request.url, &request.destination)
        .map_err(AppError::from)?;

    Ok(())
}

fn configure_proxies(
    fetch: &mut FetchOptions<'_>,
    network: &NetworkOptions,
) -> Result<(), AppError> {
    let mut proxy_options = ProxyOptions::new();

    if !network.https_proxy.is_empty() {
        proxy_options.url(&network.https_proxy);
    } else if !network.http_proxy.is_empty() {
        proxy_options.url(&network.http_proxy);
    }

    fetch.proxy_options(proxy_options);
    Ok(())
}

fn validate_transport(request: &CloneRequest, network: &NetworkOptions) -> Result<(), AppError> {
    let url = request.url.to_lowercase();

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
