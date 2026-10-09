# GitSpace

[![License: GPL-3.0-or-later](https://img.shields.io/badge/License-GPL--3.0--or--later-blue.svg)](LICENSE)

GitSpace is a desktop Git client that gathers your GitHub and GitLab repositories in one
panel-based interface inspired by GitKraken. It is written in Rust with egui.

> **Status:** early development. The app builds and runs from source, and CI builds and
> tests it on Linux, macOS and Windows. There is no published release yet. GitSpace has no
> built-in updater; updates will be delivered through the Colony launcher.

## What it does

- Clone repositories from GitHub, GitLab or any Git URL, and keep a list of recent ones.
- Browse a repository: overview, history, local and remote branches, staging with diffs,
  stashes and remotes.
- Branch actions: create, rename, delete, check out, merge, rebase and compare.
- Sign in to GitHub and GitLab through OAuth or a personal access token. Tokens are kept in
  the system keyring or in an encrypted local file.
- Structured logs with rotation (`GITSPACE_LOG` sets the filter).

## Build from source

Requirements:

- Rust (latest stable) and Git.
- On Linux, the usual egui/wgpu system libraries (see `.github/workflows/ci.yml`).

```bash
git clone https://github.com/Project-Colony/GitSpace
cd GitSpace
cargo run --release
```

## Project layout

- `src/ui/`: egui panels and layout.
- `src/git/`: `git2` wrappers for repository operations.
- `src/auth/`: OAuth and token storage.
- `docs/`: design notes and contributor guide ([docs/contrib.md](docs/contrib.md)).

## License

GitSpace is licensed under the [GNU General Public License v3.0 or later](LICENSE).

The bundled JetBrains Mono Nerd Font files in `assets/JetBrainsMonoNerdFont/` are licensed
under the SIL Open Font License 1.1 (see [OFL.txt](assets/JetBrainsMonoNerdFont/OFL.txt)).
