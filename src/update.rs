use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::Proxy;
use reqwest::blocking::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::config::{NetworkOptions, ReleaseChannel};

const DEFAULT_RELEASE_FEED: &str = "https://api.github.com/repos/gitspace-app/GitSpace/releases";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub name: String,
    pub download_url: String,
    pub checksum: Option<String>,
    pub signature_url: Option<String>,
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

        return Ok(());
    }

    Err(UpdateError::Verification(format!(
        "Asset {} is missing verification metadata",
        asset.name
    )))
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
    let mut checksums: HashMap<String, String> = HashMap::new();
    let mut signatures: HashMap<String, String> = HashMap::new();

    for asset in &release.assets {
        if let Some(target) = asset.name.strip_suffix(".sha256") {
            let checksum = fetch_checksum(client, &asset.browser_download_url, network)?;
            checksums.insert(target.to_string(), checksum);
        } else if let Some(target) = asset.name.strip_suffix(".sig") {
            signatures.insert(target.to_string(), asset.browser_download_url.clone());
        }
    }

    let mut verified = Vec::new();
    for asset in &release.assets {
        if asset.name.ends_with(".sha256") || asset.name.ends_with(".sig") {
            continue;
        }

        let checksum = checksums.get(&asset.name).cloned();
        let signature_url = signatures.get(&asset.name).cloned();

        if checksum.is_some() || signature_url.is_some() {
            verified.push(ReleaseAsset {
                name: asset.name.clone(),
                download_url: asset.browser_download_url.clone(),
                checksum,
                signature_url,
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

    if parsed.len() < 64 {
        return Err(UpdateError::Verification(
            "Checksum entry is too short to be valid SHA-256.".to_string(),
        ));
    }

    Ok(parsed.to_string())
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
