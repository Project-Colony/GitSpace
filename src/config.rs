use std::fs;
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};

use colony_ui::i18n::Locale;
use colony_ui::preferences::StandardPreferences;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use tracing::{error, warn};

const CONFIG_FILE_NAME: &str = "config.json";
pub const MIN_LOG_RETENTION_FILES: usize = 1;
pub const MAX_LOG_RETENTION_FILES: usize = 30;
/// Auto-fetch interval bounds, in minutes. Zero would make `iced::time::every` panic, and the
/// upper bound of one day keeps the conversion to seconds far from overflowing.
const MIN_AUTO_FETCH_INTERVAL_MINUTES: u64 = 1;
const MAX_AUTO_FETCH_INTERVAL_MINUTES: u64 = 24 * 60;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    repos: Vec<RepoEntry>,
    #[serde(default)]
    preferences: Preferences,
    #[serde(default)]
    logging: LoggingOptions,
    /// Set when the file on disk could not be read or backed up. Saving would then replace
    /// settings that were never loaded, so `save` refuses to write.
    #[serde(skip)]
    keep_file_on_disk: bool,
}

/// A repository in the list: opened or cloned by the user, or found in the clone folder.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoEntry {
    pub path: String,
    #[serde(default)]
    pub pinned: bool,
    /// Seconds since the Unix epoch; `None` for a found repository never opened.
    #[serde(default)]
    pub last_opened: Option<i64>,
    /// Found by the clone-folder scan rather than opened by the user.
    #[serde(default)]
    pub discovered: bool,
    /// A found repository the user removed: kept so the next scan does not add it back.
    #[serde(default)]
    pub hidden: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    System,
    English,
    French,
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

impl Language {
    pub fn locale(self) -> Locale {
        match self {
            Language::English => Locale::En,
            Language::French => Locale::Fr,
            // LC_ALL overrides LANG; neither set (as on Windows) means English.
            Language::System => ["LC_ALL", "LANG"]
                .iter()
                .find_map(|name| std::env::var(name).ok().filter(|tag| !tag.is_empty()))
                .map_or(Locale::En, |tag| Locale::from_tag(&tag)),
        }
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
    /// Appearance and accessibility, stored flat next to GitSpace's own keys.
    #[serde(flatten)]
    pub standard: StandardPreferences,
    #[serde(default, deserialize_with = "default_on_error")]
    language: Language,
    #[serde(default = "default_true")]
    reopen_last_repo: bool,
    #[serde(default = "default_true")]
    scan_clone_folder: bool,
    #[serde(default = "default_clone_path")]
    default_clone_path: String,
    #[serde(default)]
    network: NetworkOptions,
    #[serde(default)]
    allow_encrypted_tokens: bool,
    #[serde(default)]
    pinned_branches: Vec<String>,
    #[serde(default = "default_auto_fetch_enabled")]
    auto_fetch_enabled: bool,
    #[serde(default = "default_auto_fetch_interval_minutes")]
    auto_fetch_interval_minutes: u64,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            standard: StandardPreferences::default(),
            language: Language::default(),
            reopen_last_repo: true,
            scan_clone_folder: true,
            default_clone_path: default_clone_path(),
            network: NetworkOptions::default(),
            allow_encrypted_tokens: false,
            pinned_branches: Vec::new(),
            auto_fetch_enabled: default_auto_fetch_enabled(),
            auto_fetch_interval_minutes: default_auto_fetch_interval_minutes(),
        }
    }
}

impl AppConfig {
    pub fn load() -> Self {
        Self::load_from(&config_path())
    }

    /// Reads the config without ever losing it. Values written by older builds are carried
    /// over, and an invalid language value falls back to its default on its own. A file that
    /// still cannot be parsed is copied to `config.json.bak` before the defaults are used, so
    /// the next save cannot destroy it.
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
        let (parse_error, value) = match serde_json::from_slice::<Value>(&bytes) {
            Ok(mut value) => {
                migrate_legacy(&mut value, chrono::Utc::now().timestamp());
                match Self::deserialize(&value) {
                    Ok(config) => return config,
                    Err(err) => (err, Some(value)),
                }
            }
            Err(err) => (err, None),
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
            "failed to parse config; keeping the values that are valid"
        );
        let config = value.and_then(|value| lenient(&value)).unwrap_or_default();
        Self {
            keep_file_on_disk: backup_result.is_err(),
            ..config
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

    /// Every listed repository, the last opened first, hidden ones included.
    pub fn repos(&self) -> &[RepoEntry] {
        &self.repos
    }

    /// Records that `path` was opened at `now` (Unix seconds): adds it if needed, moves it to
    /// the front and shows it again if it was hidden. Returns whether anything changed.
    pub fn open_repo(&mut self, path: &str, now: i64) -> bool {
        let index = self.repos.iter().position(|entry| entry.path == path);
        if index == Some(0) && self.repos[0].last_opened == Some(now) && !self.repos[0].hidden {
            return false;
        }
        let mut entry = index.map_or_else(
            || RepoEntry {
                path: path.to_string(),
                ..RepoEntry::default()
            },
            |index| self.repos.remove(index),
        );
        entry.last_opened = Some(now);
        entry.hidden = false;
        self.repos.insert(0, entry);
        true
    }

    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the repository sidebar will call it")
    )]
    pub fn set_pinned(&mut self, path: &str, pinned: bool) {
        if let Some(entry) = self.repos.iter_mut().find(|entry| entry.path == path) {
            entry.pinned = pinned;
        }
    }

    /// Removes `path` from the list. A found repository is only hidden, so the next scan of
    /// the clone folder does not add it back.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the repository sidebar will call it")
    )]
    pub fn hide_repo(&mut self, path: &str) {
        let Some(index) = self.repos.iter().position(|entry| entry.path == path) else {
            return;
        };
        if self.repos[index].discovered {
            self.repos[index].hidden = true;
        } else {
            self.repos.remove(index);
        }
    }

    /// Adds the repositories a scan found that are not listed yet, hidden ones included.
    /// Returns whether any was added.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the clone-folder scan will call it")
    )]
    pub fn merge_discovered(&mut self, paths: &[String]) -> bool {
        let before = self.repos.len();
        for path in paths {
            if !self.repos.iter().any(|entry| &entry.path == path) {
                self.repos.push(RepoEntry {
                    path: path.clone(),
                    discovered: true,
                    ..RepoEntry::default()
                });
            }
        }
        self.repos.len() != before
    }

    /// The visible repository opened most recently; on a tie, the one nearer the front.
    pub fn last_repo(&self) -> Option<&RepoEntry> {
        self.repos
            .iter()
            .rev()
            .filter(|entry| !entry.hidden && entry.last_opened.is_some())
            .max_by_key(|entry| entry.last_opened)
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

/// Rewrites what older builds saved into today's fields: the Catppuccin `theme` and the
/// capped `recent_repos` list, whose order becomes last-opened times counting back from `now`.
fn migrate_legacy(config: &mut Value, now: i64) {
    if let Some(preferences) = config.get_mut("preferences") {
        migrate_legacy_preferences(preferences);
    }
    let Some(config) = config.as_object_mut() else {
        return;
    };
    let Some(recent) = config.remove("recent_repos") else {
        return;
    };
    if config.contains_key("repos") {
        return;
    }
    let repos = recent
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("path")?.as_str())
        .zip(0..)
        .map(|(path, age)| serde_json::json!({ "path": path, "last_opened": now - age }))
        .collect();
    config.insert("repos".to_string(), Value::Array(repos));
}

fn migrate_legacy_preferences(preferences: &mut Value) {
    let Some(preferences) = preferences.as_object_mut() else {
        return;
    };
    let Some(theme) = preferences.remove("theme") else {
        return;
    };
    if preferences.contains_key("selected_theme") {
        return;
    }
    let variant = match theme.as_str() {
        Some("Latte" | "Light") => "latte",
        Some("Frappe") => "frappe",
        Some("Macchiato") => "macchiato",
        Some("Mocha" | "Dark") => "mocha",
        _ => return,
    };
    preferences.insert("selected_theme".to_string(), "catppuccin".into());
    preferences.insert("selected_variant".to_string(), variant.into());
}

fn config_path() -> PathBuf {
    crate::paths::effective_dirs().0.join(CONFIG_FILE_NAME)
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
    write_atomic_with(path, contents, false)
}

/// [`write_atomic`] for keys and tokens: on Unix the file is owner-only (0600) from the moment
/// it is created, whatever the mode of the file it replaces.
pub(crate) fn write_private(path: &Path, contents: &[u8]) -> io::Result<()> {
    write_atomic_with(path, contents, true)
}

fn write_atomic_with(path: &Path, contents: &[u8], private: bool) -> io::Result<()> {
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
    let result = write_then_rename(&tmp, path, contents, private);
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn write_then_rename(tmp: &Path, path: &Path, contents: &[u8], private: bool) -> io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(tmp)?;
    // Keep the target's mode bits. Unix only: on Windows the only bit is read-only, which
    // would make both the rename and the cleanup of the temporary file fail.
    #[cfg(unix)]
    if !private {
        if let Ok(metadata) = fs::metadata(path) {
            file.set_permissions(metadata.permissions())?;
        }
    }
    #[cfg(not(unix))]
    let _ = private;
    file.write_all(contents)?;
    file.sync_all()?;
    drop(file);
    fs::rename(tmp, path)
}

/// Deserializes a field and falls back to its default when the stored value is invalid, for
/// example a theme name written by another release, so one bad value does not discard the
/// whole config.
/// Builds a `T` from `user`, keeping each value that is valid and the default for the rest.
///
/// Values are taken one key at a time, going into nested objects when a whole object is
/// refused, so one wrong-typed field resets only that field.
fn lenient<T: DeserializeOwned + Serialize + Default>(user: &serde_json::Value) -> Option<T> {
    fn overlay(
        root: &mut serde_json::Value,
        pointer: &str,
        user: &serde_json::Map<String, serde_json::Value>,
        valid: &dyn Fn(&serde_json::Value) -> bool,
    ) {
        for (key, value) in user {
            let mut trial = root.clone();
            if let Some(object) = trial.pointer_mut(pointer).and_then(|v| v.as_object_mut()) {
                object.insert(key.clone(), value.clone());
            }
            if valid(&trial) {
                *root = trial;
                continue;
            }
            let child = format!("{pointer}/{}", key.replace('~', "~0").replace('/', "~1"));
            match (value, root.pointer(&child)) {
                (serde_json::Value::Object(nested), Some(serde_json::Value::Object(_))) => {
                    overlay(root, &child, nested, valid)
                }
                _ => {
                    warn!(target: "gitspace::config", key = %child, "ignoring an invalid config value; using the default")
                }
            }
        }
    }

    let user = user.as_object()?;
    let mut root = serde_json::to_value(T::default()).ok()?;
    let valid = |value: &serde_json::Value| serde_json::from_value::<T>(value.clone()).is_ok();
    overlay(&mut root, "", user, &valid);
    serde_json::from_value(root).ok()
}

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

fn default_true() -> bool {
    true
}

fn default_network_timeout() -> u64 {
    30
}

fn default_use_https() -> bool {
    true
}

fn default_allow_ssh() -> bool {
    true
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

fn clamp_auto_fetch_interval(minutes: u64) -> u64 {
    minutes.clamp(
        MIN_AUTO_FETCH_INTERVAL_MINUTES,
        MAX_AUTO_FETCH_INTERVAL_MINUTES,
    )
}

impl Preferences {
    pub fn language(&self) -> Language {
        self.language
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

    pub fn pinned_branches(&self) -> &[String] {
        &self.pinned_branches
    }

    pub fn set_pinned_branches(&mut self, branches: Vec<String>) {
        self.pinned_branches = branches;
    }

    pub fn auto_fetch_enabled(&self) -> bool {
        self.auto_fetch_enabled
    }

    pub fn set_auto_fetch_enabled(&mut self, auto_fetch_enabled: bool) {
        self.auto_fetch_enabled = auto_fetch_enabled;
    }

    /// Clamped on read too: config.json and imported files are not checked when loaded.
    pub fn auto_fetch_interval_minutes(&self) -> u64 {
        clamp_auto_fetch_interval(self.auto_fetch_interval_minutes)
    }

    pub fn set_auto_fetch_interval_minutes(&mut self, minutes: u64) {
        self.auto_fetch_interval_minutes = clamp_auto_fetch_interval(minutes);
    }

    pub fn save_to_path<P: AsRef<Path>>(&self, path: P) -> std::io::Result<()> {
        let contents = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        write_atomic(path.as_ref(), &contents)
    }

    /// Reads preferences saved by [`Self::save_to_path`], or the `preferences` object of a
    /// whole `config.json`. A file with none of the preference fields is refused: read as is,
    /// it would silently reset every preference to its default.
    pub fn from_path<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        let invalid = |message: String| io::Error::new(ErrorKind::InvalidData, message);
        let contents = fs::read_to_string(path)?;
        let mut value: serde_json::Value = serde_json::from_str(&contents)
            .map_err(|err| invalid(format!("Failed to parse preferences: {err}")))?;
        if let Some(preferences) = value.get_mut("preferences").map(Value::take) {
            value = preferences;
        }
        migrate_legacy_preferences(&mut value);
        let known = serde_json::to_value(Self::default()).map_err(io::Error::other)?;
        let has_preferences = match (&value, &known) {
            (serde_json::Value::Object(fields), serde_json::Value::Object(known)) => {
                fields.keys().any(|key| known.contains_key(key))
            }
            _ => false,
        };
        if !has_preferences {
            return Err(invalid(
                "The file does not contain GitSpace preferences".to_string(),
            ));
        }
        serde_json::from_value(value)
            .map_err(|err| invalid(format!("Failed to parse preferences: {err}")))
    }
}

impl LoggingOptions {
    /// Clamped on read too: config.json is not checked when loaded.
    pub fn retention_files(&self) -> usize {
        self.retention_files
            .clamp(MIN_LOG_RETENTION_FILES, MAX_LOG_RETENTION_FILES)
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
    fn legacy_config_keeps_every_value() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(CONFIG_FILE_NAME);
        fs::write(
            &path,
            r#"{
                "preferences": {
                    "theme": "Latte",
                    "default_clone_path": "/x",
                    "pinned_branches": ["main"],
                    "network": {"http_proxy": "http://p:1"},
                    "auto_fetch_interval_minutes": 15
                },
                "recent_repos": [{"path": "/a"}, {"path": "/b"}]
            }"#,
        )
        .expect("write config");

        let config = AppConfig::load_from(&path);
        let preferences = config.preferences();
        assert_eq!(preferences.standard.selected_theme, "catppuccin");
        assert_eq!(preferences.standard.selected_variant, "latte");
        assert_eq!(preferences.default_clone_path(), "/x");
        assert_eq!(preferences.pinned_branches(), ["main"]);
        assert_eq!(preferences.network().http_proxy, "http://p:1");
        assert_eq!(preferences.auto_fetch_interval_minutes(), 15);
        let repos = config.repos();
        let paths: Vec<_> = repos.iter().map(|repo| repo.path.as_str()).collect();
        assert_eq!(paths, ["/a", "/b"]);
        assert!(repos[0].last_opened > repos[1].last_opened);
        assert!(repos.iter().all(|repo| !repo.pinned && !repo.discovered));
        assert_eq!(
            config.last_repo().map(|repo| repo.path.as_str()),
            Some("/a")
        );
        assert!(!backup_path(&path).exists());
    }

    #[test]
    fn open_repo_bumps_and_unhides() {
        let mut config = AppConfig::default();
        assert!(config.open_repo("/a", 10));
        assert!(config.open_repo("/b", 20));
        assert_eq!(
            config.last_repo().map(|repo| repo.path.as_str()),
            Some("/b")
        );
        assert!(config.open_repo("/a", 30));
        assert_eq!(config.repos().len(), 2);
        assert_eq!(config.repos()[0].path, "/a");
        assert_eq!(config.repos()[0].last_opened, Some(30));
        assert_eq!(
            config.last_repo().map(|repo| repo.path.as_str()),
            Some("/a")
        );
        assert!(!config.open_repo("/a", 30), "nothing changed");

        config.merge_discovered(&["/c".to_string()]);
        config.set_pinned("/c", true);
        config.hide_repo("/c");
        assert!(config
            .repos()
            .iter()
            .any(|repo| repo.path == "/c" && repo.hidden));
        assert!(config.open_repo("/c", 40));
        let entry = &config.repos()[0];
        assert_eq!(entry.path, "/c");
        assert!(!entry.hidden && entry.pinned && entry.discovered);
        assert_eq!(entry.last_opened, Some(40));
    }

    #[test]
    fn hide_repo_hides_discovered_and_removes_opened() {
        let mut config = AppConfig::default();
        config.open_repo("/opened", 10);
        config.merge_discovered(&["/found".to_string()]);
        config.hide_repo("/opened");
        config.hide_repo("/found");
        config.hide_repo("/unknown");

        let repos = config.repos();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].path, "/found");
        assert!(repos[0].hidden && repos[0].discovered);
        assert!(config.last_repo().is_none());
    }

    #[test]
    fn merge_discovered_skips_known_and_hidden() {
        let mut config = AppConfig::default();
        config.open_repo("/opened", 10);
        assert!(config.merge_discovered(&["/hidden".to_string()]));
        config.hide_repo("/hidden");

        let found = ["/opened", "/hidden", "/new"].map(String::from);
        assert!(config.merge_discovered(&found));
        assert!(!config.merge_discovered(&found), "second scan adds nothing");
        let repos = config.repos();
        assert_eq!(repos.len(), 3);
        let opened = repos.iter().find(|repo| repo.path == "/opened").unwrap();
        assert!(!opened.discovered);
        let hidden = repos.iter().find(|repo| repo.path == "/hidden").unwrap();
        assert!(hidden.hidden);
        let new = repos.iter().find(|repo| repo.path == "/new").unwrap();
        assert!(new.discovered && !new.hidden && new.last_opened.is_none());
    }

    #[test]
    fn round_trip_writes_standard_preferences_flat() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(CONFIG_FILE_NAME);
        fs::write(
            &path,
            r#"{"preferences": {"theme": "Frappe"}, "recent_repos": [{"path": "/a"}]}"#,
        )
        .expect("write config");
        let mut config = AppConfig::load_from(&path);
        let mut preferences = config.preferences().clone();
        preferences.standard.dyslexia_font = true;
        config.set_preferences(preferences);
        config.save_to(&path).expect("save");

        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).expect("read")).expect("json");
        let saved_preferences = &saved["preferences"];
        assert_eq!(saved_preferences["selected_theme"], "catppuccin");
        assert_eq!(saved_preferences["selected_variant"], "frappe");
        assert_eq!(saved_preferences["dyslexia_font"], true);
        assert_eq!(saved_preferences["language"], "System");
        assert_eq!(saved_preferences["reopen_last_repo"], true);
        assert_eq!(saved_preferences["scan_clone_folder"], true);
        assert!(saved_preferences.get("theme").is_none());
        assert!(saved_preferences.get("standard").is_none());
        assert!(saved.get("recent_repos").is_none());
        assert_eq!(saved["repos"][0]["path"], "/a");

        let reloaded = AppConfig::load_from(&path);
        assert_eq!(reloaded.preferences(), config.preferences());
        assert_eq!(reloaded.repos(), config.repos());
    }

    #[test]
    fn invalid_values_and_removed_settings_keep_the_rest_of_the_config() {
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
        assert_eq!(config.repos()[0].path, "/work/repo");
        let preferences = config.preferences();
        assert_eq!(preferences.standard, StandardPreferences::default());
        assert_eq!(preferences.pinned_branches(), ["main"]);
        assert_eq!(config.logging().retention_files(), 12);
        assert!(!backup_path(&path).exists());
    }

    #[test]
    fn out_of_range_numbers_in_the_file_are_clamped() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(CONFIG_FILE_NAME);
        for (minutes, files, expected_minutes, expected_files) in [
            (0, 0, 1, 1),
            (u64::MAX, usize::MAX, 24 * 60, 30),
            (10, 5, 10, 5),
        ] {
            fs::write(
                &path,
                format!(
                    r#"{{"preferences": {{"auto_fetch_interval_minutes": {minutes}}},
                        "logging": {{"retention_files": {files}}}}}"#
                ),
            )
            .expect("write config");
            let config = AppConfig::load_from(&path);
            let interval = config.preferences().auto_fetch_interval_minutes();
            assert_eq!(interval, expected_minutes);
            assert!(interval.checked_mul(60).is_some());
            assert_eq!(config.logging().retention_files(), expected_files);
        }
    }

    #[test]
    fn preferences_import_reads_an_export_or_a_whole_config() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("exports").join("preferences.json");
        let mut preferences = Preferences::default();
        preferences.standard.selected_theme = "catppuccin".to_string();
        preferences.standard.selected_variant = "latte".to_string();
        preferences.set_pinned_branches(vec!["main".to_string()]);
        preferences.save_to_path(&path).expect("export");
        assert_eq!(Preferences::from_path(&path).expect("import"), preferences);

        // The whole config.json keeps the preferences under "preferences".
        let mut config = AppConfig::default();
        config.set_preferences(preferences.clone());
        let config_path = dir.path().join(CONFIG_FILE_NAME);
        config.save_to(&config_path).expect("save config");
        assert_eq!(
            Preferences::from_path(&config_path).expect("import config"),
            preferences
        );

        for contents in [
            "{}",
            "[]",
            "3",
            r#"{"unrelated": true}"#,
            r#"{"preferences": {}}"#,
        ] {
            fs::write(&path, contents).expect("write");
            let err = Preferences::from_path(&path).expect_err(contents);
            assert_eq!(err.kind(), ErrorKind::InvalidData, "{contents}");
        }
    }

    #[test]
    fn unparseable_config_is_backed_up_before_saving() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(CONFIG_FILE_NAME);
        let corrupt = r#"{ "recent_repos": [{ "path": "/work/repo" }"#;
        fs::write(&path, corrupt).expect("write config");

        let mut config = AppConfig::load_from(&path);
        assert!(config.repos().is_empty());
        let backup = backup_path(&path);
        assert_eq!(backup.file_name().unwrap(), "config.json.bak");
        assert_eq!(fs::read_to_string(&backup).expect("read backup"), corrupt);

        config.open_repo("/work/other", 1);
        config.save_to(&path).expect("save");
        assert_eq!(fs::read_to_string(&backup).expect("read backup"), corrupt);
        let reloaded = AppConfig::load_from(&path);
        assert_eq!(reloaded.repos()[0].path, "/work/other");
    }

    #[test]
    fn a_wrong_typed_value_resets_only_that_value() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(CONFIG_FILE_NAME);
        fs::write(
            &path,
            r#"{
                "recent_repos": [{"path": "/work/repo"}],
                "preferences": {
                    "allow_encrypted_tokens": true,
                    "auto_fetch_interval_minutes": "5",
                    "network": {"allow_ssh": false, "network_timeout_secs": "slow"}
                }
            }"#,
        )
        .expect("write");

        let config = AppConfig::load_from(&path);
        assert_eq!(config.repos().len(), 1);
        assert!(config.preferences().allow_encrypted_tokens());
        assert_eq!(
            config.preferences().auto_fetch_interval_minutes(),
            Preferences::default().auto_fetch_interval_minutes()
        );
        assert!(!config.preferences().network().allow_ssh);
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
        config.open_repo("/work/repo", 1);
        config.save_to(&path).expect("first save");
        config.open_repo("/work/other", 2);
        config.save_to(&path).expect("second save");

        assert_eq!(AppConfig::load_from(&path).repos().len(), 2);
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
        config.open_repo("/work/repo", 1);
        config.save_to(&link).expect("save");

        assert!(fs::symlink_metadata(&link)
            .expect("link metadata")
            .file_type()
            .is_symlink());
        assert_eq!(AppConfig::load_from(&target).repos().len(), 1);
    }
}
