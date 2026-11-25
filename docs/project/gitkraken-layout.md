# The window layout

GitSpace started from GitKraken's shape: a rail on the left and one working area. The
colony-ui redesign kept the idea of a rail and changed what it holds. The sidebar is now the
list of repositories, and a repository's sections are tabs in the content area, so switching
repository or section takes one click. This page describes the window as `src/ui/app.rs`,
`src/ui/sidebar.rs` and `src/ui/repo_header.rs` build it.

## The window

```
+---------------+--------------------------------------------------+
| GitSpace      | colony-ui  [main] [↑2 ↓0]   ▤ >_  Fetch Pull Push|
| Repos · 34  ⌕ | Changes·4  History  Branches  Stashes·2           |
| PINNED        +--------------------------------------------------+
|  colony-ui  4 |                                                  |
|  GitSpace   ↑2|              active tab                          |
| RECENT        |                                                  |
|  Eidos      ↓3|                                                  |
|  ...          |                                                  |
| + Clone or open                                                  |
| Ctrl+K search |                                                  |
+---------------+--------------------------------------------------+
```

The window has no other chrome: no header bar, no menu bar.

## Sidebar

The sidebar is 240 px wide.

- The GitSpace button at the top opens Preferences, and the same button closes it.
- A label row shows "Repositories" and how many there are, with a magnifier at its right
  end. The magnifier or Ctrl+K replaces the row with a search field of the same height, so
  the list below does not move. The search matches names, ignoring case, and highlights the
  matched letters. Up and Down move the selection, Enter opens the repository, and Escape
  closes the search and clears it.
- The list has two groups. PINNED comes first; RECENT holds every other repository, by the
  time it was last opened, and repositories found in the clone folder but never opened come
  last, by name. While searching, one group shows the results.
- Each row shows the number of changed files and the ahead and behind counts against the
  upstream. A repository whose folder is gone is dimmed and marked "missing".
- A right-click on a row opens a menu: Pin or Unpin, Remove from list, Open folder and
  Open terminal.
- "+ Clone or open" at the bottom opens the Add repository screen, and a hint line under it
  lists two shortcuts.

The row status comes from a background scan that reads the branch, the ahead and behind
counts and the number of changed files, never a patch. At most four repositories are
scanned at the same time. The scan runs at startup, when the window gains focus, after an
operation on a repository and on the auto-fetch timer.

## Content area

With a repository open, the content area has a header and four tabs:

- The header shows the repository name, a chip with the branch (or "detached at" and a
  short hash), a chip with the ahead and behind counts when the branch has an upstream,
  buttons to open the folder and a terminal, and Fetch, Pull and Push. Push is the primary
  button when the branch is ahead, Pull when it is behind. The "⋯" menu at the right lists
  the remotes.
- The tabs are Changes (with the number of changed files), History, Branches and Stashes
  (with the number of stashes).

With no repository open, on the first start for example, the content area shows the Add
repository screen. Preferences replace the content area while they are open; the sidebar
stays.

## Keyboard

| Keys | Action |
|---|---|
| Ctrl+K | open the repository search |
| Ctrl+1 to Ctrl+4 | Changes, History, Branches, Stashes |
| Alt+Up, Alt+Down | previous or next repository in the list |
| Ctrl+, | open or close Preferences |
| F2 | rename the selected branch, on the Branches tab |
| Escape | close the search, a menu or Preferences |

On macOS, Cmd replaces Ctrl.

## What it leaves out

GitKraken has more than this. None of the following exists, and none of it is planned yet:

- a commit graph with lanes: History draws one line with a dot per commit;
- interactive rebase, blame, amend and staging single hunks;
- tabs that detach into their own window;
- telemetry of any kind, including on navigation (see [Privacy](../../README.md#privacy)).
