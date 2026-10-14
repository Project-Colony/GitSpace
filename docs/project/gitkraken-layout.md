# The GitKraken-style layout

GitSpace borrows its overall shape from GitKraken: a navigation rail on the left, one
working area in the middle, and the repository you are working on always one click away.
This page describes the shell as it is built today, in `src/ui/layout.rs` and
`src/ui/app.rs`, and what it deliberately leaves out.

## The window

```
+----------------------------------------------------------------+
| GitSpace  Workspace shell                         (header, 48 px)|
+---------------+------------------------------------------------+
| Navigation    | Clone | Open | Repo Overview | Stage | ...      |
|  Workspaces   +------------------------------------------------+
|  Actions      |                                                |
|  Context      |            active tab (scrolls)                |
|  (resizable,  |                                                |
|   220 px)     |                                                |
+---------------+------------------------------------------------+
```

- **Header.** Pinned to the top. Clicking the GitSpace name opens Preferences, which
  replaces the whole window until it is closed (General, Appearance, Accessibility and
  GitSpace categories).
- **Left rail.** A resizable side panel, 220 px wide by default, in three groups:
  - *Workspaces*: Recent, Favorites and Local Repos open the Open tab; Remote Repos opens
    Clone.
  - *Actions*: Clone, Open, New Branch (the Branches tab) and Sync (the Stage tab).
  - *Context*: the active repository with its path, and buttons to open it in the file
    manager or copy its path. "No repository selected" until one is open.
- **Centre.** A tab strip above the content of the active tab. Only the content scrolls; the
  header, rail and tab strip stay where they are.

## Tabs

In their default order: Clone, Open, Repo Overview, Stage, History, Branches and Auth.
Debug builds add a Dev Gallery tab at the end, a gallery of the menus, buttons and inputs
with their animations, for development.

- **Click** a tab to show it. The active tab is underlined in the theme's accent colour.
- **Ctrl+1 to Ctrl+8** (Cmd on macOS) jump to the tab in that position of the current order.
- **Drag** a tab over another to swap them; the dragged tab becomes active. The new order
  lasts until GitSpace is closed.

## What it leaves out

GitKraken has more than this, and so did earlier notes for GitSpace. None of the following
exists, and none of it is planned for the egui shell:

- a right-hand context rail;
- context menus on tabs, or tabs that detach into their own window;
- a commit graph with one lane per branch: History draws a single line, with merge commits
  marked;
- telemetry of any kind, including on navigation (see [Privacy](../../README.md#privacy)).

The interface is being moved to iced and the shared Colony UI kit (see
[ui-stack.md](ui-stack.md)); this page changes with that move.
