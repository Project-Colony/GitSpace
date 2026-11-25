//! Common transport utilities for Git operations.
//!
//! This module centralizes proxy configuration, transport validation,
//! and credential callbacks to avoid code duplication across clone.rs and remote.rs.

// Some utilities are designed for future use or as public API surface

use std::path::{Path, PathBuf};
use std::sync::Once;
use std::time::{Duration, Instant};

use git2::{
    Cred, CredentialType, FetchOptions, Progress, ProxyOptions, PushOptions, RemoteCallbacks,
};

use crate::config::NetworkOptions;
use crate::error::AppError;
use crate::i18n::t;

/// Validates that a URL conforms to the network policy (SSH/HTTPS/HTTP settings).
pub fn validate_transport_url(url: &str, network: &NetworkOptions) -> Result<(), AppError> {
    let url_lower = url.to_lowercase();

    if is_ssh_url(&url_lower) && !network.allow_ssh {
        return Err(AppError::Validation(t("network_ssh_blocked").to_string()));
    }

    if url_lower.starts_with("https://") && !network.use_https {
        return Err(AppError::Validation(t("network_https_blocked").to_string()));
    }

    if url_lower.starts_with("http://") && network.use_https {
        return Err(AppError::Validation(t("network_http_blocked").to_string()));
    }

    Ok(())
}

/// Detects URLs that git reaches over SSH: the `ssh://`, `git+ssh://` and `ssh+git://`
/// schemes, and the scp-like `[user@]host:path` form.
fn is_ssh_url(url: &str) -> bool {
    let url = url.to_ascii_lowercase();
    if let Some((scheme, _)) = url.split_once("://") {
        return matches!(scheme, "ssh" | "git+ssh" | "ssh+git");
    }
    // As in git: a colon before any slash makes an scp-like address, otherwise it is a
    // local path. `C:\repo` and `C:/repo` are Windows drive letters.
    let Some(colon) = url.find(':') else {
        return false;
    };
    let is_drive = colon == 1 && url.as_bytes()[0].is_ascii_alphabetic();
    colon > 0 && !is_drive && !url[..colon].contains(['/', '\\'])
}

/// Sends git traffic for `url` through the proxy set in Preferences for its scheme: the HTTPS
/// proxy for `https://` remotes and the HTTP proxy for `http://` remotes, as the API client does.
/// Without one, libgit2 falls back to git's `http.proxy` setting and the proxy environment
/// variables.
pub fn configure_proxy_options(network: &NetworkOptions, url: &str) -> ProxyOptions<'static> {
    let mut proxy_options = ProxyOptions::new();
    match configured_proxy(network, url) {
        Some(proxy) => proxy_options.url(proxy),
        None => proxy_options.auto(),
    };
    proxy_options
}

/// The proxy from Preferences that applies to `url`, if one is set.
fn configured_proxy<'a>(network: &'a NetworkOptions, url: &str) -> Option<&'a str> {
    let url = url.to_ascii_lowercase();
    let proxy = if url.starts_with("https://") {
        network.https_proxy.trim()
    } else if url.starts_with("http://") {
        network.http_proxy.trim()
    } else {
        return None;
    };
    // `ProxyOptions::url` panics on a NUL byte.
    (!proxy.is_empty() && !proxy.contains('\0')).then_some(proxy)
}

/// Cancels a transfer once its progress counters stop moving for the configured time, so a
/// long clone keeps running as long as data arrives.
pub struct StallTimer {
    /// `None` when the timeout is disabled.
    timeout: Option<Duration>,
    last_progress: [usize; 4],
    last_change: Instant,
}

impl StallTimer {
    /// A `timeout_secs` of 0 disables the timer.
    pub fn new(timeout_secs: u64) -> Self {
        Self {
            timeout: (timeout_secs > 0).then(|| Duration::from_secs(timeout_secs)),
            last_progress: [0; 4],
            last_change: Instant::now(),
        }
    }

    /// Records the transfer counters and returns `false` once nothing moved for the timeout.
    pub fn keep_going(&mut self, stats: &Progress<'_>) -> bool {
        let progress = [
            stats.received_bytes(),
            stats.received_objects(),
            stats.indexed_objects(),
            stats.indexed_deltas(),
        ];
        self.check(progress, Instant::now())
    }

    fn check(&mut self, progress: [usize; 4], now: Instant) -> bool {
        if progress != self.last_progress {
            self.last_progress = progress;
            self.last_change = now;
            return true;
        }
        self.timeout
            .is_none_or(|timeout| now.duration_since(self.last_change) < timeout)
    }
}

/// Answers a credential request with the saved token, but only over HTTPS: a remote reached
/// over plain `http://`, SSH or any other transport never receives the token.
fn token_credentials(
    url: &str,
    username_from_url: Option<&str>,
    token: &str,
) -> Result<Cred, git2::Error> {
    let url = url.to_ascii_lowercase();
    if url.starts_with("http://") {
        return Err(git2::Error::from_str(t("token_remote_plain_http")));
    }
    if !url.starts_with("https://") {
        return Err(git2::Error::from_str(t("token_https_remotes_only")));
    }
    Cred::userpass_plaintext(username_from_url.unwrap_or("git"), token)
}

/// The private keys in `~/.ssh` that GitSpace tries after ssh-agent, in order.
const DEFAULT_SSH_KEYS: [&str; 3] = ["id_ed25519", "id_ecdsa", "id_rsa"];

/// The default private keys that exist under `home`.
fn default_ssh_keys(home: Option<&Path>) -> Vec<PathBuf> {
    let Some(ssh_dir) = home.map(|home| home.join(".ssh")) else {
        return Vec::new();
    };
    DEFAULT_SSH_KEYS
        .iter()
        .map(|name| ssh_dir.join(name))
        .filter(|key| key.is_file())
        .collect()
}

/// Answers libgit2's credential requests for one transfer.
///
/// libgit2 asks again after every refused credential, so each method is offered at most once:
/// once all of them were refused the transfer fails with an explanation instead of looping.
struct CredentialPicker {
    token: Option<String>,
    ssh_keys: Vec<PathBuf>,
    sent_username: bool,
    tried_agent: bool,
    /// How many of `ssh_keys` were offered.
    tried_keys: usize,
    sent_token: bool,
    tried_default: bool,
}

impl CredentialPicker {
    fn new(token: Option<String>, ssh_keys: Vec<PathBuf>) -> Self {
        Self {
            token,
            ssh_keys,
            sent_username: false,
            tried_agent: false,
            tried_keys: 0,
            sent_token: false,
            tried_default: false,
        }
    }

    fn next(
        &mut self,
        url: &str,
        username_from_url: Option<&str>,
        allowed: CredentialType,
    ) -> Result<Cred, git2::Error> {
        let username = username_from_url.unwrap_or("git");

        if allowed.contains(CredentialType::USERNAME) && !self.sent_username {
            self.sent_username = true;
            return Cred::username(username);
        }

        if allowed.contains(CredentialType::SSH_KEY) {
            if !self.tried_agent {
                self.tried_agent = true;
                return Cred::ssh_key_from_agent(username);
            }
            // ponytail: keys are offered without a passphrase, and libssh2 aborts the whole
            // sign-in on a protected one; skip encrypted keys if users with several keys hit it.
            if let Some(key) = self.ssh_keys.get(self.tried_keys) {
                self.tried_keys += 1;
                let public_key = key.with_extension("pub");
                let public_key = public_key.is_file().then_some(public_key.as_path());
                return Cred::ssh_key(username, public_key, key, None);
            }
        }

        // The token is an HTTPS credential: it is never offered as an SSH password.
        if allowed.contains(CredentialType::USER_PASS_PLAINTEXT) && !is_ssh_url(url) {
            if let Some(token) = self.token.as_deref().filter(|_| !self.sent_token) {
                self.sent_token = true;
                return token_credentials(url, username_from_url, token);
            }
        }

        if allowed.contains(CredentialType::DEFAULT) && !self.tried_default {
            self.tried_default = true;
            return Cred::default();
        }

        Err(git2::Error::from_str(self.failure_message(url)))
    }

    fn failure_message(&self, url: &str) -> &'static str {
        if is_ssh_url(url) {
            t("auth_ssh_failed")
        } else if self.sent_token {
            t("auth_token_refused")
        } else {
            t("auth_token_needed")
        }
    }
}

/// Creates callbacks that answer credential requests for one transfer.
fn credential_callbacks(token: Option<String>) -> RemoteCallbacks<'static> {
    let mut callbacks = RemoteCallbacks::new();
    let ssh_keys = default_ssh_keys(dirs::home_dir().as_deref());
    let mut picker = CredentialPicker::new(token, ssh_keys);
    callbacks.credentials(move |url, username_from_url, allowed| {
        picker.next(url, username_from_url, allowed)
    });
    callbacks
}

/// Creates remote callbacks with credential handling and a stall timeout.
pub fn create_remote_callbacks(
    token: Option<String>,
    timeout_secs: u64,
) -> RemoteCallbacks<'static> {
    let mut callbacks = credential_callbacks(token);
    let mut stall = StallTimer::new(timeout_secs);
    callbacks.transfer_progress(move |stats| stall.keep_going(&stats));
    callbacks
}

/// Creates remote callbacks for push operations with credential handling.
pub fn create_push_callbacks(token: Option<String>) -> RemoteCallbacks<'static> {
    credential_callbacks(token)
}

/// Cuts a connection that sends nothing for `timeout_secs`. libgit2 calls the progress callback
/// only when data arrives, so [`StallTimer`] alone never sees a connection that hangs. This
/// covers SSH and the HTTPS sockets libgit2 opens itself; WinHTTP on Windows ignores it.
///
/// ponytail: libgit2 keeps the timeout in a process-wide setting, so the value from the first
/// transfer stays until GitSpace restarts.
fn apply_socket_timeout(timeout_secs: u64) {
    static APPLIED: Once = Once::new();
    APPLIED.call_once(|| {
        let millis = timeout_secs.saturating_mul(1000).min(i32::MAX as u64) as i32;
        // SAFETY: libgit2 reads this setting only when a transfer opens its connection, and
        // every transfer passes through this `Once` before it starts, so the write happens
        // before any read.
        if let Err(err) = unsafe { git2::opts::set_server_timeout_in_milliseconds(millis) } {
            tracing::warn!(target: "gitspace::git", error = %err, "could not set the git network timeout");
        }
    });
}

/// Configures FetchOptions for `url` with proxy and callbacks.
pub fn configure_fetch_options<'a>(
    network: &NetworkOptions,
    url: &str,
    callbacks: RemoteCallbacks<'a>,
) -> FetchOptions<'a> {
    apply_socket_timeout(network.network_timeout_secs);
    let mut fetch = FetchOptions::new();
    fetch.proxy_options(configure_proxy_options(network, url));
    fetch.remote_callbacks(callbacks);
    fetch
}

/// Configures PushOptions for `url` with proxy and callbacks.
pub fn configure_push_options<'a>(
    network: &NetworkOptions,
    url: &str,
    callbacks: RemoteCallbacks<'a>,
) -> PushOptions<'a> {
    apply_socket_timeout(network.network_timeout_secs);
    let mut push = PushOptions::new();
    push.proxy_options(configure_proxy_options(network, url));
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
    fn ssh_detection_covers_schemes_and_scp_form() {
        for url in [
            "ssh://git@github.com/repo.git",
            "git+ssh://git@github.com/repo.git",
            "SSH+GIT://github.com/repo.git",
            "git@github.com:user/repo.git",
            "user@host.example.com:path/to/repo",
            "github.com:user/repo.git",
        ] {
            assert!(is_ssh_url(url), "{url}");
        }

        for url in [
            "https://user@github.com/repo.git",
            "http://github.com/repo.git",
            "file:///srv/repo.git",
            "git://github.com/repo.git",
            "user@host",
            "/srv/repo.git",
            "./repo:old",
            "C:\\repos\\project",
            "C:/repos/project",
        ] {
            assert!(!is_ssh_url(url), "{url}");
        }
    }

    #[test]
    fn validate_blocks_scheme_and_userless_ssh_when_disabled() {
        let mut network = default_network();
        network.allow_ssh = false;
        assert!(validate_transport_url("git+ssh://github.com/repo.git", &network).is_err());
        assert!(validate_transport_url("github.com:user/repo.git", &network).is_err());
        assert!(validate_transport_url("C:\\repos\\project", &network).is_ok());
    }

    #[test]
    fn token_is_offered_only_over_https() {
        for url in [
            "ssh://git@github.com/repo.git",
            "git+ssh://github.com/repo.git",
            "github.com:user/repo.git",
            "git://github.com/repo.git",
        ] {
            let err = token_credentials(url, None, "secret").err().expect(url);
            assert_eq!(err.message(), t("token_https_remotes_only"));
            let mut picker = CredentialPicker::new(Some("secret".into()), Vec::new());
            assert!(picker
                .next(url, None, CredentialType::USER_PASS_PLAINTEXT)
                .is_err());
        }
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
        let err = token_credentials("http://git.example.com/repo.git", None, "secret").err();
        assert_eq!(err.unwrap().message(), t("token_remote_plain_http"));
        assert!(token_credentials("HTTP://git.example.com/repo.git", None, "secret").is_err());
        assert!(token_credentials("https://git.example.com/repo.git", None, "secret").is_ok());

        // The picker keeps the refusal.
        let mut picker = CredentialPicker::new(Some("secret".into()), Vec::new());
        assert!(picker
            .next(
                "http://git.example.com/repo.git",
                None,
                CredentialType::USER_PASS_PLAINTEXT
            )
            .is_err());
    }

    // The raw type is i32 on MSVC and u32 elsewhere.
    #[allow(clippy::unnecessary_cast)]
    fn credtype(cred: Result<Cred, git2::Error>) -> CredentialType {
        CredentialType::from_bits_truncate(cred.expect("a credential").credtype() as u32)
    }

    #[test]
    fn ssh_tries_username_agent_then_each_key_once() {
        let keys = vec![PathBuf::from("/k/id_ed25519"), PathBuf::from("/k/id_rsa")];
        let mut picker = CredentialPicker::new(Some("token".into()), keys);
        let url = "ssh://example.com/repo.git";

        assert_eq!(
            credtype(picker.next(url, None, CredentialType::USERNAME)),
            CredentialType::USERNAME
        );
        let ssh = CredentialType::SSH_KEY | CredentialType::USER_PASS_PLAINTEXT;
        for _ in 0..3 {
            // ssh-agent, then the two keys
            assert_eq!(
                credtype(picker.next(url, Some("git"), ssh)),
                CredentialType::SSH_KEY
            );
        }
        // The token is never sent as an SSH password, so every method is used up.
        let err = picker.next(url, Some("git"), ssh).err().expect("a refusal");
        assert_eq!(err.message(), t("auth_ssh_failed"));
        assert!(picker.next(url, Some("git"), ssh).is_err());
    }

    #[test]
    fn https_offers_the_token_once() {
        let mut picker = CredentialPicker::new(Some("token".into()), Vec::new());
        let url = "https://example.com/repo.git";
        let allowed = CredentialType::USER_PASS_PLAINTEXT;

        assert_eq!(
            credtype(picker.next(url, None, allowed)),
            CredentialType::USER_PASS_PLAINTEXT
        );
        let err = picker.next(url, None, allowed).err().expect("a refusal");
        assert_eq!(err.message(), t("auth_token_refused"));
    }

    #[test]
    fn without_a_token_only_default_credentials_are_tried() {
        let mut picker = CredentialPicker::new(None, Vec::new());
        let url = "https://example.com/repo.git";

        assert!(picker
            .next(url, None, CredentialType::USER_PASS_PLAINTEXT)
            .is_err());
        let allowed = CredentialType::USER_PASS_PLAINTEXT | CredentialType::DEFAULT;
        assert_eq!(
            credtype(picker.next(url, None, allowed)),
            CredentialType::DEFAULT
        );
        let err = picker.next(url, None, allowed).err().expect("a refusal");
        assert_eq!(err.message(), t("auth_token_needed"));
    }

    #[test]
    fn default_ssh_keys_lists_existing_keys_in_order() {
        let home = tempfile::tempdir().unwrap();
        let ssh = home.path().join(".ssh");
        std::fs::create_dir(&ssh).unwrap();
        std::fs::write(ssh.join("id_rsa"), "").unwrap();
        std::fs::write(ssh.join("id_ed25519"), "").unwrap();

        assert_eq!(
            default_ssh_keys(Some(home.path())),
            vec![ssh.join("id_ed25519"), ssh.join("id_rsa")]
        );
        assert!(default_ssh_keys(None).is_empty());
    }

    #[test]
    fn proxy_follows_the_remote_scheme() {
        let mut network = default_network();
        network.http_proxy = "http://proxy:8080".into();
        network.https_proxy = " https://proxy:8443 ".into();

        assert_eq!(
            configured_proxy(&network, "https://github.com/a/b.git"),
            Some("https://proxy:8443")
        );
        assert_eq!(
            configured_proxy(&network, "HTTP://example.com/a/b.git"),
            Some("http://proxy:8080")
        );
        assert_eq!(configured_proxy(&network, "git@github.com:a/b.git"), None);

        network.https_proxy = "bad\0proxy".into();
        assert_eq!(
            configured_proxy(&network, "https://github.com/a/b.git"),
            None
        );
        assert_eq!(
            configured_proxy(&default_network(), "https://github.com/a/b.git"),
            None
        );
    }

    #[test]
    fn stall_timer_trips_only_when_progress_stops() {
        let start = Instant::now();
        let mut timer = StallTimer::new(30);
        timer.last_change = start;

        // A transfer that keeps moving runs well past the timeout.
        for step in 1..=10 {
            let now = start + Duration::from_secs(step * 20);
            assert!(timer.check([step as usize, 0, 0, 0], now));
        }
        let last = start + Duration::from_secs(200);
        assert!(timer.check([10, 0, 0, 0], last + Duration::from_secs(29)));
        assert!(!timer.check([10, 0, 0, 0], last + Duration::from_secs(30)));
        // Progress resets the timer.
        assert!(timer.check([10, 1, 0, 0], last + Duration::from_secs(31)));

        let mut disabled = StallTimer::new(0);
        assert!(disabled.check([0; 4], Instant::now() + Duration::from_secs(86_400)));
    }
}
