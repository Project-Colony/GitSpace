//! Where GitSpace keeps its files, and the one-time move from the old `gitspace` folders.
//!
//! Older builds kept settings in `<config dir>/gitspace/` and logs in
//! `<data dir>/gitspace/logs/`. They now live under `Colony/GitSpace/`, the layout every
//! Colony program shares (see [`colony_ui::paths`]).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const PROGRAM: &str = "GitSpace";
const LEGACY_DIR: &str = "gitspace";
const LOGS_DIR: &str = "logs";
const CONFIG_FILE: &str = "config.json";

/// Files moved from the legacy settings folder, in move order. `config.json` goes last, so
/// a `config.json` in the new folder means the move finished.
const CONFIG_FILES: [&str; 6] = [
    "token-hosts.json",
    "token-salt.bin",
    "token-local-key.bin",
    "tokens.enc",
    "config.json.bak",
    CONFIG_FILE,
];

static EFFECTIVE: OnceLock<(PathBuf, PathBuf)> = OnceLock::new();

/// The settings folder, `Colony/GitSpace/` in the platform config directory.
pub fn config_dir() -> PathBuf {
    colony_ui::paths::config_dir(PROGRAM).unwrap_or_else(|_| legacy_root(dirs::config_dir()))
}

/// The data folder, `Colony/GitSpace/` in the platform data directory.
pub fn data_dir() -> PathBuf {
    colony_ui::paths::data_dir(PROGRAM).unwrap_or_else(|_| legacy_root(dirs::data_dir()))
}

fn legacy_root(base: Option<PathBuf>) -> PathBuf {
    base.unwrap_or_else(|| PathBuf::from(".")).join(LEGACY_DIR)
}

/// The folders older builds used.
#[derive(Debug, Clone)]
pub struct LegacyDirs {
    pub config: PathBuf,
    pub data: PathBuf,
}

pub fn legacy_dirs() -> Option<LegacyDirs> {
    Some(LegacyDirs {
        config: dirs::config_dir()?.join(LEGACY_DIR),
        data: dirs::data_dir()?.join(LEGACY_DIR),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationOutcome {
    NothingToDo,
    /// The new paths of the files that were moved.
    Moved(Vec<PathBuf>),
    Failed {
        error: String,
    },
}

/// Moves the legacy files into `config` and `data`, once.
///
/// Safe to run again after a crash: a file already at its destination with the same bytes
/// only has its source removed. A destination holding different bytes stops the move before
/// anything is touched; an error during the move puts the files already moved back.
pub fn migrate_legacy(legacy: &LegacyDirs, config: &Path, data: &Path) -> MigrationOutcome {
    if config.join(CONFIG_FILE).exists() || !legacy.config.join(CONFIG_FILE).exists() {
        return MigrationOutcome::NothingToDo;
    }

    let mut pairs: Vec<(PathBuf, PathBuf)> = Vec::new();
    let legacy_logs = legacy.data.join(LOGS_DIR);
    let logs = data.join(LOGS_DIR);
    if let Ok(entries) = fs::read_dir(&legacy_logs) {
        for entry in entries.filter_map(Result::ok) {
            if entry.file_type().is_ok_and(|kind| kind.is_file()) {
                pairs.push((entry.path(), logs.join(entry.file_name())));
            }
        }
    }
    for name in CONFIG_FILES {
        pairs.push((legacy.config.join(name), config.join(name)));
    }

    let result = (|| -> io::Result<Vec<PathBuf>> {
        for (from, to) in &pairs {
            if from.exists() && to.exists() && fs::read(from)? != fs::read(to)? {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("{} already exists with different content", to.display()),
                ));
            }
        }
        fs::create_dir_all(config)?;
        if legacy_logs.is_dir() {
            fs::create_dir_all(&logs)?;
        }
        let mut moved: Vec<(&Path, &Path)> = Vec::new();
        for (from, to) in &pairs {
            match move_file(from, to) {
                Ok(true) => moved.push((from, to)),
                Ok(false) => {}
                Err(err) => return Err(roll_back(&moved, err)),
            }
        }
        // Best effort: leaves the folder in place if something unexpected is still inside.
        let _ = fs::remove_dir(&legacy_logs);
        Ok(moved.into_iter().map(|(_, to)| to.to_path_buf()).collect())
    })();

    match result {
        Ok(moved) => MigrationOutcome::Moved(moved),
        Err(err) => MigrationOutcome::Failed {
            error: err.to_string(),
        },
    }
}

/// Moves one file. Returns `false` when there was nothing to move.
fn move_file(from: &Path, to: &Path) -> io::Result<bool> {
    if !from.exists() {
        return Ok(false);
    }
    if to.exists() {
        if fs::read(from)? != fs::read(to)? {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} already exists with different content", to.display()),
            ));
        }
        fs::remove_file(from)?;
        return Ok(true);
    }
    rename_or_copy(from, to)?;
    Ok(true)
}

fn rename_or_copy(from: &Path, to: &Path) -> io::Result<()> {
    match fs::rename(from, to) {
        Err(err) if err.kind() == io::ErrorKind::CrossesDevices => copy_then_remove(from, to),
        other => other,
    }
}

/// Puts the files already moved back in the legacy folder, newest first, so this run can
/// read everything from there and the next start begins the move again.
fn roll_back(moved: &[(&Path, &Path)], err: io::Error) -> io::Error {
    let mut message = err.to_string();
    for (from, to) in moved.iter().rev() {
        if let Err(back) = rename_or_copy(to, from) {
            message.push_str(&format!(
                "; could not move {} back to {}: {back}",
                to.display(),
                from.display()
            ));
        }
    }
    io::Error::new(err.kind(), message)
}

/// The cross-filesystem move: copy to a temporary name, sync, keep the mode, rename into
/// place, then remove the source. A crash leaves at most a stray temporary file, never a
/// half-written destination.
fn copy_then_remove(from: &Path, to: &Path) -> io::Result<()> {
    let mut temp_name = to.file_name().unwrap_or_default().to_os_string();
    temp_name.push(".moving");
    let temp = to.with_file_name(temp_name);
    fs::copy(from, &temp)?;
    let file = fs::OpenOptions::new().write(true).open(&temp)?;
    file.set_permissions(fs::metadata(from)?.permissions())?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temp, to)?;
    fs::remove_file(from)
}

/// Runs the legacy move and fixes the folders for this run. Call once, before anything
/// reads a setting.
pub fn init_at_startup() -> MigrationOutcome {
    let (config, data) = (config_dir(), data_dir());
    let outcome = match legacy_dirs() {
        Some(legacy) if legacy.config != config => migrate_legacy(&legacy, &config, &data),
        _ => MigrationOutcome::NothingToDo,
    };
    let dirs = match (&outcome, legacy_dirs()) {
        (MigrationOutcome::Failed { .. }, Some(legacy)) => (legacy.config, legacy.data),
        _ => (config, data),
    };
    let _ = EFFECTIVE.set(dirs);
    outcome
}

/// The settings and data folders every module reads, as fixed by [`init_at_startup`].
pub fn effective_dirs() -> (PathBuf, PathBuf) {
    EFFECTIVE.get_or_init(|| (config_dir(), data_dir())).clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        _root: tempfile::TempDir,
        legacy: LegacyDirs,
        config: PathBuf,
        data: PathBuf,
    }

    fn fixture() -> Fixture {
        let root = tempfile::tempdir().expect("tempdir");
        let legacy = LegacyDirs {
            config: root.path().join("old-config/gitspace"),
            data: root.path().join("old-data/gitspace"),
        };
        fs::create_dir_all(&legacy.config).expect("legacy config");
        fs::create_dir_all(legacy.data.join(LOGS_DIR)).expect("legacy logs");
        Fixture {
            config: root.path().join("new-config/Colony/GitSpace"),
            data: root.path().join("new-data/Colony/GitSpace"),
            legacy,
            _root: root,
        }
    }

    fn write(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).expect("parent");
        fs::write(path, bytes).expect("write");
    }

    #[cfg(unix)]
    fn set_mode(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("chmod");
    }

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).expect("metadata").permissions().mode() & 0o777
    }

    #[test]
    fn moves_every_known_file_and_keeps_secret_permissions() {
        let f = fixture();
        for name in CONFIG_FILES {
            write(&f.legacy.config.join(name), name.as_bytes());
        }
        #[cfg(unix)]
        for name in ["tokens.enc", "token-local-key.bin", "token-salt.bin"] {
            set_mode(&f.legacy.config.join(name), 0o600);
        }
        let log = f.legacy.data.join(LOGS_DIR).join("gitspace.log.2026-11-24");
        write(&log, b"{\"level\":\"info\"}");

        let outcome = migrate_legacy(&f.legacy, &f.config, &f.data);

        let MigrationOutcome::Moved(moved) = outcome else {
            panic!("expected Moved, got {outcome:?}");
        };
        let new_log = f.data.join(LOGS_DIR).join("gitspace.log.2026-11-24");
        assert!(moved.contains(&new_log), "{moved:?}");
        assert_eq!(fs::read(&new_log).expect("log"), b"{\"level\":\"info\"}");
        assert!(!log.exists());
        for name in CONFIG_FILES {
            let new = f.config.join(name);
            assert!(moved.contains(&new), "{name} missing from {moved:?}");
            assert_eq!(fs::read(&new).expect("moved file"), name.as_bytes());
            assert!(!f.legacy.config.join(name).exists(), "{name} left behind");
        }
        #[cfg(unix)]
        assert_eq!(mode(&f.config.join("tokens.enc")), 0o600);
    }

    #[test]
    fn does_nothing_when_the_new_config_exists() {
        let f = fixture();
        write(&f.legacy.config.join(CONFIG_FILE), b"old");
        write(&f.legacy.config.join("tokens.enc"), b"old tokens");
        write(&f.config.join(CONFIG_FILE), b"new");

        assert_eq!(
            migrate_legacy(&f.legacy, &f.config, &f.data),
            MigrationOutcome::NothingToDo
        );
        assert_eq!(fs::read(f.legacy.config.join(CONFIG_FILE)).unwrap(), b"old");
        assert!(f.legacy.config.join("tokens.enc").exists());
        assert!(!f.config.join("tokens.enc").exists());
    }

    #[test]
    fn does_nothing_without_a_legacy_config() {
        let f = fixture();
        write(&f.legacy.config.join("tokens.enc"), b"tokens");

        assert_eq!(
            migrate_legacy(&f.legacy, &f.config, &f.data),
            MigrationOutcome::NothingToDo
        );
        assert!(f.legacy.config.join("tokens.enc").exists());
    }

    #[test]
    fn resumes_after_a_partial_move() {
        let f = fixture();
        write(&f.legacy.config.join("tokens.enc"), b"tokens");
        write(&f.config.join("tokens.enc"), b"tokens");
        write(&f.legacy.config.join(CONFIG_FILE), b"{}");

        let outcome = migrate_legacy(&f.legacy, &f.config, &f.data);

        assert!(matches!(outcome, MigrationOutcome::Moved(_)), "{outcome:?}");
        assert_eq!(fs::read(f.config.join("tokens.enc")).unwrap(), b"tokens");
        assert_eq!(fs::read(f.config.join(CONFIG_FILE)).unwrap(), b"{}");
        assert!(!f.legacy.config.join("tokens.enc").exists());
        assert!(!f.legacy.config.join(CONFIG_FILE).exists());
    }

    #[test]
    fn refuses_to_overwrite_a_different_file() {
        let f = fixture();
        write(&f.legacy.config.join(CONFIG_FILE), b"{}");
        write(&f.legacy.config.join("token-hosts.json"), b"[]");
        write(&f.legacy.config.join("tokens.enc"), b"old tokens");
        write(&f.config.join("tokens.enc"), b"other tokens");

        let outcome = migrate_legacy(&f.legacy, &f.config, &f.data);

        assert!(
            matches!(outcome, MigrationOutcome::Failed { .. }),
            "{outcome:?}"
        );
        assert_eq!(
            fs::read(f.legacy.config.join("tokens.enc")).unwrap(),
            b"old tokens"
        );
        assert_eq!(
            fs::read(f.config.join("tokens.enc")).unwrap(),
            b"other tokens"
        );
        assert!(f.legacy.config.join(CONFIG_FILE).exists());
        assert!(f.legacy.config.join("token-hosts.json").exists());
        assert!(!f.config.join("token-hosts.json").exists());
    }

    /// The failure comes after three files have moved: renaming a directory onto a dangling
    /// symlink fails with ENOTDIR, while the pre-check sees no destination.
    #[cfg(unix)]
    #[test]
    fn a_failure_midway_puts_every_moved_file_back() {
        let f = fixture();
        let earlier = ["token-hosts.json", "token-salt.bin", "token-local-key.bin"];
        for name in earlier.into_iter().chain([CONFIG_FILE]) {
            write(&f.legacy.config.join(name), name.as_bytes());
        }
        fs::create_dir_all(f.legacy.config.join("tokens.enc")).unwrap();
        fs::create_dir_all(&f.config).unwrap();
        std::os::unix::fs::symlink(f.config.join("missing"), f.config.join("tokens.enc")).unwrap();
        let log = f.legacy.data.join(LOGS_DIR).join("gitspace.log.2026-11-24");
        write(&log, b"log");

        let outcome = migrate_legacy(&f.legacy, &f.config, &f.data);

        assert!(
            matches!(outcome, MigrationOutcome::Failed { .. }),
            "{outcome:?}"
        );
        for name in earlier.into_iter().chain([CONFIG_FILE]) {
            assert_eq!(
                fs::read(f.legacy.config.join(name)).unwrap(),
                name.as_bytes(),
                "{name} not back in the legacy folder"
            );
            assert!(
                !f.config.join(name).exists(),
                "{name} left in the new folder"
            );
        }
        assert_eq!(fs::read(&log).unwrap(), b"log");
        assert!(!f
            .data
            .join(LOGS_DIR)
            .join("gitspace.log.2026-11-24")
            .exists());
    }

    /// `rename` cannot cross filesystems in a test, so the fallback is checked directly.
    #[test]
    fn the_cross_filesystem_copy_keeps_bytes_and_mode() {
        let f = fixture();
        let from = f.legacy.config.join("tokens.enc");
        write(&from, b"secret");
        #[cfg(unix)]
        set_mode(&from, 0o600);
        let to = f.config.join("tokens.enc");
        fs::create_dir_all(&f.config).unwrap();

        copy_then_remove(&from, &to).expect("copy");

        assert_eq!(fs::read(&to).unwrap(), b"secret");
        assert!(!from.exists());
        assert!(!f.config.join("tokens.enc.moving").exists());
        #[cfg(unix)]
        assert_eq!(mode(&to), 0o600);
    }
}
