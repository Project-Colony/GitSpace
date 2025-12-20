use std::sync::OnceLock;

use tracing_appender::{non_blocking, rolling};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::{config::AppConfig, error::logs_directory};

const DEFAULT_LOG_FILTER: &str = "gitspace=info,info";
const LOG_ENV: &str = "GITSPACE_LOG";
const LOG_FILE_PREFIX: &str = "gitspace.log";

static FILE_GUARD: OnceLock<non_blocking::WorkerGuard> = OnceLock::new();

pub fn init_tracing() {
    if tracing::dispatcher::has_been_set() {
        return;
    }

    let env_filter = build_env_filter();

    let fmt_layer = fmt::layer()
        .with_thread_ids(false)
        .with_file(true)
        .with_line_number(true)
        .json();

    let subscriber = tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer);

    if let Some((writer, guard)) = build_file_writer() {
        let file_layer = fmt::layer()
            .with_thread_ids(false)
            .with_ansi(false)
            .with_file(true)
            .with_line_number(true)
            .json()
            .with_writer(writer);
        let _ = FILE_GUARD.set(guard);
        subscriber.with(file_layer).init();
    } else {
        subscriber.init();
    }
}

fn build_env_filter() -> EnvFilter {
    std::env::var(LOG_ENV)
        .ok()
        .and_then(|value| EnvFilter::try_new(value).ok())
        .unwrap_or_else(|| EnvFilter::new(DEFAULT_LOG_FILTER))
}

fn build_file_writer() -> Option<(non_blocking::NonBlocking, non_blocking::WorkerGuard)> {
    let logging = AppConfig::load().logging().retention_files();
    let log_dir = logs_directory();

    let file_appender = rolling::daily(&log_dir, LOG_FILE_PREFIX);
    let (non_blocking, guard) = non_blocking(file_appender);

    prune_old_logs(&log_dir, logging);

    Some((non_blocking, guard))
}

fn prune_old_logs(log_dir: &std::path::Path, max_files: usize) {
    if max_files == 0 {
        return;
    }

    let mut entries = match std::fs::read_dir(log_dir) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let path = entry.path();
                let name = path.file_name()?.to_string_lossy();
                if name.starts_with(LOG_FILE_PREFIX) {
                    let modified = entry.metadata().ok()?.modified().ok()?;
                    Some((path, modified))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>(),
        Err(_) => return,
    };

    entries.sort_by(|a, b| b.1.cmp(&a.1));
    for (index, (path, _)) in entries.iter().enumerate() {
        if index >= max_files {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    unsafe fn set_env<K: AsRef<std::ffi::OsStr>, V: AsRef<std::ffi::OsStr>>(key: K, value: V) {
        unsafe { std::env::set_var(key, value) };
    }

    unsafe fn remove_env<K: AsRef<std::ffi::OsStr>>(key: K) {
        unsafe { std::env::remove_var(key) };
    }

    #[test]
    fn init_tracing_creates_log_file() {
        let temp = tempdir().unwrap();
        let previous_data_dir = std::env::var_os("XDG_DATA_HOME");
        let previous_config_dir = std::env::var_os("XDG_CONFIG_HOME");

        unsafe {
            set_env("XDG_DATA_HOME", temp.path());
            set_env("XDG_CONFIG_HOME", temp.path());
            set_env(LOG_ENV, DEFAULT_LOG_FILTER);
        }

        init_tracing();
        tracing::info!("logging init smoke test");

        std::thread::sleep(std::time::Duration::from_millis(50));

        let logs_dir = logs_directory();
        let log_files: Vec<_> = std::fs::read_dir(&logs_dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(LOG_FILE_PREFIX)
            })
            .collect();

        unsafe {
            if let Some(dir) = previous_data_dir {
                set_env("XDG_DATA_HOME", dir);
            } else {
                remove_env("XDG_DATA_HOME");
            }

            if let Some(dir) = previous_config_dir {
                set_env("XDG_CONFIG_HOME", dir);
            } else {
                remove_env("XDG_CONFIG_HOME");
            }
        }

        assert!(
            !log_files.is_empty(),
            "expected at least one log file to be created"
        );
    }
}
