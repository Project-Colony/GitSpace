# UI Stack Decision

We considered two approaches for the desktop interface: a Tauri-based web frontend and a native Rust GUI.

## Tauri + Frontend
- Pros: access to the broader web ecosystem, reuse of web tooling, built-in window/chrome management, and rapid iteration for UI-heavy work.
- Cons: larger runtime surface (Rust backend + webview), more complex build pipeline, and higher memory footprint for simple shells.

## Native Rust GUI (egui via eframe)
- Pros: single-language stack, small binary without embedded webview, ergonomic immediate-mode API, and straightforward theming with typed colors.
- Cons: smaller widget ecosystem than the web and less built-in theming/polish compared to mature web UI kits.

## Decision
We chose **egui/eframe** for the initial shell to keep the project in Rust end-to-end and minimize runtime overhead while prototyping the workspace UI.
