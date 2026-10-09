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

    // Detect SSH URLs properly:
    // 1. ssh:// protocol prefix
    // 2. SCP-style: user@host:path (no :// but has @ followed by :)
    let is_ssh = url_lower.starts_with("ssh://") || is_scp_style_url(&url_lower);
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

/// Detects SCP-style SSH URLs like `git@github.com:user/repo.git`.
/// These have format: `[user@]host:path` with no protocol prefix.
fn is_scp_style_url(url: &str) -> bool {
    // Not a protocol URL (no ://)
    if url.contains("://") {
        return false;
    }

    // Must have @ followed by host:path pattern
    if let Some(at_pos) = url.find('@') {
        // Check there's a : after the @ (for the host:path separator)
        let after_at = &url[at_pos + 1..];
        // The colon must be present and not at the very beginning (valid host required)
        if let Some(colon_pos) = after_at.find(':') {
            // Ensure there's actually a host between @ and :
            return colon_pos > 0;
        }
    }

    false
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

/// Answers a credential request with the saved token, but only over HTTPS (or SSH):
/// a remote reached over plain `http://` never receives the token.
fn token_credentials(
    url: &str,
    username_from_url: Option<&str>,
    token: Option<&str>,
) -> Result<Cred, git2::Error> {
    let Some(token) = token else {
        return Cred::default();
    };
    if url.to_ascii_lowercase().starts_with("http://") {
        return Err(git2::Error::from_str(
            "This remote uses plain HTTP: GitSpace never sends a token over it. Use an https:// remote URL.",
        ));
    }
    Cred::userpass_plaintext(username_from_url.unwrap_or("git"), token)
}

/// Creates remote callbacks with credential handling and optional timeout.
pub fn create_remote_callbacks(
    token: Option<String>,
    timeout_secs: u64,
) -> RemoteCallbacks<'static> {
    let mut callbacks = RemoteCallbacks::new();
    let start = Instant::now();

    callbacks.credentials(move |url, username_from_url, _allowed| {
        token_credentials(url, username_from_url, token.as_deref())
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

    callbacks.credentials(move |url, username_from_url, _allowed| {
        token_credentials(url, username_from_url, token.as_deref())
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

    #[test]
    fn scp_style_detection_works() {
        // Valid SCP-style URLs
        assert!(is_scp_style_url("git@github.com:user/repo.git"));
        assert!(is_scp_style_url("user@host.example.com:path/to/repo"));

        // Not SCP-style (has protocol prefix)
        assert!(!is_scp_style_url("https://user@github.com/repo.git"));
        assert!(!is_scp_style_url("ssh://git@github.com/repo.git"));

        // Not SCP-style (no colon after @)
        assert!(!is_scp_style_url("user@host"));

        // Not SCP-style (no @ at all)
        assert!(!is_scp_style_url("github.com:user/repo.git"));
    }

    #[test]
    fn validate_https_with_embedded_credentials_not_detected_as_ssh() {
        let mut network = default_network();
        network.allow_ssh = false;

        // HTTPS URL with embedded username should NOT be detected as SSH
        // and should be allowed when SSH is disabled
        assert!(validate_transport_url("https://user@github.com/repo.git", &network).is_ok());
        assert!(validate_transport_url("https://token@github.com/repo.git", &network).is_ok());
    }

    #[test]
    fn token_is_never_offered_over_plain_http() {
        let token = Some("secret");
        assert!(token_credentials("http://git.example.com/repo.git", None, token).is_err());
        assert!(token_credentials("HTTP://git.example.com/repo.git", None, token).is_err());
        assert!(token_credentials("https://git.example.com/repo.git", None, token).is_ok());
        // Without a token there is nothing to leak, so plain HTTP keeps the default credentials.
        assert!(token_credentials("http://git.example.com/repo.git", None, None).is_ok());
    }
}
