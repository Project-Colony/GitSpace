# Installing GitSpace

There is no published release yet. Until the first one is out, building from source is the
only way to run GitSpace.

## Through Colony

Search for **GitSpace** in [Colony](https://github.com/Project-Colony/Colony) and install
it. Colony checks the signature of the download before installing it and brings updates.
GitSpace has no updater of its own.

## From the release page

Each release carries one binary per platform:

| Platform | Asset |
|---|---|
| Linux (x86_64) | `gitspace-linux` |
| Windows (x86_64) | `gitspace-windows.exe` |
| macOS (Apple Silicon) | `gitspace-macos` |
| macOS (Intel) | `gitspace-macos-x86` |

Each asset has `.sig`, `.meta` and `.meta.sig` files next to it. The
[Code signing policy](../../README.md#code-signing-policy) shows how to check them with
OpenSSL before running the binary.

## From source

You need Rust 1.88 or newer ([rustup](https://rustup.rs/) installs it) and Git.

On Debian or Ubuntu, install the system packages the window and audio crates link against
first:

```bash
sudo apt-get update
sudo apt-get install -y libasound2-dev libudev-dev pkg-config libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev
```

Other distributions need the same libraries under their own package names. macOS and
Windows need nothing beyond the Rust toolchain (on Windows, the MSVC build tools rustup
offers to install).

Then build:

```bash
git clone https://github.com/Project-Colony/GitSpace
cd GitSpace
cargo build --release
```

The binary is `target/release/gitspace` (`gitspace.exe` on Windows). `gitspace --version`
prints the version without opening a window.

## What GitSpace needs at runtime

- **The `git` command** on your `PATH`. Clone, fetch, pull, push, staging, history and
  branches go through the built-in Git library, but merge, rebase and restoring a file from
  another branch run `git` itself. Without it those three actions fail with an error and
  everything else keeps working.
- **A keyring** to keep tokens in: Keychain on macOS, Credential Manager on Windows, and a
  Secret Service provider such as GNOME Keyring or KWallet on Linux. Without one, turn on
  "Allow encrypted file storage if the native keyring is unavailable"; see
  [configuration](configuration.md) for what that stores and how well it is protected.
- **OpenGL** for drawing the window.
