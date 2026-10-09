//! Repository cloning with progress reporting.

use git2::build::RepoBuilder;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::config::NetworkOptions;
use crate::error::AppError;
use crate::git::transport::{
    configure_proxy_options, create_remote_callbacks, validate_transport_url,
};

/// Request parameters for cloning a repository.
#[derive(Debug, Clone)]
pub struct CloneRequest {
    pub url: String,
    pub destination: PathBuf,
    pub token: Option<String>,
    pub network: NetworkOptions,
}

/// Progress information during clone operation.
#[derive(Debug, Clone, Default)]
pub struct CloneProgress {
    pub received_objects: usize,
    pub total_objects: usize,
    pub total_deltas: usize,
    pub indexed_deltas: usize,
    pub received_bytes: usize,
}

/// Clones a repository with progress reporting.
///
/// # Arguments
/// * `request` - Clone parameters including URL, destination, and network options
/// * `on_progress` - Callback invoked with progress updates during the clone
///
/// # Errors
/// Returns an error if:
/// - The URL violates network policy (SSH disabled, HTTPS required, etc.)
/// - The clone operation fails
/// - The operation times out
pub fn clone_repository(
    request: CloneRequest,
    mut on_progress: impl FnMut(CloneProgress) + Send + 'static,
) -> Result<(), AppError> {
    // Validate URL against network policy
    validate_transport_url(&request.url, &request.network)?;

    let start = Instant::now();
    let timeout_secs = request.network.network_timeout_secs;
    let timed_out = Arc::new(AtomicBool::new(false));
    let timed_out_clone = Arc::clone(&timed_out);

    // Create callbacks with credential handling
    let mut callbacks = create_remote_callbacks(request.token.clone(), timeout_secs);

    // Custom progress callback with timeout check
    callbacks.transfer_progress(move |stats| {
        on_progress(CloneProgress {
            received_objects: stats.received_objects(),
            total_objects: stats.total_objects(),
            total_deltas: stats.total_deltas(),
            indexed_deltas: stats.indexed_deltas(),
            received_bytes: stats.received_bytes(),
        });

        // Check timeout
        if timeout_secs > 0 && start.elapsed().as_secs() >= timeout_secs {
            timed_out_clone.store(true, Ordering::SeqCst);
            return false;
        }
        true
    });

    // Configure fetch options with proxy
    let mut fetch = git2::FetchOptions::new();
    fetch.proxy_options(configure_proxy_options(&request.network));
    fetch.remote_callbacks(callbacks);

    // Build and execute clone
    let mut builder = RepoBuilder::new();
    builder.fetch_options(fetch);

    builder
        .clone(&request.url, &request.destination)
        .map_err(|err| {
            if timed_out.load(Ordering::SeqCst) {
                AppError::Network("Clone operation timed out".to_string())
            } else {
                AppError::from(err)
            }
        })?;

    Ok(())
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
    fn clone_request_can_be_created() {
        let request = CloneRequest {
            url: "https://github.com/example/repo.git".to_string(),
            destination: PathBuf::from("/tmp/repo"),
            token: Some("test-token".to_string()),
            network: default_network(),
        };

        assert_eq!(request.url, "https://github.com/example/repo.git");
        assert!(request.token.is_some());
    }

    #[test]
    fn clone_progress_default_is_zero() {
        let progress = CloneProgress::default();
        assert_eq!(progress.received_objects, 0);
        assert_eq!(progress.total_objects, 0);
        assert_eq!(progress.received_bytes, 0);
    }
}
