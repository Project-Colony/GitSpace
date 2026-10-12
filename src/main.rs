#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod auth;
mod config;
mod error;
mod git;
mod logging;
mod ui;

use ui::app::GitSpaceApp;

fn main() {
    // Answered before logging starts and before any window opens: the release
    // workflow smoke-tests every built binary with `--version` on runners that
    // have no display.
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--version" || arg == "-V")
    {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        return;
    }

    logging::init_tracing();
    log_dev_feature_flags();

    let native_options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    if let Err(err) = eframe::run_native(
        "GitSpace",
        native_options,
        Box::new(|_cc| Box::new(GitSpaceApp::new())),
    ) {
        tracing::error!(target: "gitspace::main", error = %err, "failed to start GitSpace UI");
        eprintln!("Error: Failed to start GitSpace UI: {err}");
        eprintln!("Please check that your display server is running and accessible.");
        std::process::exit(1);
    }
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
