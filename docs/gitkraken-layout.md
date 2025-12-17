# GitKraken-like layout maturity

This note captures the layout grid, navigation prototypes, and telemetry touchpoints for the GitKraken-inspired shell.

## Primary panes and docking
- **Left rail (repository tree + quick actions):** `ShellLayout::sidebar` uses a resizable `SidePanel` with a default width of `220px`. It holds navigation sections for workspaces and common actions, and it can collapse or expand via the panel drag handle.
- **Center canvas (active content):** The main tab strip rides above the scrollable content region inside the central panel. Tabs map to the core panes: Clone/Open, Repo Overview, Stage/Diff, History graph, Branches, and Auth/Settings.
- **Right rail (context):** `ShellLayout::right_panel` mirrors the left panel with a `SidePanel` defaulting to `260px`, surfacing the active repository summary and contextual tips when no repo is loaded.
- **Resizing behavior:** Both rails use egui’s built-in resize affordances and retain their widths across interactions within a session. Tabs stay docked to the center panel and redraw when widths change to keep history graphs and diffs legible.
- **Docking expectations:** The header is pinned to the top, with navigation rails always docked left/right; only the center content scrolls. Dragging tabs reorders the active strip without detaching panes.

## Navigation prototypes
- **Keyboard shortcuts:** `Ctrl/Cmd + 1-8` jumps to the corresponding tab in the current tab order, enabling rapid toggling between history, stage/diff, branches, remotes, and settings without using the mouse.
- **Context menus:** Right-clicking a tab opens a context menu with a direct “Switch to …” action, providing a discoverable secondary path to focus a pane.
- **Drag-and-drop:** The tab strip accepts drag gestures; dragging a tab over another swaps their positions and immediately focuses the dropped tab. This offers a lightweight docking prototype for users who prefer spatial organization.
- **Fallback click flow:** Standard left-clicking still selects a tab; the underline accent highlights the active pane.

## Telemetry + opt-in UX
- **Event coverage:** UI navigation emits `ui_tab_switch` (properties: tab label + trigger of click/keyboard/context menu/drag-and-drop) and `ui_tab_reordered` (properties: tab label, from/to indices). Repository selection still hashes paths before emission.
- **Prompt + controls:** A one-time notification asks users to opt into telemetry; Settings → Privacy mirrors the toggle and offers a purge button. Declines are honored without re-prompting.
- **Responsible defaults:** Telemetry is off until the user consents. Events queue locally when offline; users can clear queued payloads, and disabling telemetry stops further collection.
