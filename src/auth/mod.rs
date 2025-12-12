use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use argon2::{Algorithm, Argon2, Params, Version};
use base64::{Engine as _, engine::general_purpose};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hostname::get as get_hostname;
use keyring::Entry;
use rand::RngCore;
use rand::rngs::OsRng;
use reqwest::blocking::Client;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue, USER_AGENT};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{error, warn};
use url::Url;

const SERVICE_NAME: &str = "gitspace";
const TOKEN_FILE_NAME: &str = "tokens.enc";
const TOKEN_SALT_FILE: &str = "token-salt.bin";
const TOKEN_PEPPER_FILE: &str = "token-pepper.bin";
const MASTER_PASSWORD_ENV: &str = "GITSPACE_TOKEN_MASTER_PASSWORD";

#[derive(Debug, Clone)]
pub struct AuthManager {
    storage: TokenStorage,
}

impl AuthManager {
    pub fn new() -> Self {
        Self {
            storage: TokenStorage::new(false),
        }
    }

    pub fn with_encrypted_fallback(allow_encrypted_fallback: bool) -> Self {
        Self {
            storage: TokenStorage::new(allow_encrypted_fallback),
        }
    }

    pub fn resolve_for_host(&self, host: &str) -> Option<String> {
        self.storage.get_token(host).ok().flatten()
    }

    pub fn resolve_for_url(&self, url: &str) -> Option<String> {
        extract_host(url).and_then(|host| self.resolve_for_host(&host))
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
        if token.trim().is_empty() {
            return Err("Token cannot be empty".to_string());
        }
        let normalized_host = normalize_host(host);
        let client = Client::builder()
            .user_agent("gitspace")
            .build()
            .map_err(|err| err.to_string())?;

        if normalized_host.contains("github") {
            validate_github(&client, &normalized_host, token)
        } else if normalized_host.contains("gitlab") {
            validate_gitlab(&client, &normalized_host, token)
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
}

#[derive(Debug, Clone)]
pub struct TokenStorage {
    key: [u8; 32],
    path: PathBuf,
    allow_encrypted_fallback: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct TokenMap {
    tokens: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EncryptedTokenFile {
    nonce: String,
    ciphertext: String,
}

impl TokenStorage {
    pub fn new(allow_encrypted_fallback: bool) -> Self {
        let key = derive_local_key();
        let path = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(SERVICE_NAME)
            .join(TOKEN_FILE_NAME);
        Self {
            key,
            path,
            allow_encrypted_fallback,
        }
    }

    pub fn set_token(&self, host: &str, token: &str) -> Result<(), String> {
        let keyring_result = self.store_in_keyring(host, token);
        if let Err(ref err) = keyring_result {
            warn!(target: "gitspace::auth", error = %err, "failed to store token in native keyring");
        }

        if self.allow_encrypted_fallback {
            self.persist_fallback(host, token)
        } else if keyring_result.is_err() {
            Err("Native keyring unavailable and encrypted storage is disabled".to_string())
        } else {
            Ok(())
        }
    }

    pub fn get_token(&self, host: &str) -> Result<Option<String>, String> {
        if let Some(token) = self.fetch_from_keyring(host)? {
            return Ok(Some(token));
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
        if self.allow_encrypted_fallback {
            let mut map = self.read_fallback()?;
            map.tokens.remove(host);
            self.write_fallback(&map)
        } else {
            Ok(())
        }
    }

    pub fn known_hosts(&self) -> Vec<String> {
        if self.allow_encrypted_fallback {
            self.read_fallback()
                .map(|map| map.tokens.keys().cloned().collect())
                .unwrap_or_default()
        } else {
            Vec::new()
        }
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
        let mut tokens = self.read_fallback().unwrap_or_default();
        tokens.tokens.insert(host.to_string(), token.to_string());
        self.write_fallback(&tokens)
    }

    fn write_fallback(&self, map: &TokenMap) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("Failed to prepare credential directory: {err}"))?;
        }
        let blob = encrypt_tokens(map, &self.key)?;
        let serialized = serde_json::to_string_pretty(&blob)
            .map_err(|err| format!("Failed to serialize credentials: {err}"))?;
        fs::write(&self.path, serialized)
            .map_err(|err| format!("Failed to write credentials: {err}"))
    }

    fn read_fallback(&self) -> Result<TokenMap, String> {
        if !self.path.exists() {
            return Ok(TokenMap::default());
        }
        let data = fs::read_to_string(&self.path)
            .map_err(|err| format!("Failed to read credential file: {err}"))?;
        let blob: EncryptedTokenFile = serde_json::from_str(&data)
            .map_err(|err| format!("Failed to parse credential file: {err}"))?;
        decrypt_tokens(&blob, &self.key)
    }

    pub fn set_allow_encrypted_fallback(&mut self, allowed: bool) {
        self.allow_encrypted_fallback = allowed;
    }
}

fn encrypt_tokens(map: &TokenMap, key: &[u8; 32]) -> Result<EncryptedTokenFile, String> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let nonce_obj = Nonce::from_slice(&nonce);

    let serialized =
        serde_json::to_string(map).map_err(|err| format!("Failed to serialize tokens: {err}"))?;
    let encrypted = cipher
        .encrypt(nonce_obj, serialized.as_bytes())
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

fn derive_local_key() -> [u8; 32] {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "gitspace".to_string());
    let host = get_hostname()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "localhost".to_string());
    let master_password = std::env::var(MASTER_PASSWORD_ENV)
        .unwrap_or_else(|_| format!("{}:{}:{}", user, host, std::env::consts::OS));

    let salt = load_or_create_secret(TOKEN_SALT_FILE, 16);
    let pepper = load_or_create_secret(TOKEN_PEPPER_FILE, 32);

    let mut keyed = Sha256::new();
    keyed.update(master_password.as_bytes());
    keyed.update(&pepper);
    let password_material = keyed.finalize();

    let params = Params::new(32, 3, 1, None).unwrap_or_else(|_| Params::DEFAULT);
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    let mut key = [0u8; 32];
    if let Err(err) = argon2.hash_password_into(&password_material, &salt, &mut key) {
        error!(target: "gitspace::auth", error = %err, "failed to derive local token key");
        let fallback = Sha256::digest(&password_material);
        key.copy_from_slice(&fallback[..32]);
    }
    key
}

fn load_or_create_secret(name: &str, len: usize) -> Vec<u8> {
    let path = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(SERVICE_NAME)
        .join(name);

    if let Ok(bytes) = fs::read(&path) {
        if bytes.len() == len {
            return bytes;
        }
    }

    let mut secret = vec![0u8; len];
    OsRng.fill_bytes(&mut secret);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Err(err) = fs::write(&path, &secret) {
        warn!(target: "gitspace::auth", error = %err, path = %path.display(), "unable to persist derived-key secret");
    }
    secret
}

fn normalize_host(host: &str) -> String {
    let trimmed = host.trim().trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("https://{}", trimmed)
    }
}

fn validate_github(client: &Client, host: &str, token: &str) -> Result<(), String> {
    let api_base = if host.contains("api.github.com") {
        host.to_string()
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

    if let Some((user_host, _)) = target.split_once(':')
        && let Some((_, host)) = user_host.split_once('@')
    {
        return Some(host.to_string());
    }

    target
        .split('/')
        .next()
        .filter(|segment| !segment.is_empty())
        .map(|h| h.to_string())
}
