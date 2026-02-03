//! Common transport utilities for Git operations.
//!
//! This module centralizes proxy configuration, transport validation,
//! and credential callbacks to avoid code duplication across clone.rs and remote.rs.

// Some utilities are designed for future use or as public API surface
#![allow(dead_code)]

use std::time::Instant;

use git2::{Cred, FetchOptions, ProxyOptions, PushOptions, RemoteCallbacks};

use crate::config::NetworkOptions;
use crate::error::AppError;

/// Validates that a URL conforms to the network policy (SSH/HTTPS/HTTP settings).
pub fn validate_transport_url(url: &str, network: &NetworkOptions) -> Result<(), AppError> {
    let url_lower = url.to_lowercase();

    let is_ssh = url_lower.starts_with("ssh://") || url_lower.contains('@');
    if is_ssh && !network.allow_ssh {
        return Err(AppError::Validation(
            "SSH access is disabled in your network preferences.".to_string(),
        ));
    }

    if url_lower.starts_with("https://") && !network.use_https {
        return Err(AppError::Validation(
            "HTTPS connections are disabled in your network preferences.".to_string(),
        ));
    }

    if url_lower.starts_with("http://") && network.use_https {
        return Err(AppError::Validation(
            "Plain HTTP is blocked. Enable HTTP in settings or use HTTPS.".to_string(),
        ));
    }

    Ok(())
}

/// Ensures a URL conforms to HTTPS policy for API requests.
pub fn enforce_https_policy(url: &str, network: &NetworkOptions) -> Result<(), AppError> {
    if url.starts_with("https://") && !network.use_https {
        return Err(AppError::Validation(
            "HTTPS endpoints are disabled in your network settings.".to_string(),
        ));
    }

    if url.starts_with("http://") && network.use_https {
        return Err(AppError::Validation(
            "HTTP requests are blocked. Enable HTTP in network settings or use HTTPS.".to_string(),
        ));
    }

    Ok(())
}

/// Configures proxy options from network settings.
pub fn configure_proxy_options(network: &NetworkOptions) -> ProxyOptions<'static> {
    let mut proxy_options = ProxyOptions::new();

    if !network.https_proxy.is_empty() {
        // ProxyOptions::url takes a &str but we need 'static lifetime
        // We use auto detection when both are set, preferring HTTPS
        proxy_options.auto();
    } else if !network.http_proxy.is_empty() {
        proxy_options.auto();
    }

    proxy_options
}

/// Creates remote callbacks with credential handling and optional timeout.
pub fn create_remote_callbacks(
    token: Option<String>,
    timeout_secs: u64,
) -> RemoteCallbacks<'static> {
    let mut callbacks = RemoteCallbacks::new();
    let start = Instant::now();

    callbacks.credentials(move |_url, username_from_url, _allowed| {
        if let Some(ref token) = token {
            let username = username_from_url.unwrap_or("git");
            Cred::userpass_plaintext(username, token)
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

    callbacks
}

/// Creates remote callbacks for push operations with credential handling.
pub fn create_push_callbacks(token: Option<String>) -> RemoteCallbacks<'static> {
    let mut callbacks = RemoteCallbacks::new();

    callbacks.credentials(move |_url, username_from_url, _allowed| {
        if let Some(ref token) = token {
            let username = username_from_url.unwrap_or("git");
            Cred::userpass_plaintext(username, token)
        } else {
            Cred::default()
        }
    });

    callbacks.push_transfer_progress(|_current, _total, _bytes| {});

    callbacks
}

/// Configures FetchOptions with proxy and callbacks.
pub fn configure_fetch_options<'a>(
    network: &NetworkOptions,
    callbacks: RemoteCallbacks<'a>,
) -> FetchOptions<'a> {
    let mut fetch = FetchOptions::new();
    fetch.proxy_options(configure_proxy_options(network));
    fetch.remote_callbacks(callbacks);
    fetch
}

/// Configures PushOptions with proxy and callbacks.
pub fn configure_push_options<'a>(
    network: &NetworkOptions,
    callbacks: RemoteCallbacks<'a>,
) -> PushOptions<'a> {
    let mut push = PushOptions::new();
    push.proxy_options(configure_proxy_options(network));
    push.remote_callbacks(callbacks);
    push
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_network() -> NetworkOptions {
        NetworkOptions {
            network_timeout_secs: 30,
            http_proxy: String::new(),
            https_proxy: String::new(),
            use_https: true,
            allow_ssh: true,
        }
    }

    #[test]
    fn validate_allows_https_when_enabled() {
        let network = default_network();
        assert!(validate_transport_url("https://github.com/repo.git", &network).is_ok());
    }

    #[test]
    fn validate_blocks_http_when_https_required() {
        let network = default_network();
        assert!(validate_transport_url("http://github.com/repo.git", &network).is_err());
    }

    #[test]
    fn validate_allows_ssh_when_enabled() {
        let network = default_network();
        assert!(validate_transport_url("git@github.com:user/repo.git", &network).is_ok());
        assert!(validate_transport_url("ssh://git@github.com/repo.git", &network).is_ok());
    }

    #[test]
    fn validate_blocks_ssh_when_disabled() {
        let mut network = default_network();
        network.allow_ssh = false;
        assert!(validate_transport_url("git@github.com:user/repo.git", &network).is_err());
    }

    #[test]
    fn enforce_https_blocks_http() {
        let network = default_network();
        assert!(enforce_https_policy("http://api.github.com", &network).is_err());
    }

    #[test]
    fn enforce_https_allows_https() {
        let network = default_network();
        assert!(enforce_https_policy("https://api.github.com", &network).is_ok());
    }
}
