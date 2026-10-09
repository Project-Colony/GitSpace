pub mod oauth;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::ErrorKind;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose, Engine as _};
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use keyring::Entry;
use rand::rngs::OsRng;
use rand::RngCore;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, USER_AGENT};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};
use url::Url;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::config::write_atomic;

pub use oauth::{OAuthFlow, OAuthProvider, OAuthResult, OAuthToken};

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

    pub fn resolve_for_host(&self, host: &str) -> Option<String> {
        self.storage.get_token(host).ok().flatten()
    }

    pub fn resolve_for_url(&self, url: &str) -> Option<String> {
        let host = extract_host(url)?;
        self.resolve_for_host(&host)
            .or_else(|| match host.as_str() {
                "github.com" => self.resolve_for_host("api.github.com"),
                _ => None,
            })
    }

    #[allow(dead_code)]
    pub fn set_token(&self, host: &str, token: &str) -> Result<(), String> {
        self.storage.set_token(host, token)
    }

    pub fn clear_token(&self, host: &str) -> Result<(), String> {
        self.storage.clear_token(host)
    }

    pub fn known_hosts(&self) -> Vec<String> {
        self.storage.known_hosts()
    }

    pub fn validate_token(&self, host: &str, token: &str) -> Result<(), String> {
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

        let normalized_host = normalize_host(host)?;
        let client = get_http_client();

        if normalized_host.contains("github") {
            validate_github(client, &normalized_host, token)
        } else if normalized_host.contains("gitlab") {
            validate_gitlab(client, &normalized_host, token)
        } else {
            Ok(())
        }
    }

    pub fn validate_and_store(&self, host: &str, token: &str) -> Result<(), String> {
        self.validate_token(host, token)?;
        self.storage.set_token(host, token)
    }

    pub fn set_encrypted_fallback(&mut self, allowed: bool) {
        self.storage.set_allow_encrypted_fallback(allowed);
    }

    /// Stores an OAuth token for a host.
    pub fn store_oauth_token(&self, token: &OAuthToken) -> Result<(), String> {
        // Store the access token using the existing mechanism
        self.storage.set_token(&token.host, &token.access_token)?;

        // Store the full OAuth token (with refresh token) as JSON in a separate entry
        let oauth_key = format!("oauth:{}", token.host);
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
        let oauth_key = format!("oauth:{}", host);
        let oauth_json = self.storage.get_token(&oauth_key).ok()??;
        serde_json::from_str(&oauth_json).ok()
    }

    /// Checks if a host has OAuth authentication configured.
    pub fn has_oauth(&self, host: &str) -> bool {
        self.get_oauth_token(host).is_some()
    }

    /// Starts an OAuth flow for the given provider.
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
        let oauth_key = format!("oauth:{}", host);
        let _ = self.storage.clear_token(&oauth_key);
        self.storage.clear_token(host)
    }
}

/// Secure token storage with automatic key zeroization.
#[derive(Clone)]
pub struct TokenStorage {
    key: SecureKey,
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
        let key = load_or_create_keyring_key()
            .map_err(|err| {
                warn!(target: "gitspace::auth", error = %err, "failed to access keyring encryption key");
            })
            .unwrap_or_else(|_| load_or_create_local_key());
        let path = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(SERVICE_NAME)
            .join(TOKEN_FILE_NAME);
        let host_path = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(SERVICE_NAME)
            .join(HOST_FILE_NAME);
        Self {
            key: SecureKey(key),
            path,
            host_path,
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

    pub fn clear_token(&self, host: &str) -> Result<(), String> {
        if let Err(err) = self.remove_from_keyring(host) {
            warn!(target: "gitspace::auth", error = %err, "failed to clear token from native keyring");
        }
        let mut result = if self.allow_encrypted_fallback {
            let mut map = self.read_fallback_for_update()?;
            map.tokens.remove(host);
            self.write_fallback(&map)
        } else {
            Ok(())
        };

        if let Err(err) = self.remove_host(host) {
            warn!(target: "gitspace::auth", error = %err, "failed to update saved host list");
            if result.is_ok() {
                result = Err(err);
            }
        }

        result
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
        match entry.delete_password() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
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
        write_atomic(&self.path, serialized.as_bytes())
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
        decrypt_tokens(&blob, &self.key.0)
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
        write_atomic(&self.host_path, serialized.as_bytes())
            .map_err(|err| format!("Failed to write host index: {err}"))
    }

    pub fn set_allow_encrypted_fallback(&mut self, allowed: bool) {
        self.allow_encrypted_fallback = allowed;
    }
}

fn encrypt_tokens(map: &TokenMap, key: &[u8; 32]) -> Result<EncryptedTokenFile, String> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);

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
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let nonce_bytes = general_purpose::STANDARD
        .decode(&blob.nonce)
        .map_err(|err| format!("Failed to decode nonce: {err}"))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let cipher_bytes = general_purpose::STANDARD
        .decode(&blob.ciphertext)
        .map_err(|err| format!("Failed to decode ciphertext: {err}"))?;

    let plaintext = cipher
        .decrypt(nonce, cipher_bytes.as_ref())
        .map_err(|err| format!("Failed to decrypt credentials: {err}"))?;
    let content = String::from_utf8(plaintext)
        .map_err(|err| format!("Invalid credential encoding: {err}"))?;
    serde_json::from_str(&content)
        .map_err(|err| format!("Failed to parse decrypted credentials: {err}"))
}

/// Loads or creates a secure local encryption key.
///
/// SECURITY: This function generates a cryptographically secure random key
/// and stores it in a file with restricted permissions (0600).
/// If a master password is provided via environment variable, it's used to
/// derive an additional key that's XORed with the stored key for defense in depth.
fn load_or_create_local_key() -> [u8; 32] {
    let key_path = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(SERVICE_NAME)
        .join(TOKEN_LOCAL_KEY_FILE);

    // Try to load existing key
    let mut stored_key = match fs::read(&key_path) {
        Ok(bytes) if bytes.len() == 32 => {
            let mut key = [0u8; 32];
            key.copy_from_slice(&bytes);
            key
        }
        Ok(_) => {
            warn!(target: "gitspace::auth", "invalid local key file size, regenerating");
            generate_and_store_key(&key_path)
        }
        Err(err) if err.kind() == ErrorKind::NotFound => {
            info!(target: "gitspace::auth", "generating new local encryption key");
            generate_and_store_key(&key_path)
        }
        Err(err) => {
            // Replacing a key file that exists but cannot be read would make the tokens
            // encrypted with it unrecoverable. Use a key for this session only: the saved
            // tokens then fail to decrypt and are left untouched.
            error!(target: "gitspace::auth", error = %err, "failed to read local key file");
            let mut key = [0u8; 32];
            OsRng.fill_bytes(&mut key);
            key
        }
    };

    // If master password is set, derive additional key material for defense in depth
    if let Ok(master_password) = std::env::var(MASTER_PASSWORD_ENV) {
        let salt = load_or_create_secret(TOKEN_SALT_FILE, 16);
        // OWASP recommended: 64 MB memory, 3 iterations, 1 thread
        // m_cost is in KiB, so 65536 = 64 MB
        let params = Params::new(65536, 3, 1, None).unwrap_or(Params::DEFAULT);
        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

        let mut derived = [0u8; 32];
        if argon2
            .hash_password_into(master_password.as_bytes(), &salt, &mut derived)
            .is_ok()
        {
            // XOR the derived key with the stored key for defense in depth
            for (a, b) in stored_key.iter_mut().zip(derived.iter()) {
                *a ^= b;
            }
        }
        // Zero the derived key
        derived.zeroize();
    }

    stored_key
}

/// Generates a cryptographically secure random key and stores it with restricted permissions.
fn generate_and_store_key(path: &PathBuf) -> [u8; 32] {
    let mut key = [0u8; 32];
    OsRng.fill_bytes(&mut key);

    if let Some(parent) = path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            error!(target: "gitspace::auth", error = %err, "failed to create key directory");
            return key;
        }
    }

    // Write key file
    if let Err(err) = fs::write(path, key) {
        error!(target: "gitspace::auth", error = %err, "failed to write local key file");
        return key;
    }

    // Set restrictive permissions (owner read/write only)
    #[cfg(unix)]
    {
        if let Err(err) = fs::set_permissions(path, fs::Permissions::from_mode(0o600)) {
            warn!(target: "gitspace::auth", error = %err, "failed to set key file permissions");
        }
    }

    key
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
            let mut key = [0u8; 32];
            OsRng.fill_bytes(&mut key);
            let encoded = general_purpose::STANDARD.encode(key);
            entry
                .set_password(&encoded)
                .map_err(|err| format!("Failed to store token key in keyring: {err}"))?;
            Ok(key)
        }
        Err(err) => Err(format!("Failed to read keyring token key: {err}")),
    }
}

fn load_or_create_secret(name: &str, len: usize) -> Vec<u8> {
    let path = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(SERVICE_NAME)
        .join(name);

    let mut secret = vec![0u8; len];
    OsRng.fill_bytes(&mut secret);
    match fs::read(&path) {
        Ok(bytes) if bytes.len() == len => return bytes,
        Err(err) if err.kind() != ErrorKind::NotFound => {
            // Same as the local key: never replace a secret that exists but cannot be read.
            warn!(target: "gitspace::auth", error = %err, path = %path.display(), "unable to read derived-key secret; using one for this session only");
            return secret;
        }
        _ => {}
    }
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Err(err) = fs::write(&path, &secret) {
        warn!(target: "gitspace::auth", error = %err, path = %path.display(), "unable to persist derived-key secret");
    } else {
        // Set restrictive permissions (owner read/write only)
        #[cfg(unix)]
        {
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
        }
    }
    secret
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

fn validate_github(client: &Client, host: &str, token: &str) -> Result<(), String> {
    let api_base = if host.contains("api.github.com") {
        host.to_string()
    } else if host.contains("github.com") {
        "https://api.github.com".to_string()
    } else {
        format!("{}/api/v3", host)
    };
    let url = format!("{}/user", api_base.trim_end_matches('/'));

    let response = client
        .get(url)
        .header(USER_AGENT, HeaderValue::from_static("gitspace"))
        .header(AUTHORIZATION, format!("Bearer {}", token))
        .send()
        .map_err(|err| format!("GitHub validation failed: {err}"))?;

    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("GitHub rejected token ({}).", response.status()))
    }
}

fn validate_gitlab(client: &Client, host: &str, token: &str) -> Result<(), String> {
    let url = format!("{}/api/v4/user", host.trim_end_matches('/'));
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
        .map_err(|err| format!("GitLab validation failed: {err}"))?;

    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("GitLab rejected token ({}).", response.status()))
    }
}

pub fn extract_host(target: &str) -> Option<String> {
    if let Ok(url) = Url::parse(target) {
        return url.host_str().map(|h| h.to_string());
    }

    if let Some((host, _)) = target.split_once("://") {
        return Some(host.to_string());
    }

    if let Some((user_host, _)) = target.split_once(':') {
        if let Some((_, host)) = user_host.split_once('@') {
            return Some(host.to_string());
        }
    }

    target
        .split('/')
        .next()
        .filter(|segment| !segment.is_empty())
        .map(|h| h.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_encryption_round_trips_with_fresh_nonces() {
        let key: [u8; 32] = ChaCha20Poly1305::generate_key(&mut OsRng).into();
        let mut map = TokenMap::default();
        map.tokens.insert("github.com".into(), "secret".into());

        let first = encrypt_tokens(&map, &key).expect("encrypt");
        let second = encrypt_tokens(&map, &key).expect("encrypt");
        assert_ne!(first.nonce, second.nonce);

        let decrypted = decrypt_tokens(&first, &key).expect("decrypt");
        assert_eq!(decrypted.tokens, map.tokens);
    }

    /// A storage in `dir` with a fresh random key, so two calls never share a key.
    fn storage_in(dir: &std::path::Path) -> TokenStorage {
        TokenStorage {
            key: SecureKey(ChaCha20Poly1305::generate_key(&mut OsRng).into()),
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
    fn plain_http_host_is_refused_before_any_request() {
        let dir = tempfile::tempdir().expect("tempdir");
        let auth = AuthManager {
            storage: storage_in(dir.path()),
        };
        let err = auth
            .validate_token("http://gitlab.example.com", "glpat-0123456789abcdef")
            .expect_err("plain HTTP host");
        assert!(err.contains("https"), "{err}");
    }
}
