# The UI stack

## Today: iced 0.14 and colony-ui

GitSpace draws its window with [iced](https://iced.rs) 0.14 on the wgpu renderer, with tokio
as its runtime, and takes its look from
[colony-ui](https://crates.io/crates/colony-ui) 0.1.6, the design system the other Project
Colony programs share. The application follows iced's model: `GitSpaceApp` in
`src/ui/app.rs` holds the state, `update` handles one message at a time, and `view` builds
the sidebar and the content area from that state. Each screen in `src/ui/` has its own
`Message` enum, `update` and `view`.

Git, keyring, network and folder-scan work never runs in `update` or `view`. It goes to an
iced `Task` on tokio's blocking pool and comes back as a message carrying the repository
path, so the window keeps drawing during a clone, a search, a fetch, a merge or a keyring
prompt, and a result for a repository that is no longer open is dropped.

What comes from colony-ui:

- the palettes of its 26 theme families and the accent colours, read through
  `colony_ui::theme::Palette` each time a widget is drawn;
- the text sizes from `Typography::current()` and the interface, glyph and
  dyslexia-friendly fonts;
- the sidebar pieces (identity button, nav labels and items, the selection style), the
  Preferences page with its General, Appearance and Accessibility categories, dropdowns,
  toggles and the success toast;
- the per-user folders (`Colony/GitSpace/`) and the English and French locale switch.

GitSpace adds the widgets the kit does not have yet in `src/ui/widgets/`: push buttons in
four strengths, chips, tabs with counts, a segmented control, "⋯" and right-click menus
drawn as iced overlays, error toasts with action buttons, and one line of text that ends in
"…" when its space is too narrow. They follow the kit's rules
(palette roles, `Typography`, the selection style) so they can move into colony-ui later.

Nothing in `src/ui/` stores a colour, a size, a font or a translated string. A test fails on
any hex colour literal there, and another on a literal user-visible string. Changing the
theme, accent, text size, dyslexia font or language in Preferences therefore redraws every
screen at once.

## Before: egui, then iced 0.13

The first versions used [egui](https://github.com/emilk/egui) 0.27 through eframe. egui was
chosen over Tauri with a web frontend to keep one language end to end and a small binary
with no web view. It was replaced because the other Project Colony programs share a design
system built on iced, and because the immediate-mode panels ran their git calls inside the
frame.

The first iced version ran on iced 0.13 with GitSpace's own look: four Catppuccin palettes, a
header, a left rail and a strip of seven tabs. colony-ui needs iced 0.14, so the move to the
kit started with that upgrade. `rust-version` is 1.100, above the 1.88 floor of iced 0.14
and colony-ui.
