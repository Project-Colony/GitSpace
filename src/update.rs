use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use ed25519_dalek::{Signature as Ed25519Signature, VerifyingKey};
use reqwest::Proxy;
use reqwest::blocking::Client;
use rsa::RsaPublicKey;
use rsa::pkcs1v15::Signature as RsaSignature;
use rsa::pkcs1v15::VerifyingKey as RsaVerifyingKey;
use rsa::pkcs8::DecodePublicKey;
use rsa::signature::Verifier;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use spki::der::{Decode, Encode};
use spki::{AlgorithmIdentifierRef, ObjectIdentifier, SubjectPublicKeyInfoRef};
use x509_cert::Certificate;

use crate::config::{NetworkOptions, ReleaseChannel, app_data_dir};

const DEFAULT_RELEASE_FEED: &str = "https://api.github.com/repos/gitspace-app/GitSpace/releases";
const SIGNING_KEY_FILE: &str = "update-signing.pem";
const EMBEDDED_SIGNING_KEY: &str = "-----BEGIN PUBLIC KEY-----
MCowBQYDK2VwAyEAtZR3haYs4DLQXGepshiHit+bttO4OsGZxiiByTmmOJ4=
-----END PUBLIC KEY-----";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureAlgorithm {
    Ed25519,
    RsaSha256,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureMetadata {
    pub algorithm: SignatureAlgorithm,
    pub public_key: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub name: String,
    pub download_url: String,
    pub checksum: Option<String>,
    pub signature_url: Option<String>,
    pub signature: Option<SignatureMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub version: String,
    pub url: String,
    pub notes: Option<String>,
    pub channel: ReleaseChannel,
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateError {
    Network(String),
    Verification(String),
    InvalidResponse(String),
    Io(String),
    Policy(String),
}

impl std::fmt::Display for UpdateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Network(msg)
            | Self::Verification(msg)
            | Self::InvalidResponse(msg)
            | Self::Io(msg)
            | Self::Policy(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for UpdateError {}

impl From<reqwest::Error> for UpdateError {
    fn from(value: reqwest::Error) -> Self {
        if value.is_timeout() {
            Self::Network("The update request timed out.".to_string())
        } else if value.is_status() {
            Self::InvalidResponse(format!("HTTP error: {value}"))
        } else {
            Self::Network(value.to_string())
        }
    }
}

impl From<std::io::Error> for UpdateError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.to_string())
    }
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    prerelease: bool,
    body: Option<String>,
    assets: Vec<GitHubAsset>,
}

pub type UpdateResult = Result<Option<ReleaseInfo>, UpdateError>;

pub fn check_for_updates(
    channel: ReleaseChannel,
    feed_override: Option<&str>,
    network: &NetworkOptions,
) -> UpdateResult {
    let url = feed_override.unwrap_or(DEFAULT_RELEASE_FEED);
    ensure_https_policy(url, network)?;

    let client = build_client(network)?;

    let response = client
        .get(url)
        .send()
        .map_err(UpdateError::from)?
        .error_for_status()
        .map_err(UpdateError::from)?;

    let releases: Vec<GitHubRelease> = response.json().map_err(|err| {
        UpdateError::InvalidResponse(format!("Failed to parse release feed: {err}"))
    })?;

    let desired_release = match channel {
        ReleaseChannel::Stable => releases.iter().find(|release| !release.prerelease),
        ReleaseChannel::Preview => releases.iter().find(|release| release.prerelease),
    };

    let Some(release) = desired_release else {
        return Ok(None);
    };

    let normalized_tag = release.tag_name.trim_start_matches('v');
    let current_version = env!("CARGO_PKG_VERSION");

    if normalized_tag == current_version {
        return Ok(None);
    }

    let assets = collect_assets(&client, release, network)?;

    Ok(Some(ReleaseInfo {
        version: normalized_tag.to_string(),
        url: release.html_url.clone(),
        notes: release.body.clone(),
        channel,
        assets,
    }))
}

#[allow(dead_code)]
pub fn download_verified_asset(
    network: &NetworkOptions,
    asset: &ReleaseAsset,
    destination: &Path,
) -> Result<(), UpdateError> {
    ensure_https_policy(&asset.download_url, network)?;
    let client = build_client(network)?;

    let backup = backup_existing(destination)?;

    let bytes = client
        .get(&asset.download_url)
        .send()
        .map_err(UpdateError::from)?
        .error_for_status()
        .map_err(UpdateError::from)?
        .bytes()
        .map_err(UpdateError::from)?;

    if let Err(err) = ensure_asset_verification(&bytes, asset, &client, network) {
        rollback_from_backup(destination, backup);
        return Err(err);
    }

    if let Err(err) = fs::write(destination, &bytes) {
        rollback_from_backup(destination, backup);
        return Err(UpdateError::Io(err.to_string()));
    }

    if let Some(backup_path) = backup {
        let _ = fs::remove_file(backup_path);
    }

    Ok(())
}

#[allow(dead_code)]
fn ensure_asset_verification(
    bytes: &[u8],
    asset: &ReleaseAsset,
    client: &Client,
    network: &NetworkOptions,
) -> Result<(), UpdateError> {
    if let Some(expected) = &asset.checksum {
        let computed = compute_sha256(bytes);
        if !expected.eq_ignore_ascii_case(&computed) {
            return Err(UpdateError::Verification(format!(
                "Checksum mismatch for {}: expected {}, got {}",
                asset.name, expected, computed
            )));
        }
        return Ok(());
    }

    if let Some(signature_url) = &asset.signature_url {
        ensure_https_policy(signature_url, network)?;
        let metadata = asset.signature.as_ref().ok_or_else(|| {
            UpdateError::Verification(format!("Signature metadata missing for {}", asset.name))
        })?;
        let signature = client
            .get(signature_url)
            .send()
            .map_err(UpdateError::from)?
            .error_for_status()
            .map_err(UpdateError::from)?
            .bytes()
            .map_err(UpdateError::from)?;

        if signature.is_empty() {
            return Err(UpdateError::Verification(format!(
                "Signature download for {} was empty",
                asset.name
            )));
        }

        return verify_signature(bytes, &signature, metadata);
    }

    Err(UpdateError::Verification(format!(
        "Asset {} is missing verification metadata",
        asset.name
    )))
}

fn verify_signature(
    bytes: &[u8],
    signature: &[u8],
    metadata: &SignatureMetadata,
) -> Result<(), UpdateError> {
    let normalized = normalize_signature(signature)?;
    match metadata.algorithm {
        SignatureAlgorithm::Ed25519 => {
            let key = build_ed25519_key(metadata)?;
            let signature = Ed25519Signature::from_slice(&normalized).map_err(|err| {
                UpdateError::Verification(format!("Invalid Ed25519 signature bytes: {err}"))
            })?;
            key.verify(bytes, &signature).map_err(|err| {
                UpdateError::Verification(format!("Signature verification failed: {err}"))
            })
        }
        SignatureAlgorithm::RsaSha256 => {
            let key = build_rsa_key(metadata)?;
            let signature = RsaSignature::try_from(normalized.as_slice()).map_err(|err| {
                UpdateError::Verification(format!("Invalid RSA signature bytes: {err}"))
            })?;
            let verifier = RsaVerifyingKey::<Sha256>::new(key);
            verifier.verify(bytes, &signature).map_err(|err| {
                UpdateError::Verification(format!("Signature verification failed: {err}"))
            })
        }
    }
}

fn normalize_signature(signature: &[u8]) -> Result<Vec<u8>, UpdateError> {
    let trimmed = String::from_utf8_lossy(signature).trim().to_string();
    if trimmed.is_empty() {
        return Err(UpdateError::Verification(
            "Signature payload was empty".to_string(),
        ));
    }

    if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(trimmed.as_bytes()) {
        return Ok(decoded);
    }

    Ok(signature.to_vec())
}

fn build_ed25519_key(metadata: &SignatureMetadata) -> Result<VerifyingKey, UpdateError> {
    VerifyingKey::from_public_key_der(&metadata.public_key)
        .or_else(|_| {
            let bytes: [u8; 32] = metadata.public_key.as_slice().try_into().map_err(|_| {
                UpdateError::Verification("Invalid Ed25519 public key length".to_string())
            })?;
            VerifyingKey::from_bytes(&bytes).map_err(|err| {
                UpdateError::Verification(format!("Invalid Ed25519 public key: {err}"))
            })
        })
        .map_err(|err| UpdateError::Verification(format!("Invalid Ed25519 public key: {err}")))
}

fn build_rsa_key(metadata: &SignatureMetadata) -> Result<RsaPublicKey, UpdateError> {
    RsaPublicKey::from_public_key_der(&metadata.public_key)
        .map_err(|err| UpdateError::Verification(format!("Invalid RSA public key: {err}")))
}

#[allow(dead_code)]
fn compute_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn collect_assets(
    client: &Client,
    release: &GitHubRelease,
    network: &NetworkOptions,
) -> Result<Vec<ReleaseAsset>, UpdateError> {
    let mut signing_metadata: Option<SignatureMetadata> = None;
    let mut checksums: HashMap<String, String> = HashMap::new();
    let mut signatures: HashMap<String, String> = HashMap::new();

    for asset in &release.assets {
        if let Some(target) = asset.name.strip_suffix(".sha256") {
            let checksum = fetch_checksum(client, &asset.browser_download_url, network)?;
            checksums.insert(target.to_string(), checksum);
        } else if let Some(target) = asset.name.strip_suffix(".sig") {
            signatures.insert(target.to_string(), asset.browser_download_url.clone());
            if signing_metadata.is_none() {
                signing_metadata = Some(load_signing_material()?);
            }
        }
    }

    let mut verified = Vec::new();
    for asset in &release.assets {
        if asset.name.ends_with(".sha256") || asset.name.ends_with(".sig") {
            continue;
        }

        let checksum = checksums.get(&asset.name).cloned();
        let signature_url = signatures.get(&asset.name).cloned();
        let signature = if signature_url.is_some() {
            Some(signing_metadata.clone().ok_or_else(|| {
                UpdateError::Verification(
                    "Signature metadata missing for signed release asset.".to_string(),
                )
            })?)
        } else {
            None
        };

        if checksum.is_some() || signature_url.is_some() || signature.is_some() {
            verified.push(ReleaseAsset {
                name: asset.name.clone(),
                download_url: asset.browser_download_url.clone(),
                checksum,
                signature_url,
                signature,
            });
        }
    }

    if verified.is_empty() {
        return Err(UpdateError::Verification(
            "No signed or checksummed assets found for the selected release.".to_string(),
        ));
    }

    Ok(verified)
}

fn load_signing_material() -> Result<SignatureMetadata, UpdateError> {
    if let Some(external) = read_external_signing_key()? {
        return Ok(external);
    }

    parse_signature_key(EMBEDDED_SIGNING_KEY.as_bytes())
}

fn read_external_signing_key() -> Result<Option<SignatureMetadata>, UpdateError> {
    let path = app_data_dir().join(SIGNING_KEY_FILE);
    if !path.exists() {
        return Ok(None);
    }

    let bytes = fs::read(&path).map_err(|err| UpdateError::Io(err.to_string()))?;
    if bytes.is_empty() {
        return Ok(None);
    }

    parse_signature_key(&bytes).map(Some)
}

fn parse_signature_key(bytes: &[u8]) -> Result<SignatureMetadata, UpdateError> {
    let pem = pem::parse(bytes)
        .map_err(|_| UpdateError::Verification("Failed to parse signing key PEM.".to_string()))?;

    let der = if pem.tag().contains("CERTIFICATE") {
        Certificate::from_der(pem.contents())
            .map_err(|err| {
                UpdateError::Verification(format!("Failed to parse certificate: {err}"))
            })?
            .tbs_certificate
            .subject_public_key_info
            .to_der()
            .map_err(|err| {
                UpdateError::Verification(format!("Failed to encode certificate public key: {err}"))
            })?
    } else if pem.tag().contains("PUBLIC KEY") {
        pem.contents().to_vec()
    } else {
        return Err(UpdateError::Verification(
            "Unsupported signing key format; expected a public key or certificate.".to_string(),
        ));
    };

    let spki = SubjectPublicKeyInfoRef::try_from(der.as_slice())
        .map_err(|err| UpdateError::Verification(format!("Unsupported signing key: {err}")))?;

    let algorithm = detect_algorithm(spki.algorithm)?;

    Ok(SignatureMetadata {
        algorithm,
        public_key: der,
    })
}

fn detect_algorithm(
    algorithm: AlgorithmIdentifierRef<'_>,
) -> Result<SignatureAlgorithm, UpdateError> {
    let ed25519_oid = ObjectIdentifier::new("1.3.101.112")
        .map_err(|err| UpdateError::Verification(format!("Invalid Ed25519 OID: {err}")))?;
    let rsa_oid = ObjectIdentifier::new("1.2.840.113549.1.1.1")
        .map_err(|err| UpdateError::Verification(format!("Invalid RSA OID: {err}")))?;

    if algorithm.oid == ed25519_oid {
        Ok(SignatureAlgorithm::Ed25519)
    } else if algorithm.oid == rsa_oid {
        Ok(SignatureAlgorithm::RsaSha256)
    } else {
        Err(UpdateError::Verification(format!(
            "Unsupported signature algorithm OID: {}",
            algorithm.oid
        )))
    }
}

fn fetch_checksum(
    client: &Client,
    url: &str,
    network: &NetworkOptions,
) -> Result<String, UpdateError> {
    ensure_https_policy(url, network)?;
    let body = client
        .get(url)
        .send()
        .map_err(UpdateError::from)?
        .error_for_status()
        .map_err(UpdateError::from)?
        .text()
        .map_err(UpdateError::from)?;

    let parsed = body
        .split_whitespace()
        .next()
        .ok_or_else(|| UpdateError::Verification("Checksum file was empty.".to_string()))?;

    // SHA-256 checksum must be exactly 64 hex characters
    if parsed.len() != 64 {
        return Err(UpdateError::Verification(format!(
            "Invalid SHA-256 checksum length: expected 64 characters, got {}",
            parsed.len()
        )));
    }

    // Validate hex format
    if !parsed.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(UpdateError::Verification(
            "Checksum contains invalid characters (must be hexadecimal)".to_string(),
        ));
    }

    Ok(parsed.to_lowercase())
}

fn build_client(network: &NetworkOptions) -> Result<Client, UpdateError> {
    let mut builder = Client::builder()
        .user_agent("GitSpace-Updater/0.1")
        .timeout(Duration::from_secs(network.network_timeout_secs.max(1)));

    if !network.http_proxy.is_empty() {
        builder = builder.proxy(
            Proxy::http(&network.http_proxy)
                .map_err(|err| UpdateError::Policy(format!("Invalid HTTP proxy: {err}")))?,
        );
    }

    if !network.https_proxy.is_empty() {
        builder = builder.proxy(
            Proxy::https(&network.https_proxy)
                .map_err(|err| UpdateError::Policy(format!("Invalid HTTPS proxy: {err}")))?,
        );
    }

    builder
        .build()
        .map_err(|err| UpdateError::Network(format!("Failed to build HTTP client: {err}")))
}

fn ensure_https_policy(url: &str, network: &NetworkOptions) -> Result<(), UpdateError> {
    if url.starts_with("https://") && !network.use_https {
        return Err(UpdateError::Policy(
            "HTTPS endpoints are disabled in your network preferences.".to_string(),
        ));
    }

    if url.starts_with("http://") && network.use_https {
        return Err(UpdateError::Policy(
            "HTTP endpoints are blocked. Enable HTTP in settings or switch to HTTPS.".to_string(),
        ));
    }

    Ok(())
}

#[allow(dead_code)]
fn backup_existing(path: &Path) -> Result<Option<PathBuf>, UpdateError> {
    if path.exists() {
        let fallback_name = path
            .file_name()
            .map(|name| name.to_string_lossy())
            .map(|name| name.to_string())
            .unwrap_or_else(|| "update".to_string());
        let backup = path.with_file_name(format!("{}.bak", fallback_name));
        fs::copy(path, &backup)?;
        return Ok(Some(backup));
    }

    Ok(None)
}

#[allow(dead_code)]
fn rollback_from_backup(path: &Path, backup: Option<PathBuf>) {
    if path.exists() {
        let _ = fs::remove_file(path);
    }

    if let Some(backup_path) = backup {
        let _ = fs::rename(&backup_path, path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rsa::RsaPrivateKey;
    use rsa::pkcs8::DecodePrivateKey;
    use rsa::signature::{SignatureEncoding, Signer};

    const TEST_ED25519_PRIVATE_KEY: &str = "REDACTED: private key removed from history";

    const TEST_RSA_PRIVATE_KEY: &str = "REDACTED: private key removed from history";

    const TEST_RSA_PUBLIC_KEY: &str = "-----BEGIN PUBLIC KEY-----
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAs+1d20i0/rTNWw/LhBUK
d39WUSRzUOGqSH4Hf0T8mDl3jkr/1sD8YUBvpnq3396itv50CpyzqVLMgv7tCpAT
+x4YeFn848N3YfHeIlzVOzqENdxus0lJ7ukF7vdJzl1t14tdgKDBgCmTQlz22paf
aCDgMkakx+F1uR4NYDKMnMMXBWxHDC5xz8CxIfQPcfujmU8nZlJSiuGDO8xflMBt
iexnQ4fi860vCwKsZAfMvBEzY1HSKb+kjXyh/SeHFA4qW+sWdpdfTSJwqoJTwqDv
TLayByl3pUMlyZWjvjnC6AvaEu3tNZ9Vh6O0k9HZwa38uZarX+mlRH3wvoiuFplB
hwIDAQAB
-----END PUBLIC KEY-----";

    fn embedded_metadata() -> SignatureMetadata {
        parse_signature_key(EMBEDDED_SIGNING_KEY.as_bytes()).expect("embedded key")
    }

    #[test]
    fn verifies_ed25519_signatures() {
        let pem = pem::parse(TEST_ED25519_PRIVATE_KEY).expect("private key");
        let signing = SigningKey::from_pkcs8_der(pem.contents()).expect("signing key");
        let payload = b"release-payload";
        let signature = signing.sign(payload);

        verify_signature(payload, &signature.to_bytes(), &embedded_metadata())
            .expect("signature valid");
    }

    #[test]
    fn rejects_corrupted_signature() {
        let pem = pem::parse(TEST_ED25519_PRIVATE_KEY).expect("private key");
        let signing = SigningKey::from_pkcs8_der(pem.contents()).expect("signing key");
        let payload = b"release-payload";
        let mut signature = signing.sign(payload).to_bytes();
        signature[0] ^= 0xFF;

        let err = verify_signature(payload, &signature, &embedded_metadata())
            .expect_err("signature should fail");
        assert!(matches!(err, UpdateError::Verification(_)));
    }

    #[test]
    fn rejects_missing_signature_metadata() {
        let asset = ReleaseAsset {
            name: "test.zip".to_string(),
            download_url: "https://example.com/test.zip".to_string(),
            checksum: None,
            signature_url: Some("https://example.com/test.sig".to_string()),
            signature: None,
        };
        let client = Client::builder().build().expect("client");
        let network = NetworkOptions::default();
        let err = ensure_asset_verification(b"payload", &asset, &client, &network)
            .expect_err("metadata missing");
        assert!(matches!(err, UpdateError::Verification(_)));
    }

    #[test]
    fn verifies_rsa_signatures() {
        let private_key = RsaPrivateKey::from_pkcs8_pem(TEST_RSA_PRIVATE_KEY).expect("rsa key");
        let public_key =
            parse_signature_key(TEST_RSA_PUBLIC_KEY.as_bytes()).expect("rsa public key");
        let payload = b"rsa-payload";
        let signer = rsa::pkcs1v15::SigningKey::<Sha256>::new(private_key);
        let signature = signer.sign(payload);

        verify_signature(payload, &signature.to_bytes(), &public_key).expect("rsa valid");
    }
}
