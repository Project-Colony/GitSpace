# Release and Installation Guide

This document explains how to build GitSpace on each supported desktop platform and how to ship release artifacts with CI. GitSpace has no built-in updater: updates will be delivered through the Colony launcher.

## Building locally

### Linux
1. Install Rust (stable toolchain) and the desktop build dependencies:
   ```bash
   sudo apt-get update && sudo apt-get install -y libasound2-dev libudev-dev pkg-config libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev
   ```
2. Build a release binary:
   ```bash
   cargo build --release
   ```
3. The optimized binary is available at `target/release/gitspace`.

### macOS
1. Install the Rust toolchain (via [rustup](https://rustup.rs/) or Homebrew).
2. Build the optimized binary:
   ```bash
   cargo build --release
   ```
3. The binary is located at `target/release/gitspace`.

### Windows
1. Install the Rust toolchain using the official installer.
2. Build the optimized binary:
   ```powershell
   cargo build --release
   ```
3. The binary is located at `target/release/gitspace.exe`.

## CI release workflow

The GitHub Actions workflow in `.github/workflows/release.yml` builds release artifacts for Linux, macOS, and Windows.

* **Triggers:** manual (`workflow_dispatch`) or a pushed tag that matches `v*`.
* **Build:** runs `cargo build --release` on each OS, installing required Linux system packages before compilation.
* **Packaging:** bundles the binary into platform-specific archives (tar.gz for Linux/macOS, zip for Windows).
* **Artifacts:** uploads the archives as workflow artifacts named `gitspace-<os>` (one per platform) for distribution or attaching to a GitHub Release.

To create a tagged release:
1. Bump the crate version in `Cargo.toml` if needed.
2. Tag the commit (e.g., `git tag v0.2.0 && git push origin v0.2.0`).
3. Download the artifacts from the workflow run and publish them (or attach them to a GitHub Release page).
