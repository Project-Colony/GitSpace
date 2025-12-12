# Release and Installation Guide

This document explains how to build GitSpace on each supported desktop platform, how the update checker works, and how to ship signed release artifacts with CI.

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

### Update checks and channels
GitSpace can automatically check for updates at launch. You can toggle this behavior and choose between the **Stable** and **Preview** channels under **Settings → Updates**.

- **Preview** deliberately targets prerelease builds so early adopters can validate fixes before they are promoted to stable. Unsigned or unchecked assets are ignored to avoid distributing unverified builds on this channel.
- **Stable** continues to prioritize the latest non-prerelease build with the same verification rules applied to every asset.

Each downloadable artifact must publish a checksum or detached signature. GitSpace downloads and validates the published fingerprint before persisting the update; if verification fails or the download stalls, the updater restores the previous file from a backup to keep the current installation intact. This rollback guard also covers partially downloaded assets so users can simply retry once network conditions improve.

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
