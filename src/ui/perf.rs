use std::time::Instant;

use tracing::info;

pub enum PerfScope {
    Enabled { label: &'static str, start: Instant },
    Disabled,
}

impl PerfScope {
    pub fn new(label: &'static str) -> Self {
        if std::env::var_os("GITSPACE_PROFILE_UI").is_some() {
            Self::Enabled {
                label,
                start: Instant::now(),
            }
        } else {
            Self::Disabled
        }
    }
}

impl Drop for PerfScope {
    fn drop(&mut self) {
        if let Self::Enabled { label, start } = self {
            let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
            info!(
                target: "gitspace::ui::perf",
                label = *label,
                elapsed_ms,
                "ui update path profiled"
            );
        }
    }
}
