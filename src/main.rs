#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod auth;
mod config;
mod error;
mod git;
mod logging;
mod ui;

use colony_ui::fonts;
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

    let result = fonts::BYTES
        .into_iter()
        .fold(
            iced::application(GitSpaceApp::new, GitSpaceApp::update, GitSpaceApp::view),
            |app, font| app.font(font),
        )
        .title("GitSpace")
        .subscription(GitSpaceApp::subscription)
        .theme(GitSpaceApp::theme)
        .default_font(colony_ui::ui_font())
        .window_size((1280.0, 800.0))
        .run();

    if let Err(err) = result {
        tracing::error!(target: "gitspace::main", error = %err, "echec du lancement de l'interface GitSpace");
        eprintln!("Erreur: Impossible de lancer GitSpace: {err}");
        eprintln!("Verifiez que votre serveur d'affichage est lance et accessible.");
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
