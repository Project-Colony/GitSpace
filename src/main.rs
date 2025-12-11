mod config;
mod git;
mod ui;

use ui::app::GitSpaceApp;

fn main() {
    let native_options = eframe::NativeOptions::default();

    eframe::run_native(
        "GitSpace",
        native_options,
        Box::new(|_cc| Box::new(GitSpaceApp::new())),
    )
    .expect("failed to start GitSpace UI");
}
