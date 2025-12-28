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
    #[serde(default)]
    preferences: Preferences,
    #[serde(default)]
    logging: LoggingOptions,
    #[serde(default)]
    telemetry_prompt_shown: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecentRepo {
    pub path: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ThemeMode {
    #[serde(alias = "Light")]
    Latte,
    Frappe,
    Macchiato,
    #[serde(alias = "Dark")]
    Mocha,
}

impl Default for ThemeMode {
    fn default() -> Self {
        Self::Mocha
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Keybinding {
    pub action: String,
    pub binding: String,
}

impl Default for Keybinding {
    fn default() -> Self {
        Self {
            action: "Open settings".to_string(),
            binding: "Ctrl+,".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkOptions {
    #[serde(default = "default_network_timeout")]
    pub network_timeout_secs: u64,
    #[serde(default)]
    pub http_proxy: String,
    #[serde(default)]
    pub https_proxy: String,
    #[serde(default = "default_use_https")]
    pub use_https: bool,
    #[serde(default = "default_allow_ssh")]
    pub allow_ssh: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoggingOptions {
    #[serde(default = "default_log_retention_files")]
    retention_files: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReleaseChannel {
    Stable,
    Preview,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MotionIntensity {
    Low,
    Medium,
    High,
}

impl Default for ReleaseChannel {
    fn default() -> Self {
        Self::Stable
    }
}

impl Default for MotionIntensity {
    fn default() -> Self {
        Self::Medium
    }
}

impl Default for NetworkOptions {
    fn default() -> Self {
        Self {
            network_timeout_secs: default_network_timeout(),
            http_proxy: String::new(),
            https_proxy: String::new(),
            use_https: default_use_https(),
            allow_ssh: default_allow_ssh(),
        }
    }
}

impl Default for LoggingOptions {
    fn default() -> Self {
        Self {
            retention_files: default_log_retention_files(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Preferences {
    #[serde(default)]
    theme: ThemeMode,
    #[serde(default = "default_clone_path")]
    default_clone_path: String,
    #[serde(default = "default_keybindings")]
    keybindings: Vec<Keybinding>,
    #[serde(default)]
    network: NetworkOptions,
    #[serde(default = "default_auto_check_updates")]
    auto_check_updates: bool,
    #[serde(default)]
    release_channel: ReleaseChannel,
    #[serde(default)]
    update_feed_override: Option<String>,
    #[serde(default)]
    telemetry_enabled: bool,
    #[serde(default)]
    allow_encrypted_tokens: bool,
    #[serde(default = "default_control_height")]
    control_height: f32,
    #[serde(default = "default_branch_box_height")]
    branch_box_height: f32,
    #[serde(default)]
    pinned_branches: Vec<String>,
    #[serde(default)]
    reduced_motion: bool,
    #[serde(default = "default_motion_intensity")]
    motion_intensity: MotionIntensity,
    #[serde(default)]
    performance_mode: bool,
    #[serde(default = "default_auto_fetch_enabled")]
    auto_fetch_enabled: bool,
    #[serde(default = "default_auto_fetch_interval_minutes")]
    auto_fetch_interval_minutes: u64,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: ThemeMode::Mocha,
            default_clone_path: default_clone_path(),
            keybindings: default_keybindings(),
            network: NetworkOptions::default(),
            auto_check_updates: default_auto_check_updates(),
            release_channel: ReleaseChannel::default(),
            update_feed_override: None,
            telemetry_enabled: false,
            allow_encrypted_tokens: false,
            control_height: default_control_height(),
            branch_box_height: default_branch_box_height(),
            pinned_branches: Vec::new(),
            reduced_motion: false,
            motion_intensity: default_motion_intensity(),
            performance_mode: false,
            auto_fetch_enabled: default_auto_fetch_enabled(),
            auto_fetch_interval_minutes: default_auto_fetch_interval_minutes(),
        }
    }
}

impl AppConfig {
    pub fn load() -> Self {
        let path = config_path();
        if let Ok(contents) = fs::read_to_string(&path)
            && let Ok(config) = serde_json::from_str::<Self>(&contents)
        {
            return config;
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

    pub fn preferences(&self) -> &Preferences {
        &self.preferences
    }

    pub fn set_preferences(&mut self, preferences: Preferences) {
        self.preferences = preferences;
    }

    pub fn logging(&self) -> &LoggingOptions {
        &self.logging
    }

    pub fn telemetry_prompt_shown(&self) -> bool {
        self.telemetry_prompt_shown
    }

    pub fn mark_telemetry_prompt_shown(&mut self) {
        self.telemetry_prompt_shown = true;
    }
}

fn config_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join(APP_CONFIG_DIR).join(CONFIG_FILE_NAME)
}

pub fn app_data_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_CONFIG_DIR)
}

fn default_clone_path() -> String {
    dirs::home_dir()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
        .display()
        .to_string()
}

fn default_keybindings() -> Vec<Keybinding> {
    vec![
        Keybinding {
            action: "Clone repository".to_string(),
            binding: "Ctrl+Shift+C".to_string(),
        },
        Keybinding {
            action: "Open recent".to_string(),
            binding: "Ctrl+O".to_string(),
        },
        Keybinding {
            action: "Stage changes".to_string(),
            binding: "Ctrl+S".to_string(),
        },
        Keybinding::default(),
    ]
}

fn default_network_timeout() -> u64 {
    30
}

fn default_control_height() -> f32 {
    28.0
}

pub const MIN_BRANCH_BOX_HEIGHT: f32 = 72.0;

fn default_branch_box_height() -> f32 {
    92.0
}

fn default_use_https() -> bool {
    true
}

fn default_allow_ssh() -> bool {
    true
}

fn default_auto_check_updates() -> bool {
    true
}

fn default_motion_intensity() -> MotionIntensity {
    MotionIntensity::Medium
}

fn default_log_retention_files() -> usize {
    7
}

fn default_auto_fetch_enabled() -> bool {
    false
}

fn default_auto_fetch_interval_minutes() -> u64 {
    5
}

impl Preferences {
    pub fn theme_mode(&self) -> ThemeMode {
        self.theme
    }

    pub fn set_theme_mode(&mut self, mode: ThemeMode) {
        self.theme = mode;
    }

    pub fn default_clone_path(&self) -> &str {
        &self.default_clone_path
    }

    pub fn set_default_clone_path<S: Into<String>>(&mut self, path: S) {
        self.default_clone_path = path.into();
    }

    pub fn default_clone_path_mut(&mut self) -> &mut String {
        &mut self.default_clone_path
    }

    pub fn keybindings_mut(&mut self) -> &mut Vec<Keybinding> {
        &mut self.keybindings
    }

    pub fn network_mut(&mut self) -> &mut NetworkOptions {
        &mut self.network
    }

    pub fn network(&self) -> &NetworkOptions {
        &self.network
    }

    pub fn auto_check_updates(&self) -> bool {
        self.auto_check_updates
    }

    pub fn set_auto_check_updates(&mut self, enabled: bool) {
        self.auto_check_updates = enabled;
    }

    pub fn release_channel(&self) -> ReleaseChannel {
        self.release_channel
    }

    pub fn set_release_channel(&mut self, channel: ReleaseChannel) {
        self.release_channel = channel;
    }

    pub fn update_feed_override(&self) -> Option<&str> {
        self.update_feed_override.as_deref()
    }

    pub fn telemetry_enabled(&self) -> bool {
        self.telemetry_enabled
    }

    pub fn set_telemetry_enabled(&mut self, enabled: bool) {
        self.telemetry_enabled = enabled;
    }

    pub fn allow_encrypted_tokens(&self) -> bool {
        self.allow_encrypted_tokens
    }

    pub fn set_allow_encrypted_tokens(&mut self, allowed: bool) {
        self.allow_encrypted_tokens = allowed;
    }

    pub fn control_height(&self) -> f32 {
        self.control_height
    }

    pub fn set_control_height(&mut self, height: f32) {
        self.control_height = height.clamp(20.0, 48.0);
    }

    pub fn branch_box_height(&self) -> f32 {
        self.branch_box_height
    }

    pub fn set_branch_box_height(&mut self, height: f32) {
        self.branch_box_height = height.max(MIN_BRANCH_BOX_HEIGHT);
    }

    pub fn pinned_branches(&self) -> &[String] {
        &self.pinned_branches
    }

    pub fn set_pinned_branches(&mut self, branches: Vec<String>) {
        self.pinned_branches = branches;
    }

    pub fn reduced_motion(&self) -> bool {
        self.reduced_motion
    }

    pub fn set_reduced_motion(&mut self, reduced_motion: bool) {
        self.reduced_motion = reduced_motion;
    }

    pub fn motion_intensity(&self) -> MotionIntensity {
        self.motion_intensity
    }

    pub fn set_motion_intensity(&mut self, motion_intensity: MotionIntensity) {
        self.motion_intensity = motion_intensity;
    }

    pub fn performance_mode(&self) -> bool {
        self.performance_mode
    }

    pub fn set_performance_mode(&mut self, performance_mode: bool) {
        self.performance_mode = performance_mode;
    }

    pub fn auto_fetch_enabled(&self) -> bool {
        self.auto_fetch_enabled
    }

    pub fn set_auto_fetch_enabled(&mut self, auto_fetch_enabled: bool) {
        self.auto_fetch_enabled = auto_fetch_enabled;
    }

    pub fn auto_fetch_interval_minutes(&self) -> u64 {
        self.auto_fetch_interval_minutes
    }

    pub fn set_auto_fetch_interval_minutes(&mut self, minutes: u64) {
        self.auto_fetch_interval_minutes = minutes.max(1);
    }

    pub fn save_to_path<P: AsRef<Path>>(&self, path: P) -> std::io::Result<()> {
        let contents = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string());
        if let Some(parent) = path.as_ref().parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, contents)
    }

    pub fn from_path<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        let contents = fs::read_to_string(path)?;
        serde_json::from_str(&contents).map_err(|err| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to parse preferences: {err}"),
            )
        })
    }
}

impl LoggingOptions {
    pub fn retention_files(&self) -> usize {
        self.retention_files
    }
}
