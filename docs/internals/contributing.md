# Contributing to GitSpace

## Building and checking

Set up the toolchain and system packages from [install.md](../guide/install.md#from-source),
then run what CI runs, from the repository root:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

CI runs these on Linux, macOS and Windows with the latest stable Rust, adds a
`cargo check --target x86_64-apple-darwin` on macOS, and runs `cargo check` on Rust 1.88,
the `rust-version` in `Cargo.toml`. A change that needs a newer Rust raises `rust-version` on
purpose, in its own commit. `Cargo.toml` sets `resolver = "3"`, so `cargo update` only locks
crate versions that build on that Rust.

## Code

- **Edition 2021**, formatted by `cargo fmt` with the settings in `rustfmt.toml`.
- **Clippy is clean** with `-D warnings`. Silence a lint only with a comment saying why.
- **Errors** are returned with context, and logged with `tracing` and structured fields
  (`warn!(error = %err, host, "...")`), never with `println!` or `eprintln!` (the
  `--version` output and the startup failure message in `main.rs` are the exceptions).
- **Tokens** never reach a log line, an error message shown to the user, or a plain-HTTP
  request. `git/transport.rs` and `auth/` are where that is enforced; keep it there.
- **Files** the program saves go through `config::write_atomic`, and a file that exists
  but cannot be read is never overwritten.
- **Dependencies** are pinned to their full version (`"1.0.229"`, not `"1.0"`) and kept on
  their latest release. Dependabot opens the update PRs every week.
- **Tests** sit next to the code they test. Anything that changes behaviour comes with a test
  that fails without the change. Git tests build throwaway repositories with `tempfile`;
  nothing touches the network.
- **Documentation** that a change makes wrong is fixed in the same pull request. A change to
  the module layout updates [architecture.md](architecture.md).

The modules and how they fit together are in [architecture.md](architecture.md).

## Commits and pull requests

- Work on a branch named after the change (`fix/...`, `feat/...`, `docs/...`), never on
  `main`.
- The pull request title is a [Conventional Commit](https://www.conventionalcommits.org/)
  written as the changelog entry a user reads: `fix: keep saved tokens when the keyring is
  locked`, not `fix: handle Err in fetch_from_keyring`. The release notes list `feat:`,
  `fix:`, `perf:`, `refactor:` and `docs:`; `chore:`, `ci:` and `test:` stay out of them
  (`release-please-config.json`).
- Pull requests are squash-merged once CI is green, so the title becomes the commit on
  `main` and the line in `CHANGELOG.md`.
- Everything in the repository is written in English.

Releases are not part of a normal pull request: see [releasing.md](releasing.md).
