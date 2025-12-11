use reqwest::blocking::Client;
use serde::Deserialize;

use crate::config::ReleaseChannel;

const DEFAULT_RELEASE_FEED: &str = "https://api.github.com/repos/gitspace-app/GitSpace/releases";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub version: String,
    pub url: String,
    pub notes: Option<String>,
    pub channel: ReleaseChannel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateError(String);

impl std::fmt::Display for UpdateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for UpdateError {}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    prerelease: bool,
    body: Option<String>,
}

pub type UpdateResult = Result<Option<ReleaseInfo>, UpdateError>;

pub fn check_for_updates(channel: ReleaseChannel, feed_override: Option<&str>) -> UpdateResult {
    let url = feed_override.unwrap_or(DEFAULT_RELEASE_FEED);

    let client = Client::builder()
        .user_agent("GitSpace-Updater/0.1")
        .build()
        .map_err(|err| UpdateError(format!("Failed to build HTTP client: {err}")))?;

    let response = client
        .get(url)
        .send()
        .map_err(|err| UpdateError(format!("Failed to reach release feed: {err}")))?
        .error_for_status()
        .map_err(|err| UpdateError(format!("Release feed returned an error: {err}")))?;

    let releases: Vec<GitHubRelease> = response
        .json()
        .map_err(|err| UpdateError(format!("Failed to parse release feed: {err}")))?;

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

    Ok(Some(ReleaseInfo {
        version: normalized_tag.to_string(),
        url: release.html_url.clone(),
        notes: release.body.clone(),
        channel,
    }))
}
