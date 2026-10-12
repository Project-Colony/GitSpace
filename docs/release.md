# Release and Installation Guide

This document explains how to build GitSpace on each supported desktop platform and how releases are published. GitSpace has no built-in updater: updates will be delivered through the Colony launcher.

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

## Releases

Releases are never made by hand: no manual tags, no hand-edited versions.

1. Every merge to `main` runs `.github/workflows/release.yml`. Its release-please job reads the
   conventional commit titles since the last release and opens, or updates, a release PR that
   bumps the version in `Cargo.toml` and `Cargo.lock` and adds the entries to `CHANGELOG.md`.
   The configuration is `release-please-config.json` and `.release-please-manifest.json`.
2. Merging that release PR creates the `vX.Y.Z` tag and a draft release.
3. The same run builds the tag on GitHub-hosted runners into four binaries:
   `gitspace-linux`, `gitspace-windows.exe`, `gitspace-macos` (Apple Silicon) and
   `gitspace-macos-x86` (Intel). Each one must answer `--version`, and `colony.json` is checked
   against these names with `colony validate-manifest`.
4. The shared `sign-and-publish` workflow of Project-Colony-Resources signs every binary
   (`.sig`, `.meta`, `.meta.sig`, see [Code signing policy](../README.md#code-signing-policy)),
   checks the result and publishes the release.

If a run fails after the tag exists, the release stays a draft. Fix the cause, then rebuild that
tag with `gh workflow run release.yml --ref vX.Y.Z -f tag=vX.Y.Z`.
