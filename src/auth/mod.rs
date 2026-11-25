pub mod oauth;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose, Engine as _};
use chacha20poly1305::aead::{Aead, Generate, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use keyring::Entry;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, USER_AGENT};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};
use url::Url;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::config::write_private;

pub use oauth::{DeviceFlow, OAuthFlow, OAuthProvider, OAuthResult, OAuthToken};

const SERVICE_NAME: &str = "gitspace";
const TOKEN_FILE_NAME: &str = "tokens.enc";
const HOST_FILE_NAME: &str = "token-hosts.json";
const TOKEN_SALT_FILE: &str = "token-salt.bin";
const TOKEN_LOCAL_KEY_FILE: &str = "token-local-key.bin";
const TOKEN_KEYRING_ENTRY: &str = "token-key";
const MASTER_PASSWORD_ENV: &str = "GITSPACE_TOKEN_MASTER_PASSWORD";

/// Minimum token length for validation.
const MIN_TOKEN_LENGTH: usize = 8;
/// Maximum token length for validation.
const MAX_TOKEN_LENGTH: usize = 512;
/// HTTP client timeout for token validation.
const VALIDATION_TIMEOUT: Duration = Duration::from_secs(10);

/// Reusable HTTP client for token validation.
static HTTP_CLIENT: OnceLock<Client> = OnceLock::new();

/// Gets or creates the shared HTTP client for token validation.
fn get_http_client() -> &'static Client {
    HTTP_CLIENT.get_or_init(|| {
        Client::builder()
            .user_agent("gitspace")
            .timeout(VALIDATION_TIMEOUT)
            .build()
            .unwrap_or_else(|_| Client::new())
    })
}

/// Wrapper for encryption key that automatically zeroes memory on drop.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
struct SecureKey([u8; 32]);

#[derive(Debug, Clone)]
pub struct AuthManager {
    storage: TokenStorage,
}

impl Default for AuthManager {
    fn default() -> Self {
        Self::with_encrypted_fallback(false)
    }
}

impl AuthManager {
    pub fn with_encrypted_fallback(allow_encrypted_fallback: bool) -> Self {
        Self {
            storage: TokenStorage::new(allow_encrypted_fallback),
        }
    }

    /// Returns the token saved for `host`. An expired OAuth token with a refresh token is
    /// refreshed first, which blocks on one HTTP request; if that fails, the saved access
    /// token is returned and the host will reject it.
    pub fn resolve_for_host(&self, host: &str) -> Option<String> {
        let key = storage_key(host);
        if self
            .get_oauth_token(&key)
            .is_some_and(|oauth| oauth.is_expired() && oauth.refresh_token.is_some())
        {
            // One refresh at a time: a provider that rotates refresh tokens refuses the second
            // use of the same one. The token is read again once the lock is held.
            static REFRESH: std::sync::Mutex<()> = std::sync::Mutex::new(());
            let _guard = REFRESH.lock().unwrap_or_else(|err| err.into_inner());
            if let Some(oauth) = self.get_oauth_token(&key) {
                if oauth.is_expired() && oauth.refresh_token.is_some() {
                    if let Some(token) = self.refresh_oauth_token(&oauth) {
                        return Some(token);
                    }
                }
            }
        }
        self.storage.get_token(&key).ok().flatten()
    }

    /// Refreshes an expired OAuth token, saves the new one and returns its access token.
    fn refresh_oauth_token(&self, token: &OAuthToken) -> Option<String> {
        let refreshed = token
            .refresh()
            .inspect_err(|err| {
                warn!(target: "gitspace::auth", host = %token.host, error = %err, "failed to refresh the OAuth token");
            })
            .ok()?;
        if let Err(err) = self.store_oauth_token(&refreshed) {
            warn!(target: "gitspace::auth", host = %token.host, error = %err, "failed to save the refreshed OAuth token");
        }
        Some(refreshed.access_token)
    }

    pub fn resolve_for_url(&self, url: &str) -> Option<String> {
        let host = extract_host(url)?;
        self.resolve_for_host(&host)
            .or_else(|| match host.as_str() {
                "github.com" => self.resolve_for_host("api.github.com"),
                _ => None,
            })
    }

    /// Removes the token saved for `host`, normalized like a lookup. An entry that an older
    /// release saved under the host as typed is removed too.
    pub fn clear_token(&self, host: &str) -> Result<(), String> {
        let key = storage_key(host);
        let result = self.storage.clear_token(&key);
        if key != host {
            self.storage.clear_token(host)?;
        }
        result
    }

    pub fn known_hosts(&self) -> Vec<String> {
        self.storage.known_hosts()
    }

    pub async fn validate_token(&self, host: &str, token: &str) -> Result<(), String> {
        let trimmed = token.trim();

        // Basic format validation
        if trimmed.is_empty() {
            return Err("Token cannot be empty".to_string());
        }

        if trimmed.len() < MIN_TOKEN_LENGTH {
            return Err(format!(
                "Token is too short (minimum {} characters)",
                MIN_TOKEN_LENGTH
            ));
        }

        if trimmed.len() > MAX_TOKEN_LENGTH {
            return Err(format!(
                "Token is too long (maximum {} characters)",
                MAX_TOKEN_LENGTH
            ));
        }

        // Check for invalid characters (tokens should be printable ASCII)
        if !trimmed.chars().all(|c| c.is_ascii_graphic()) {
            return Err("Token contains invalid characters".to_string());
        }

        let origin = normalize_host(host)?;
        let client = get_http_client();

        match token_check(&origin) {
            Some(TokenCheck::GitHub(url)) => validate_github(client, &url, trimmed).await,
            Some(TokenCheck::GitLab(url)) => validate_gitlab(client, &url, trimmed).await,
            None => Ok(()),
        }
    }

    pub async fn validate_and_store(&self, host: &str, token: &str) -> Result<(), String> {
        self.validate_token(host, token).await?;
        let key = storage_key(host);
        // Saved under the key `resolve_for_url` looks up, not the host as typed.
        self.storage.set_token(&key, token.trim())?;
        // The token typed by the user replaces an OAuth sign-in, whose refresh would
        // otherwise overwrite it.
        self.storage.clear_token(&format!("oauth:{key}"))
    }

    pub fn set_encrypted_fallback(&mut self, allowed: bool) {
        self.storage.set_allow_encrypted_fallback(allowed);
    }

    /// Stores an OAuth token for a host.
    pub fn store_oauth_token(&self, token: &OAuthToken) -> Result<(), String> {
        // Store the access token using the existing mechanism
        let host = storage_key(&token.host);
        self.storage.set_token(&host, &token.access_token)?;

        // Store the full OAuth token (with refresh token) as JSON in a separate entry
        let oauth_key = format!("oauth:{host}");
        let oauth_json = serde_json::to_string(token)
            .map_err(|e| format!("Failed to serialize OAuth token: {}", e))?;
        self.storage.set_token(&oauth_key, &oauth_json)?;

        info!(
            target: "gitspace::auth",
            host = %token.host,
            provider = %token.provider,
            has_refresh = token.refresh_token.is_some(),
            "OAuth token stored"
        );

        Ok(())
    }

    /// Retrieves the OAuth token for a host, if one exists.
    pub fn get_oauth_token(&self, host: &str) -> Option<OAuthToken> {
        let oauth_key = format!("oauth:{}", storage_key(host));
        let oauth_json = self.storage.get_token(&oauth_key).ok()??;
        serde_json::from_str(&oauth_json).ok()
    }

    /// Checks if a host has OAuth authentication configured.
    pub fn has_oauth(&self, host: &str) -> bool {
        self.get_oauth_token(host).is_some()
    }

    /// Starts an authorization code flow with PKCE for the given provider (GitLab).
    /// GitHub signs in with [`DeviceFlow`] instead.
    ///
    /// This will open the browser for authentication and block until complete.
    pub fn start_oauth(&self, provider: OAuthProvider) -> OAuthResult {
        match OAuthFlow::new(provider) {
            Ok(flow) => flow.start(),
            Err(e) => OAuthResult::Error(e),
        }
    }

    /// Clears OAuth token for a host.
    pub fn clear_oauth_token(&self, host: &str) -> Result<(), String> {
        let oauth_key = format!("oauth:{}", storage_key(host));
        let oauth_result = self.storage.clear_token(&oauth_key);
        // The access token goes even when the refresh token could not be removed.
        self.clear_token(host)?;
        oauth_result
    }
}

/// Secure token storage with automatic key zeroization.
#[derive(Clone)]
pub struct TokenStorage {
    key: SecureKey,
    /// The local key file, when `key` comes from the keyring: `tokens.enc` may have been
    /// written with the local key while the keyring was locked.
    local_key_path: Option<PathBuf>,
    path: PathBuf,
    host_path: PathBuf,
    allow_encrypted_fallback: bool,
}

impl std::fmt::Debug for TokenStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenStorage")
            .field("path", &self.path)
            .field("host_path", &self.host_path)
            .field("allow_encrypted_fallback", &self.allow_encrypted_fallback)
            .field("key", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct TokenMap {
    tokens: HashMap<String, String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct HostIndex {
    hosts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EncryptedTokenFile {
    nonce: String,
    ciphertext: String,
}

impl TokenStorage {
    pub fn new(allow_encrypted_fallback: bool) -> Self {
        let (key, local_key_path) = match load_or_create_keyring_key() {
            Ok(key) => (key, Some(config_dir().join(TOKEN_LOCAL_KEY_FILE))),
            Err(err) => {
                warn!(target: "gitspace::auth", error = %err, "failed to access keyring encryption key");
                (load_or_create_local_key(), None)
            }
        };
        Self {
            key: SecureKey(key),
            local_key_path,
            path: config_dir().join(TOKEN_FILE_NAME),
            host_path: config_dir().join(HOST_FILE_NAME),
            allow_encrypted_fallback,
        }
    }

    pub fn set_token(&self, host: &str, token: &str) -> Result<(), String> {
        let keyring_result = self.store_in_keyring(host, token);
        if let Err(ref err) = keyring_result {
            warn!(target: "gitspace::auth", error = %err, "failed to store token in native keyring");
        }

        let result = if self.allow_encrypted_fallback {
            self.persist_fallback(host, token).or_else(|err| {
                if keyring_result.is_ok() {
                    // The keyring copy is saved and read first, so the token is not lost.
                    warn!(target: "gitspace::auth", error = %err, "token saved in the native keyring only; the encrypted fallback was not updated");
                    Ok(())
                } else {
                    Err(err)
                }
            })
        } else if keyring_result.is_err() {
            Err("Native keyring unavailable and encrypted storage is disabled".to_string())
        } else {
            Ok(())
        };

        if result.is_ok() {
            if let Err(err) = self.record_host(host) {
                warn!(target: "gitspace::auth", error = %err, "failed to update saved host list");
            }
        }

        result
    }

    pub fn get_token(&self, host: &str) -> Result<Option<String>, String> {
        match self.fetch_from_keyring(host) {
            Ok(Some(token)) => return Ok(Some(token)),
            Ok(None) => {}
            Err(err) => {
                warn!(target: "gitspace::auth", error = %err, host, "failed to read keyring");
                if !self.allow_encrypted_fallback {
                    return Err(err);
                }
            }
        }
        if self.allow_encrypted_fallback {
            let tokens = self.read_fallback()?;
            Ok(tokens.tokens.get(host).cloned())
        } else {
            Ok(None)
        }
    }

    /// Removes the token from the keyring and from `tokens.enc`, then the host from the host
    /// list. The host stays listed when the keyring entry could not be removed, since the token
    /// would still be used.
    pub fn clear_token(&self, host: &str) -> Result<(), String> {
        let keyring_result = self.remove_from_keyring(host);
        if let Err(err) = &keyring_result {
            error!(target: "gitspace::auth", error = %err, host, "failed to clear token from native keyring");
        }
        let file_result = self.scrub_fallback(host);
        keyring_result?;
        file_result?;
        self.remove_host(host).inspect_err(|err| {
            warn!(target: "gitspace::auth", error = %err, "failed to update saved host list");
        })
    }

    /// Removes `host` from `tokens.enc` whether or not the encrypted fallback is on, so a token
    /// saved while it was on does not come back when it is turned on again.
    fn scrub_fallback(&self, host: &str) -> Result<(), String> {
        let mut map = match self.read_fallback_for_update() {
            Ok(map) => map,
            // With the fallback off the file is not in use, so a key mismatch is not an error.
            Err(err) if !self.allow_encrypted_fallback => {
                warn!(target: "gitspace::auth", error = %err, "could not remove the token from the encrypted file");
                return Ok(());
            }
            Err(err) => return Err(err),
        };
        if map.tokens.remove(host).is_some() {
            self.write_fallback(&map)
        } else {
            Ok(())
        }
    }

    pub fn known_hosts(&self) -> Vec<String> {
        let mut hosts = HashSet::new();
        if let Ok(index) = self.read_host_index() {
            for host in index.hosts {
                hosts.insert(host);
            }
        }
        if self.allow_encrypted_fallback {
            if let Ok(map) = self.read_fallback() {
                for host in map.tokens.keys() {
                    hosts.insert(host.clone());
                }
            }
        }
        let mut list: Vec<String> = hosts.into_iter().collect();
        list.sort();
        list
    }

    fn store_in_keyring(&self, host: &str, token: &str) -> Result<(), String> {
        let entry = Entry::new(SERVICE_NAME, host)
            .map_err(|err| format!("Failed to access keyring: {err}"))?;
        entry
            .set_password(token)
            .map_err(|err| format!("Failed to store token in keyring: {err}"))
    }

    fn fetch_from_keyring(&self, host: &str) -> Result<Option<String>, String> {
        let entry = Entry::new(SERVICE_NAME, host)
            .map_err(|err| format!("Failed to access keyring: {err}"))?;
        match entry.get_password() {
            Ok(password) => Ok(Some(password)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(format!("Failed to read keyring: {err}")),
        }
    }

    fn remove_from_keyring(&self, host: &str) -> Result<(), String> {
        let entry = Entry::new(SERVICE_NAME, host)
            .map_err(|err| format!("Failed to access keyring: {err}"))?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            // No keyring service at all: nothing can have been saved there. A locked keyring
            // (NoStorageAccess) is still an error, since the token may be in it.
            Err(keyring::Error::PlatformFailure(err)) => {
                warn!(target: "gitspace::auth", error = %err, host, "native keyring unavailable; nothing to remove there");
                Ok(())
            }
            Err(err) => Err(format!("Failed to remove keyring entry: {err}")),
        }
    }

    fn persist_fallback(&self, host: &str, token: &str) -> Result<(), String> {
        let mut tokens = self.read_fallback_for_update()?;
        tokens.tokens.insert(host.to_string(), token.to_string());
        self.write_fallback(&tokens)
    }

    fn write_fallback(&self, map: &TokenMap) -> Result<(), String> {
        let blob = encrypt_tokens(map, &self.key.0)?;
        let serialized = serde_json::to_string_pretty(&blob)
            .map_err(|err| format!("Failed to serialize credentials: {err}"))?;
        write_private(&self.path, serialized.as_bytes())
            .map_err(|err| format!("Failed to write credentials: {err}"))
    }

    /// Reads the saved tokens before a rewrite. A file that exists but cannot be read or
    /// decrypted (keyring locked, master password changed or unset) is an error, never an
    /// empty map: rewriting it would drop every other saved token.
    fn read_fallback_for_update(&self) -> Result<TokenMap, String> {
        self.read_fallback().map_err(|err| {
            format!(
                "{err}. {} was left untouched so the other saved tokens are kept. Unlock the \
                 system keyring or use the same {MASTER_PASSWORD_ENV} as when the tokens were \
                 saved, then restart GitSpace, or move the file aside to start over",
                self.path.display()
            )
        })
    }

    fn read_fallback(&self) -> Result<TokenMap, String> {
        let data = match fs::read_to_string(&self.path) {
            Ok(data) => data,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(TokenMap::default()),
            Err(err) => return Err(format!("Failed to read credential file: {err}")),
        };
        let blob: EncryptedTokenFile = serde_json::from_str(&data)
            .map_err(|err| format!("Failed to parse credential file: {err}"))?;
        let err = match decrypt_tokens(&blob, &self.key.0) {
            Ok(map) => return Ok(map),
            Err(err) => err,
        };

        // Written with the local key while the keyring was locked: decrypt it with that key
        // and save it again under the keyring key.
        let Some(mut local_key) = self.local_key_path.as_deref().and_then(read_local_key) else {
            return Err(err);
        };
        let result = decrypt_tokens(&blob, &local_key);
        local_key.zeroize();
        let map = result.map_err(|_| err)?;
        match self.write_fallback(&map) {
            Ok(()) => {
                info!(target: "gitspace::auth", "re-encrypted saved tokens with the keyring key")
            }
            Err(err) => {
                warn!(target: "gitspace::auth", error = %err, "failed to re-encrypt saved tokens with the keyring key")
            }
        }
        Ok(map)
    }

    fn record_host(&self, host: &str) -> Result<(), String> {
        let mut index = self.read_host_index()?;
        if !index.hosts.iter().any(|value| value == host) {
            index.hosts.push(host.to_string());
            index.hosts.sort();
            self.write_host_index(&index)?;
        }
        Ok(())
    }

    fn remove_host(&self, host: &str) -> Result<(), String> {
        let mut index = self.read_host_index()?;
        let original_len = index.hosts.len();
        index.hosts.retain(|value| value != host);
        if index.hosts.len() != original_len {
            self.write_host_index(&index)?;
        }
        Ok(())
    }

    fn read_host_index(&self) -> Result<HostIndex, String> {
        let data = match fs::read_to_string(&self.host_path) {
            Ok(data) => data,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(HostIndex::default()),
            Err(err) => return Err(format!("Failed to read host index: {err}")),
        };
        serde_json::from_str(&data).map_err(|err| format!("Failed to parse host index: {err}"))
    }

    fn write_host_index(&self, index: &HostIndex) -> Result<(), String> {
        let serialized = serde_json::to_string_pretty(index)
            .map_err(|err| format!("Failed to serialize host index: {err}"))?;
        write_private(&self.host_path, serialized.as_bytes())
            .map_err(|err| format!("Failed to write host index: {err}"))
    }

    pub fn set_allow_encrypted_fallback(&mut self, allowed: bool) {
        self.allow_encrypted_fallback = allowed;
    }
}

fn encrypt_tokens(map: &TokenMap, key: &[u8; 32]) -> Result<EncryptedTokenFile, String> {
    let cipher = ChaCha20Poly1305::new(key.into());
    // Drawn from the operating system's RNG for every write, so a key never reuses a nonce.
    let nonce = Nonce::generate();

    let serialized =
        serde_json::to_string(map).map_err(|err| format!("Failed to serialize tokens: {err}"))?;
    let encrypted = cipher
        .encrypt(&nonce, serialized.as_bytes())
        .map_err(|err| format!("Failed to encrypt tokens: {err}"))?;

    Ok(EncryptedTokenFile {
        nonce: general_purpose::STANDARD.encode(nonce),
        ciphertext: general_purpose::STANDARD.encode(encrypted),
    })
}

fn decrypt_tokens(blob: &EncryptedTokenFile, key: &[u8; 32]) -> Result<TokenMap, String> {
    let cipher = ChaCha20Poly1305::new(key.into());
    let nonce_bytes = general_purpose::STANDARD
        .decode(&blob.nonce)
        .map_err(|err| format!("Failed to decode nonce: {err}"))?;
    let nonce = Nonce::try_from(nonce_bytes.as_slice())
        .map_err(|_| format!("Invalid nonce length: {} bytes", nonce_bytes.len()))?;
    let cipher_bytes = general_purpose::STANDARD
        .decode(&blob.ciphertext)
        .map_err(|err| format!("Failed to decode ciphertext: {err}"))?;

    let plaintext = cipher
        .decrypt(&nonce, cipher_bytes.as_ref())
        .map_err(|err| format!("Failed to decrypt credentials: {err}"))?;
    let content = String::from_utf8(plaintext)
        .map_err(|err| format!("Invalid credential encoding: {err}"))?;
    serde_json::from_str(&content)
        .map_err(|err| format!("Failed to parse decrypted credentials: {err}"))
}

/// The settings folder that holds the token files.
fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(SERVICE_NAME)
}

/// Loads or creates the local encryption key, used when the keyring cannot hold the key.
///
/// The key is 32 random bytes in `token-local-key.bin`, owner-only on Unix. When
/// `GITSPACE_TOKEN_MASTER_PASSWORD` is set, a key derived from it is XORed in for defense in
/// depth. The master password therefore only applies while the keyring is unavailable; a key
/// kept in the keyring is used as it is.
fn load_or_create_local_key() -> [u8; 32] {
    let dir = config_dir();
    let mut key = load_or_create_secret(&dir.join(TOKEN_LOCAL_KEY_FILE));
    mix_master_password(&mut key, &dir);
    key
}

/// Reads the local key without creating it, mixed with the master password like
/// [`load_or_create_local_key`].
fn read_local_key(path: &Path) -> Option<[u8; 32]> {
    let mut key: [u8; 32] = fs::read(path).ok()?.try_into().ok()?;
    mix_master_password(&mut key, path.parent().unwrap_or(Path::new(".")));
    Some(key)
}

/// XORs the key derived from `GITSPACE_TOKEN_MASTER_PASSWORD`, if set, into `key`.
fn mix_master_password(key: &mut [u8; 32], dir: &Path) {
    let Ok(master_password) = std::env::var(MASTER_PASSWORD_ENV) else {
        return;
    };
    let salt: [u8; 16] = load_or_create_secret(&dir.join(TOKEN_SALT_FILE));
    let mut derived = [0u8; 32];
    if derive_master_key(master_password.as_bytes(), &salt, &mut derived) {
        for (a, b) in key.iter_mut().zip(derived.iter()) {
            *a ^= b;
        }
    }
    derived.zeroize();
}

/// Derives the key mixed into the local key when a master password is set, into `out`, which
/// the caller zeroes. Changing the algorithm or parameters makes every token saved under a
/// master password unreadable.
fn derive_master_key(password: &[u8], salt: &[u8], out: &mut [u8; 32]) -> bool {
    // OWASP recommended: 64 MB memory, 3 iterations, 1 thread
    // m_cost is in KiB, so 65536 = 64 MB
    let params = Params::new(65536, 3, 1, None).unwrap_or(Params::DEFAULT);
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    argon2.hash_password_into(password, salt, out).is_ok()
}

fn load_or_create_keyring_key() -> Result<[u8; 32], String> {
    let entry = Entry::new(SERVICE_NAME, TOKEN_KEYRING_ENTRY)
        .map_err(|err| format!("Failed to access keyring: {err}"))?;
    match entry.get_password() {
        Ok(password) => {
            let decoded = general_purpose::STANDARD
                .decode(password.trim())
                .map_err(|err| format!("Failed to decode keyring token key: {err}"))?;
            if decoded.len() != 32 {
                return Err("Keyring token key is invalid".to_string());
            }
            let mut key = [0u8; 32];
            key.copy_from_slice(&decoded);
            Ok(key)
        }
        Err(keyring::Error::NoEntry) => {
            let key: [u8; 32] = Generate::generate();
            let encoded = general_purpose::STANDARD.encode(key);
            entry
                .set_password(&encoded)
                .map_err(|err| format!("Failed to store token key in keyring: {err}"))?;
            Ok(key)
        }
        Err(err) => Err(format!("Failed to read keyring token key: {err}")),
    }
}

/// Reads the `N`-byte secret in `path`, or creates the file, owner-only on Unix, when it does
/// not exist.
///
/// A file that exists but cannot be read or has the wrong size is never replaced: the tokens
/// encrypted with it would become unrecoverable. A secret for this session only is used
/// instead, so those tokens fail to decrypt and are left untouched.
fn load_or_create_secret<const N: usize>(path: &Path) -> [u8; N] {
    match fs::read(path) {
        Ok(bytes) => match <[u8; N]>::try_from(bytes.as_slice()) {
            Ok(saved) => return saved,
            Err(_) => error!(
                target: "gitspace::auth",
                path = %path.display(),
                size = bytes.len(),
                expected = N,
                "secret file has the wrong size; using a secret for this session only and leaving the file as it is"
            ),
        },
        Err(err) if err.kind() == ErrorKind::NotFound => {
            let secret: [u8; N] = Generate::generate();
            if let Err(err) = write_private(path, &secret) {
                error!(target: "gitspace::auth", error = %err, path = %path.display(), "failed to save a new secret file; it is used for this session only");
            }
            return secret;
        }
        Err(err) => error!(
            target: "gitspace::auth",
            error = %err,
            path = %path.display(),
            "failed to read secret file; using a secret for this session only"
        ),
    }
    Generate::generate()
}

/// Turns a user-entered host into the `https://host[:port]` base URL that tokens are sent to.
///
/// A host without a scheme gets `https://`. Any other scheme, `http://` included, is refused so
/// a token is never sent in clear text.
fn normalize_host(host: &str) -> Result<String, String> {
    let trimmed = host.trim().trim_end_matches('/');
    let with_scheme = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };
    let parsed =
        Url::parse(&with_scheme).map_err(|err| format!("Invalid host \"{trimmed}\": {err}"))?;
    if parsed.scheme() != "https" {
        return Err(
            "The host must use https: GitSpace never sends a token over plain HTTP.".to_string(),
        );
    }
    Ok(parsed.origin().ascii_serialization())
}

/// The API that checks a token for `origin` (from [`normalize_host`]), with its `/user` URL.
#[derive(Debug, PartialEq, Eq)]
enum TokenCheck {
    GitHub(String),
    GitLab(String),
}

/// Picks the API from the exact host name: a substring test would send the token of
/// `github.company.com` to `api.github.com`.
fn token_check(origin: &str) -> Option<TokenCheck> {
    let host = Url::parse(origin).ok()?.host_str()?.to_ascii_lowercase();
    if host == "github.com" || host == "api.github.com" {
        Some(TokenCheck::GitHub(
            "https://api.github.com/user".to_string(),
        ))
    } else if host.starts_with("github.") {
        // GitHub Enterprise Server serves its REST API under /api/v3.
        Some(TokenCheck::GitHub(format!("{origin}/api/v3/user")))
    } else if host == "gitlab.com" || host.starts_with("gitlab.") {
        Some(TokenCheck::GitLab(format!("{origin}/api/v4/user")))
    } else {
        None
    }
}

async fn validate_github(client: &Client, url: &str, token: &str) -> Result<(), String> {
    let response = client
        .get(url)
        .header(USER_AGENT, HeaderValue::from_static("gitspace"))
        .header(AUTHORIZATION, format!("Bearer {}", token))
        .send()
        .await
        .map_err(|err| format!("GitHub validation failed: {err}"))?;

    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("GitHub rejected token ({}).", response.status()))
    }
}

async fn validate_gitlab(client: &Client, url: &str, token: &str) -> Result<(), String> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static("gitspace"));
    headers.insert(
        HeaderName::from_static("private-token"),
        HeaderValue::from_str(token).map_err(|err| err.to_string())?,
    );

    let response = client
        .get(url)
        .headers(headers)
        .send()
        .await
        .map_err(|err| format!("GitLab validation failed: {err}"))?;

    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("GitLab rejected token ({}).", response.status()))
    }
}

/// Returns the lowercase host name of a URL, `host:port`, scp-like `user@host:path` or bare
/// host.
pub fn extract_host(target: &str) -> Option<String> {
    let target = target.trim();
    let host = match Url::parse(target) {
        // `gitlab.example.com:8443` parses as a URL with scheme `gitlab.example.com` and no
        // host, so only a URL with a host is taken.
        Ok(url) if url.host_str().is_some() => url.host_str().unwrap_or_default().to_string(),
        _ => {
            let rest = target.split_once("://").map_or(target, |(_, rest)| rest);
            let authority = rest.split('/').next().unwrap_or_default();
            let authority = authority
                .rsplit_once('@')
                .map_or(authority, |(_, host)| host);
            authority.split(':').next().unwrap_or_default().to_string()
        }
    };
    let host = host.to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

/// The key a host's token is saved under: the host name [`extract_host`] finds, so a token
/// typed for `https://GitLab.example.com:8443/` is found again for any remote on that host.
fn storage_key(host: &str) -> String {
    extract_host(host).unwrap_or_else(|| host.trim().to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_encryption_round_trips_with_fresh_nonces() {
        let key: [u8; 32] = Generate::generate();
        let mut map = TokenMap::default();
        map.tokens.insert("github.com".into(), "secret".into());

        let first = encrypt_tokens(&map, &key).expect("encrypt");
        let second = encrypt_tokens(&map, &key).expect("encrypt");
        assert_ne!(first.nonce, second.nonce);

        let decrypted = decrypt_tokens(&first, &key).expect("decrypt");
        assert_eq!(decrypted.tokens, map.tokens);
    }

    // Known answers computed with OpenSSL, independently of the crates in Cargo.toml: a
    // crate update that changed either result would lock users out of their saved tokens.

    #[test]
    fn saved_token_file_decrypts_with_a_known_key() {
        let key: [u8; 32] = std::array::from_fn(|i| i as u8);
        let blob = EncryptedTokenFile {
            nonce: "ZGVmZ2hpamtsbW5v".into(),
            ciphertext: "TzPPpfPAGCHZ5G5dleCs+wWkvFfwPcmX5Fie3aVZopy91+2BwUk9Fu2wvoKDxobquaZzixP/xoVED+ssLExO+hF9".into(),
        };
        let tokens = decrypt_tokens(&blob, &key).expect("decrypt").tokens;
        assert_eq!(tokens["github.com"], "gho_known_answer_token");

        let short_nonce = EncryptedTokenFile {
            nonce: "AAAA".into(),
            ..blob
        };
        let err = decrypt_tokens(&short_nonce, &key).expect_err("a 3-byte nonce is invalid");
        assert!(err.contains("nonce"), "{err}");
    }

    #[test]
    fn master_password_derives_a_known_key() {
        let salt: [u8; 16] = std::array::from_fn(|i| i as u8);
        let password: [u8; 20] = std::array::from_fn(|i| b'a' + i as u8);
        let mut derived = [0u8; 32];
        assert!(derive_master_key(&password, &salt, &mut derived));
        assert_eq!(
            derived,
            [
                0xb5, 0xdd, 0x0e, 0x57, 0xa7, 0x51, 0xbf, 0xd0, 0x66, 0xa3, 0x6b, 0x14, 0x61, 0x4e,
                0x1f, 0x7d, 0xd3, 0xfb, 0x03, 0x1a, 0x01, 0xe0, 0x21, 0x8f, 0xf7, 0xaa, 0x9b, 0x74,
                0x0f, 0xdc, 0xdb, 0xf9,
            ]
        );
    }

    /// A storage in `dir` with a fresh random key, so two calls never share a key.
    fn storage_in(dir: &std::path::Path) -> TokenStorage {
        TokenStorage {
            key: SecureKey(Generate::generate()),
            local_key_path: Some(dir.join(TOKEN_LOCAL_KEY_FILE)),
            path: dir.join(TOKEN_FILE_NAME),
            host_path: dir.join(HOST_FILE_NAME),
            allow_encrypted_fallback: true,
        }
    }

    #[test]
    fn undecryptable_token_file_is_not_overwritten() {
        let dir = tempfile::tempdir().expect("tempdir");
        let original = storage_in(dir.path());
        original
            .persist_fallback("github.com", "first-token")
            .expect("first save");
        original
            .persist_fallback("gitlab.com", "second-token")
            .expect("second save");
        let before = fs::read(&original.path).expect("read tokens");

        // Same file, other key: the keyring was locked or the master password changed.
        let other_key = storage_in(dir.path());
        let err = other_key
            .persist_fallback("example.com", "third-token")
            .expect_err("an undecryptable file must not be rewritten");
        assert!(err.contains("left untouched"), "{err}");
        assert!(other_key.read_fallback_for_update().is_err());

        assert_eq!(fs::read(&original.path).expect("read tokens"), before);
        let tokens = original.read_fallback().expect("decrypt").tokens;
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens["gitlab.com"], "second-token");
    }

    #[test]
    fn sign_out_scrubs_the_token_file_with_the_fallback_off() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut storage = storage_in(dir.path());
        storage
            .persist_fallback("gitlab.com", "old-token")
            .expect("save");
        storage
            .persist_fallback("github.com", "kept-token")
            .expect("save");

        storage.set_allow_encrypted_fallback(false);
        storage.scrub_fallback("gitlab.com").expect("scrub");
        let tokens = storage.read_fallback().expect("decrypt").tokens;
        assert!(!tokens.contains_key("gitlab.com"));
        assert_eq!(tokens["github.com"], "kept-token");

        // A file this key cannot decrypt is not in use while the fallback is off.
        let before = fs::read(&storage.path).expect("read tokens");
        let mut other_key = storage_in(dir.path());
        other_key.set_allow_encrypted_fallback(false);
        other_key.scrub_fallback("github.com").expect("ignored");
        other_key.set_allow_encrypted_fallback(true);
        assert!(other_key.scrub_fallback("github.com").is_err());
        assert_eq!(fs::read(&storage.path).expect("read tokens"), before);
    }

    #[test]
    fn wrong_size_secret_is_never_overwritten() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("gitspace").join(TOKEN_LOCAL_KEY_FILE);

        let created: [u8; 32] = load_or_create_secret(&path);
        assert_eq!(load_or_create_secret::<32>(&path), created);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).expect("metadata").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        // A truncated write: a session key is used and the file stays as it is.
        fs::write(&path, &created[..10]).expect("truncate");
        let session: [u8; 32] = load_or_create_secret(&path);
        assert_ne!(session, created);
        assert_eq!(fs::read(&path).expect("read"), &created[..10]);
    }

    #[cfg(unix)]
    #[test]
    fn token_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let storage = storage_in(dir.path());
        storage
            .persist_fallback("github.com", "token")
            .expect("save");
        // A file left 0644 by an older release is made owner-only on the next write.
        fs::set_permissions(&storage.path, fs::Permissions::from_mode(0o644)).expect("chmod");
        storage
            .persist_fallback("gitlab.com", "token")
            .expect("save");
        storage.record_host("github.com").expect("record");
        for path in [&storage.path, &storage.host_path] {
            let mode = fs::metadata(path).expect("metadata").permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "{}", path.display());
        }
    }

    #[test]
    fn tokens_saved_with_the_local_key_open_once_the_keyring_is_back() {
        let dir = tempfile::tempdir().expect("tempdir");
        // First run, keyring locked: the local key encrypts the file.
        let local_key: [u8; 32] = load_or_create_secret(&dir.path().join(TOKEN_LOCAL_KEY_FILE));
        let locked = TokenStorage {
            key: SecureKey(local_key),
            local_key_path: None,
            ..storage_in(dir.path())
        };
        locked
            .persist_fallback("github.com", "token")
            .expect("save");

        // Next run, keyring unlocked: another key, the local one as a fallback.
        let unlocked = storage_in(dir.path());
        assert_eq!(
            unlocked.read_fallback().expect("decrypt").tokens["github.com"],
            "token"
        );
        // The file is now encrypted with the keyring key.
        let blob: EncryptedTokenFile =
            serde_json::from_slice(&fs::read(&unlocked.path).expect("read")).expect("parse");
        assert!(decrypt_tokens(&blob, &unlocked.key.0).is_ok());
        assert!(decrypt_tokens(&blob, &local_key).is_err());
    }

    #[test]
    fn missing_token_file_starts_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let storage = storage_in(&dir.path().join("gitspace"));
        storage
            .persist_fallback("github.com", "token")
            .expect("save into a new directory");
        assert_eq!(
            storage.read_fallback().expect("decrypt").tokens["github.com"],
            "token"
        );
    }

    #[test]
    fn unreadable_host_index_is_not_overwritten() {
        let dir = tempfile::tempdir().expect("tempdir");
        let storage = storage_in(dir.path());
        fs::write(&storage.host_path, "not json").expect("write index");
        assert!(storage.record_host("github.com").is_err());
        assert!(storage.remove_host("github.com").is_err());
        assert_eq!(
            fs::read_to_string(&storage.host_path).expect("read index"),
            "not json"
        );
    }

    #[test]
    fn normalize_host_only_allows_https() {
        assert_eq!(
            normalize_host(" gitlab.example.com/ ").as_deref(),
            Ok("https://gitlab.example.com")
        );
        assert_eq!(
            normalize_host("https://git.example.com:8443/group").as_deref(),
            Ok("https://git.example.com:8443")
        );
        assert!(normalize_host("http://gitlab.example.com").is_err());
        assert!(normalize_host("HTTP://gitlab.example.com").is_err());
        assert!(normalize_host("ssh://gitlab.example.com").is_err());
    }

    #[test]
    fn token_check_matches_the_exact_host() {
        let check = |host: &str| token_check(&normalize_host(host).unwrap());
        let github = |url: &str| Some(TokenCheck::GitHub(url.to_string()));
        let gitlab = |url: &str| Some(TokenCheck::GitLab(url.to_string()));

        assert_eq!(check("github.com"), github("https://api.github.com/user"));
        assert_eq!(
            check("API.GitHub.com"),
            github("https://api.github.com/user")
        );
        // An Enterprise host containing "github.com" keeps its token on its own server.
        assert_eq!(
            check("github.company.com"),
            github("https://github.company.com/api/v3/user")
        );
        assert_eq!(
            check("https://github.example.org:8443/org"),
            github("https://github.example.org:8443/api/v3/user")
        );
        assert_eq!(
            check("gitlab.com"),
            gitlab("https://gitlab.com/api/v4/user")
        );
        assert_eq!(
            check("gitlab.example.com"),
            gitlab("https://gitlab.example.com/api/v4/user")
        );
        assert_eq!(check("notgithub.com"), None);
        assert_eq!(check("mygitlab.example.com"), None);
        assert_eq!(check("git.example.com"), None);
    }

    #[test]
    fn manual_hosts_are_saved_under_the_key_a_remote_looks_up() {
        let remote = extract_host("https://gitlab.example.com/group/repo.git");
        assert_eq!(remote.as_deref(), Some("gitlab.example.com"));
        for typed in [
            "gitlab.example.com",
            "GitLab.example.com",
            "gitlab.example.com:8443",
            " gitlab.example.com/group ",
            "https://gitlab.example.com",
            "https://GitLab.Example.com:8443/",
        ] {
            assert_eq!(Some(storage_key(typed)), remote, "{typed}");
        }
        for remote in [
            "ssh://git@GitLab.example.com:22/group/repo.git",
            "git@gitlab.example.com:group/repo.git",
        ] {
            assert_eq!(
                extract_host(remote).as_deref(),
                Some("gitlab.example.com"),
                "{remote}"
            );
        }
        assert_eq!(extract_host(""), None);
        assert_eq!(extract_host("file:///home/me/repo"), None);
    }

    #[tokio::test]
    async fn plain_http_host_is_refused_before_any_request() {
        let dir = tempfile::tempdir().expect("tempdir");
        let auth = AuthManager {
            storage: storage_in(dir.path()),
        };
        let err = auth
            .validate_token("http://gitlab.example.com", "glpat-0123456789abcdef")
            .await
            .expect_err("plain HTTP host");
        assert!(err.contains("https"), "{err}");
    }
}
