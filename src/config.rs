use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const MAX_RECENT: usize = 15;
const CONFIG_FILE_NAME: &str = "config.json";
const APP_CONFIG_DIR: &str = "gitspace";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    recent_repos: Vec<RecentRepo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecentRepo {
    pub path: String,
}

impl AppConfig {
    pub fn load() -> Self {
        let path = config_path();
        if let Ok(contents) = fs::read_to_string(&path) {
            if let Ok(config) = serde_json::from_str::<Self>(&contents) {
                return config;
            }
        }
        Self::default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string());
        fs::write(path, content)
    }

    pub fn touch_recent<P: AsRef<Path>>(&mut self, path: P) -> bool {
        let normalized = path.as_ref().to_string_lossy().to_string();
        if self
            .recent_repos
            .first()
            .map(|entry| entry.path == normalized)
            .unwrap_or(false)
        {
            return false;
        }

        self.recent_repos.retain(|entry| entry.path != normalized);
        self.recent_repos.insert(0, RecentRepo { path: normalized });
        if self.recent_repos.len() > MAX_RECENT {
            self.recent_repos.truncate(MAX_RECENT);
        }
        true
    }

    pub fn recent_repos(&self) -> &[RecentRepo] {
        &self.recent_repos
    }
}

fn config_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join(APP_CONFIG_DIR).join(CONFIG_FILE_NAME)
}
