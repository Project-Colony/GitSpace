#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod auth;
mod config;
mod error;
mod git;
mod logging;
mod ui;
mod update;

use ui::app::GitSpaceApp;
use ui::fonts;

fn main() {
    logging::init_tracing();
    log_dev_feature_flags();

    let result = iced::application("GitSpace", GitSpaceApp::update, GitSpaceApp::view)
        .subscription(GitSpaceApp::subscription)
        .theme(GitSpaceApp::theme)
        .font(fonts::FONT_REGULAR)
        .font(fonts::FONT_MEDIUM)
        .font(fonts::FONT_SEMIBOLD)
        .font(fonts::FONT_BOLD)
        .font(fonts::FONT_ITALIC)
        .font(fonts::FONT_BOLD_ITALIC)
        .font(fonts::FONT_MONO_REGULAR)
        .font(fonts::FONT_MONO_BOLD)
        .default_font(fonts::DEFAULT_FONT)
        .window_size((1280.0, 800.0))
        .run_with(GitSpaceApp::new);

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
