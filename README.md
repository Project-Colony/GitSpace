# GitSpace

GitSpace is a Git hub application that centralizes multiple repositories in one modern, GitKraken-inspired interface. It is primarily implemented in Rust with planned .NET interoperability where it adds value (for example, leveraging existing .NET libraries or UI components).

## Goals
- Provide an intuitive, panel-based GUI for browsing repositories, viewing history, and managing clones.
- Make it easy to list, browse, download, and clone repositories from a single workspace.
- Offer optional automated setup or installation steps for repositories when available.

## Prerequisites
- **Rust** (latest stable toolchain) for building and running the application.
- **.NET SDK** for any forthcoming .NET integration components.
- **Git** for interacting with repositories.

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
- `tasks/` – Task tracking and folder conventions (see `tasks/tasks.md`).

As the project grows, new folders will include their own documentation files following the conventions described in `tasks/tasks.md`.
