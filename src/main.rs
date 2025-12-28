#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod auth;
mod config;
mod dotnet;
mod error;
mod git;
mod logging;
mod telemetry;
mod ui;
mod update;

use ui::app::GitSpaceApp;

fn main() {
    logging::init_tracing();
    log_dev_feature_flags();

    let native_options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    eframe::run_native(
        "GitSpace",
        native_options,
        Box::new(|_cc| Box::new(GitSpaceApp::new())),
    )
    .expect("failed to start GitSpace UI");
}

fn log_dev_feature_flags() {
    #[cfg(feature = "mock-providers")]
    tracing::warn!(
        target: "gitspace::features",
        "mock providers enabled; external services will be mocked"
    );

    #[cfg(feature = "fake-repos")]
    tracing::warn!(
        target: "gitspace::features",
        "fake repositories enabled; repository operations use synthetic data"
    );

    #[cfg(all(not(feature = "mock-providers"), not(feature = "fake-repos")))]
    tracing::debug!(target: "gitspace::features", "running with production feature set");
}
