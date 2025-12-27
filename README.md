# GitSpace

GitSpace is a Git hub application that centralizes multiple repositories in one modern, GitKraken-inspired interface. It is primarily implemented in Rust with planned .NET interoperability where it adds value (for example, leveraging existing .NET libraries or UI components).

## Goals
- Provide an intuitive, panel-based GUI for browsing repositories, viewing history, and managing clones.
- Make it easy to list, browse, download, and clone repositories from a single workspace.
- Offer optional automated setup or installation steps for repositories when available.

## Prerequisites
- **Rust** (latest stable toolchain) for building and running the application.
- **.NET SDK 8.x** for any forthcoming .NET integration components.
- **Git** for interacting with repositories.

## .NET Installation Notes
- Follow the platform-specific instructions in [`docs/dotnet-setup.md`](docs/dotnet-setup.md).
- Ensure the `dotnet` CLI is available on your `PATH`.
- If you install the SDK to a custom location, set `DOTNET_ROOT` and add it to your `PATH`.
- Optional: set `DOTNET_CLI_TELEMETRY_OPTOUT=1` to disable .NET CLI telemetry.

## Key Dependencies
- **eframe/egui** for the panel-based desktop UI (with WGPU rendering).
- **git2** for repository operations (branches, status, history, stash, remotes).
- **tracing** + **tracing-subscriber** for structured logging.
- **reqwest** for networked features and update checks.
- **keyring** + **chacha20poly1305** for secure credential handling.

## Current Functionality
- Launches a multi-pane egui interface (`GitSpaceApp`) with logging configured out of the box.
- Provides panels for cloning, recent repositories, repository overview, history, branches, staging, authentication, and settings.
- Emits optional, anonymized telemetry about app and repository openings (with opt-out controls).
- Implements git helpers for listing, creating, deleting, renaming, and checking out branches, along with status, history, diff, stash, and merge utilities.

## Architecture Overview
- **UI (`src/ui/`)**: egui components and layout wiring for the GitKraken-inspired interface.
- **Git (`src/git/`)**: wrappers around `git2` for repository operations consumed by the UI.
- **Auth (`src/auth/`)**: authentication primitives to be wired into provider flows.
- **Config (`src/config.rs`)**: user and runtime configuration settings.
- **Telemetry (`src/telemetry.rs`)**: anonymized diagnostics with batching and user controls.
- **Logging (`src/logging.rs`)**: structured log setup for the desktop app.

## Quick Start
1. Install the prerequisites above.
2. Clone this repository.
3. Build the project:
   ```bash
   cargo build
   ```
4. Run the project:
   ```bash
   cargo run
   ```

## Project Structure
- `src/` – Rust source code for the application entry point and modules.
- `docs/` – High-level documentation about architecture, design, and decisions (see `docs/docs.md`).
- `docs/telemetry.md` – Details on optional diagnostics, collected fields, batching, and how to purge data.
- `tasks/` – Task tracking and folder conventions (see `tasks/tasks.md`).

As the project grows, new folders will include their own documentation files following the conventions described in `tasks/tasks.md`.
