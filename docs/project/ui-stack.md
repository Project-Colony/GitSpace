# The UI stack

## Today: iced 0.13

GitSpace draws its window with [iced](https://iced.rs) 0.13 on the wgpu renderer, with tokio
as its runtime. The application follows iced's model: `GitSpaceApp` in `src/ui/app.rs` holds
the state, `update` handles one message at a time, and `view` builds the header, the left
rail and the active tab from that state. Each panel in `src/ui/` has its own `Message` enum,
`update` and `view`.

Git, keyring and network work never runs in `update` or `view`. It goes to an iced `Task` on
tokio's blocking pool and comes back as a message, so the window keeps drawing during a clone,
a search, a fetch, a merge or a keyring prompt.

The look is GitSpace's own: four Catppuccin palettes in `src/ui/theme.rs` and menu items in
`src/ui/menu.rs`. The fonts come from colony-ui.

## Before: egui

The first versions used [egui](https://github.com/emilk/egui) 0.27 through eframe. egui was
chosen over Tauri with a web frontend to keep one language end to end and a small binary
with no web view. It was replaced because the other Project Colony programs share a design
system built on iced, and because the immediate-mode panels ran their git calls inside the
frame.

## Where it is going: colony-ui

The shared design system,
[colony-ui](https://github.com/Project-Colony/Project-Colony-Resources/tree/main/crates/colony-ui),
is built on iced 0.14. It brings the shared typography, themes, preferences page, English and
French locales and per-user paths. Moving GitSpace onto it means upgrading from iced 0.13 to
0.14 first. `rust-version` is 1.100, above the 1.88 floor of iced 0.14.
