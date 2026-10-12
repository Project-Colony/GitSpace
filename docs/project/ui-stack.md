# The UI stack

## Today: egui 0.27 through eframe

GitSpace draws its window with [egui](https://github.com/emilk/egui) 0.27, run by eframe with
the OpenGL (glow) renderer. Panels are immediate-mode: every frame, `GitSpaceApp::update`
in `src/ui/app.rs` lays out the header, the left rail and the active tab again from the
application state. Clone, repository search, token checks, OAuth sign-in and auto-fetch run
on a background thread behind a `poll_promise::Promise`, so the window keeps drawing while
they wait on the network. Fetch, pull and push started from Repo Overview do not yet: they
run on the UI thread, and the window stops responding until they finish.

The look is GitSpace's own: four Catppuccin palettes in `src/ui/theme.rs`, motion tokens and
the reduced-motion switch in `src/ui/animation.rs`, the bundled JetBrains Mono Nerd Font in
`src/ui/fonts.rs`, and hand-drawn menu items in `src/ui/menu.rs`.

## Why egui was chosen

Two options were on the table when GitSpace started:

- **Tauri with a web frontend**: the whole web ecosystem and mature UI kits, at the cost of a
  Rust backend plus a web view at runtime, a two-language build and a larger memory footprint
  for a simple shell.
- **A native Rust GUI (egui)**: one language end to end, a small binary with no web view, and
  typed colours for theming, at the cost of fewer ready-made widgets and less polish out of
  the box.

egui won, to keep the project in Rust and the runtime small while the workspace UI was being
prototyped.

## Where it is going: iced and colony-ui

The other Project Colony programs share one design system,
[colony-ui](https://github.com/Project-Colony/Project-Colony-Resources/tree/main/crates/colony-ui),
built on [iced](https://iced.rs) 0.14. It brings the shared typography, themes, motion,
preferences page, English and French locales and the per-user paths, none of which egui
GitSpace can reuse. GitSpace is being ported to it, which is also why `rust-version` is 1.88,
the floor of iced 0.14. Until that port lands, the egui shell described here is what runs.
