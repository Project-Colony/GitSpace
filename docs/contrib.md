# Contributing to GitSpace

This guide outlines how to work on GitSpace so changes stay consistent and maintainable. Pair it with `docs.md` for folder context and `tasks/tasks.md` for open work.

## Coding standards
- **Rust edition**: Target Rust 2024 and keep the codebase `cargo fmt` clean. Formatting is configured via `rustfmt.toml`; run `cargo fmt-all` before opening a PR.
- **Linting**: Use `cargo clippy-all` to check all targets and features. Fix new lints you introduce and avoid silencing diagnostics unless there is a documented rationale.
- **Logging**: Instrument new logic with `tracing` spans and events. Default filters come from the `GITSPACE_LOG` env var (fallback `gitspace=info,info`). Use structured fields instead of string concatenation.
- **Error handling**: Propagate errors with context where possible; log them with structured fields instead of `println!`/`eprintln!`.
- **Features**: Optional dev tooling is gated behind feature flags:
  - `mock-providers`: swap external service calls for mocks.
  - `fake-repos`: operate on synthetic repositories.
  - `dev-tools`: convenience flag that enables both.

## Workflow
1. **Set up tooling**: Install the Rust toolchain and run `cargo fmt-all` followed by `cargo clippy-all` locally.
2. **Plan changes**: Update or add docs alongside code when behavior changes (UI notes live in `docs/`).
3. **Logging**: Initialize tracing early in the binary (see `src/logging.rs`) and prefer structured fields for observability.
4. **Testing**: Add focused unit tests where possible. Prefer deterministic helpers over global state; use the `fake-repos`/`mock-providers` features for local experiments.
5. **Commits**: Keep commits scoped and descriptive. Include context in messages when modifying configuration or behavior.
6. **PRs**: Summarize user-visible changes and validation steps. Mention enabled feature flags and logging changes when relevant.

## Folder expectations
- Every folder includes a Markdown explainer (see `tasks/tasks.md` for the convention).
- Keep documentation close to the implementation it describes; link files when cross-referencing.
