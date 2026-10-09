use std::fs;
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use tracing::{error, warn};

const MAX_RECENT: usize = 15;
const CONFIG_FILE_NAME: &str = "config.json";
const APP_CONFIG_DIR: &str = "gitspace";
pub const MIN_LOG_RETENTION_FILES: usize = 1;
pub const MAX_LOG_RETENTION_FILES: usize = 30;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    recent_repos: Vec<RecentRepo>,
    #[serde(default)]
    preferences: Preferences,
    #[serde(default)]
    logging: LoggingOptions,
    /// Set when the file on disk could not be read or backed up. Saving would then replace
    /// settings that were never loaded, so `save` refuses to write.
    #[serde(skip)]
    keep_file_on_disk: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecentRepo {
    pub path: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum ThemeMode {
    #[serde(alias = "Light")]
    Latte,
    Frappe,
    Macchiato,
    #[serde(alias = "Dark")]
    #[default]
    Mocha,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum MotionIntensity {
    Low,
    #[default]
    Medium,
    High,
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
    #[serde(default, deserialize_with = "default_on_error")]
    theme: ThemeMode,
    #[serde(default = "default_clone_path")]
    default_clone_path: String,
    #[serde(default = "default_keybindings")]
    keybindings: Vec<Keybinding>,
    #[serde(default)]
    network: NetworkOptions,
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
    #[serde(
        default = "default_motion_intensity",
        deserialize_with = "default_on_error"
    )]
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
        Self::load_from(&config_path())
    }

    /// Reads the config without ever losing it. An invalid theme or motion value falls back to
    /// its default on its own. A file that still cannot be parsed is copied to
    /// `config.json.bak` before the defaults are used, so the next save cannot destroy it.
    fn load_from(path: &Path) -> Self {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == ErrorKind::NotFound => return Self::default(),
            Err(err) => {
                error!(
                    target: "gitspace::config",
                    path = %path.display(),
                    error = %err,
                    "failed to read config; using defaults without saving over it"
                );
                return Self {
                    keep_file_on_disk: true,
                    ..Self::default()
                };
            }
        };
        let parse_error = match serde_json::from_slice(&bytes) {
            Ok(config) => return config,
            Err(err) => err,
        };
        // Keep the unreadable file before anything can overwrite it. If even that fails, never
        // save over it.
        let backup = backup_path(path);
        let backup_result = write_atomic(&backup, &bytes);
        error!(
            target: "gitspace::config",
            path = %path.display(),
            error = %parse_error,
            backup = %backup.display(),
            backup_error = ?backup_result.as_ref().err(),
            "failed to parse config; using defaults"
        );
        Self {
            keep_file_on_disk: backup_result.is_err(),
            ..Self::default()
        }
    }

    pub fn save(&self) -> io::Result<()> {
        self.save_to(&config_path())
    }

    fn save_to(&self, path: &Path) -> io::Result<()> {
        if self.keep_file_on_disk {
            return Err(io::Error::other(format!(
                "{} could not be read at startup; not overwriting it",
                path.display()
            )));
        }
        let content = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        write_atomic(path, &content)
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

    pub fn set_logging(&mut self, logging: LoggingOptions) {
        self.logging = logging;
    }
}

fn config_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join(APP_CONFIG_DIR).join(CONFIG_FILE_NAME)
}

fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".bak");
    path.with_file_name(name)
}

/// Writes `contents` to a temporary sibling file, syncs it, then renames it over `path`, so a
/// crash or a full disk never leaves a truncated file behind. The replaced file keeps its
/// permissions.
pub(crate) fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    // Write through a symlink (dotfile managers) instead of replacing the link itself.
    let resolved;
    let path = if fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        resolved = fs::canonicalize(path)?;
        resolved.as_path()
    } else {
        path
    };
    let parent = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "path has no file name"))?;
    let tmp = parent.join(format!(
        ".{}.{:08x}.tmp",
        name.to_string_lossy(),
        rand::random::<u32>()
    ));
    let result = write_then_rename(&tmp, path, contents);
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn write_then_rename(tmp: &Path, path: &Path, contents: &[u8]) -> io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(tmp)?;
    // Keep the target's mode bits. Unix only: on Windows the only bit is read-only, which
    // would make both the rename and the cleanup of the temporary file fail.
    #[cfg(unix)]
    {
        if let Ok(metadata) = fs::metadata(path) {
            file.set_permissions(metadata.permissions())?;
        }
    }
    file.write_all(contents)?;
    file.sync_all()?;
    drop(file);
    fs::rename(tmp, path)
}

/// Deserializes a field and falls back to its default when the stored value is invalid, for
/// example a theme name written by another release, so one bad value does not discard the
/// whole config.
fn default_on_error<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned + Default,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_else(|err| {
        warn!(
            target: "gitspace::config",
            error = %err,
            "ignoring an invalid config value; using the default"
        );
        T::default()
    }))
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

    /// Sets the default clone path.
    ///
    /// The path is validated to ensure it's not empty and is a valid directory path.
    /// Relative paths are expanded to absolute paths using the home directory.
    pub fn set_default_clone_path<S: Into<String>>(&mut self, path: S) {
        let path = path.into();

        // Skip empty paths
        if path.trim().is_empty() {
            return;
        }

        // Expand home directory if path starts with ~
        let expanded = match (path.strip_prefix("~/"), dirs::home_dir()) {
            (Some(rest), Some(home)) => home.join(rest).to_string_lossy().to_string(),
            _ => path,
        };

        self.default_clone_path = expanded;
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

    pub fn set_retention_files(&mut self, retention_files: usize) {
        self.retention_files =
            retention_files.clamp(MIN_LOG_RETENTION_FILES, MAX_LOG_RETENTION_FILES);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_enum_values_keep_the_rest_of_the_config() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(CONFIG_FILE_NAME);
        fs::write(
            &path,
            r#"{
                "recent_repos": [{ "path": "/work/repo" }],
                "preferences": {
                    "theme": "GruvboxDark",
                    "motion_intensity": 3,
                    "control_height": 32.0,
                    "pinned_branches": ["main"]
                },
                "logging": { "retention_files": 12 }
            }"#,
        )
        .expect("write config");

        let config = AppConfig::load_from(&path);
        assert_eq!(config.recent_repos()[0].path, "/work/repo");
        let preferences = config.preferences();
        assert_eq!(preferences.theme_mode(), ThemeMode::Mocha);
        assert_eq!(preferences.motion_intensity(), MotionIntensity::Medium);
        assert_eq!(preferences.control_height(), 32.0);
        assert_eq!(preferences.pinned_branches(), ["main"]);
        assert_eq!(config.logging().retention_files(), 12);
        assert!(!backup_path(&path).exists());
    }

    #[test]
    fn unparseable_config_is_backed_up_before_saving() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(CONFIG_FILE_NAME);
        let corrupt = r#"{ "recent_repos": [{ "path": "/work/repo" }"#;
        fs::write(&path, corrupt).expect("write config");

        let mut config = AppConfig::load_from(&path);
        assert!(config.recent_repos().is_empty());
        let backup = backup_path(&path);
        assert_eq!(backup.file_name().unwrap(), "config.json.bak");
        assert_eq!(fs::read_to_string(&backup).expect("read backup"), corrupt);

        config.touch_recent("/work/other");
        config.save_to(&path).expect("save");
        assert_eq!(fs::read_to_string(&backup).expect("read backup"), corrupt);
        let reloaded = AppConfig::load_from(&path);
        assert_eq!(reloaded.recent_repos()[0].path, "/work/other");
    }

    #[test]
    fn config_that_cannot_be_read_is_never_saved_over() {
        let dir = tempfile::tempdir().expect("tempdir");
        // A directory in place of the file: it exists but reading it fails.
        let path = dir.path().join(CONFIG_FILE_NAME);
        fs::create_dir(&path).expect("create dir");

        let config = AppConfig::load_from(&path);
        assert!(config.save_to(&path).is_err());
        assert!(path.is_dir());
    }

    #[test]
    fn save_replaces_the_file_without_leaving_temporary_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nested").join(CONFIG_FILE_NAME);
        let mut config = AppConfig::default();
        config.touch_recent("/work/repo");
        config.save_to(&path).expect("first save");
        config.touch_recent("/work/other");
        config.save_to(&path).expect("second save");

        assert_eq!(AppConfig::load_from(&path).recent_repos().len(), 2);
        let entries: Vec<_> = fs::read_dir(path.parent().unwrap())
            .expect("read dir")
            .map(|entry| entry.expect("entry").file_name())
            .collect();
        assert_eq!(entries, [CONFIG_FILE_NAME]);
    }

    #[cfg(unix)]
    #[test]
    fn save_writes_through_a_symlinked_config() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("dotfiles").join(CONFIG_FILE_NAME);
        fs::create_dir_all(target.parent().unwrap()).expect("create dir");
        fs::write(&target, "{}").expect("write target");
        let link = dir.path().join(CONFIG_FILE_NAME);
        std::os::unix::fs::symlink(&target, &link).expect("symlink");

        let mut config = AppConfig::load_from(&link);
        config.touch_recent("/work/repo");
        config.save_to(&link).expect("save");

        assert!(fs::symlink_metadata(&link)
            .expect("link metadata")
            .file_type()
            .is_symlink());
        assert_eq!(AppConfig::load_from(&target).recent_repos().len(), 1);
    }
}
