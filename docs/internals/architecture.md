# Architecture

GitSpace is one binary crate, `gitspace`, with no workspace and no build script. `src/main.rs`
answers `--version`, starts logging and hands the window to iced; everything else lives in
five modules.

```
src/
├── main.rs        entry point: --version, logging, iced::application
├── ui/            the iced application: shell, tabs, panels, theme
├── git/           repository operations on top of git2, plus the git CLI for three of them
├── auth/          tokens: validation, OAuth, keyring and encrypted-file storage
├── config.rs      config.json: preferences, network and logging options, atomic writes
├── logging.rs     tracing setup: JSON to stderr and to a daily rotating file
└── error.rs       AppError and the logs directory
```

The dependency direction is one way: `ui` calls `git`, `auth` and `config`; `git` and `auth`
never call `ui`. `git` and `auth` meet in one place, `git/transport.rs`, which receives a
token as a plain argument from the UI.

## `ui`: the application

`ui/app.rs` holds `GitSpaceApp`, the iced application state. Its `update` routes each
message to the panel it belongs to, applies changes the panels report through `take_*`
methods, and reloads the other panels when one of them changes the repository (commit,
checkout, merge, rebase). Its `view` draws the shell from `ui/layout.rs` (header, left rail,
tab strip, see [gitkraken-layout.md](../project/gitkraken-layout.md)) and the active tab. Its
subscriptions are the Ctrl+1 to Ctrl+7 tab shortcuts, the auto-fetch timer and the toast
timer.

One file per tab: `clone.rs` (search and clone), `recent.rs` (the Open tab), `repo_overview.rs`,
`stage.rs`, `history.rs`, `branches.rs`, `auth.rs`, and `preferences.rs` for the full-window
Preferences. Shared pieces: `theme.rs` (the Catppuccin palettes), `menu.rs` (menu items),
`fonts.rs` (the bundled JetBrains Mono Nerd Font, Regular only), `context.rs` (the open
repository) and `notifications.rs` (toasts).

Every git, keyring and network call runs in an iced `Task`, on tokio's blocking pool, and
returns its result as a message. Results carry the repository path they were computed for,
and a panel drops a result for a repository that is no longer open. `view` functions only
read state.

## `git`: repository operations

Thin functions over [git2](https://docs.rs/git2) (libgit2, vendored OpenSSL), one file per
area: `clone.rs`, `remote.rs` (fetch, pull as a fast-forward of the checked-out branch only,
push of the checked-out branch to its upstream), `branch.rs`, `commit.rs`, `log.rs`,
`diff.rs`, `status.rs`, `stash.rs`, `compare.rs` and `merge.rs`. Checkout, pull and stash
apply use libgit2's safe checkout, which fails instead of overwriting a modified file.

`transport.rs` is shared by every network operation: it checks the URL against the network
preferences (HTTPS preferred, SSH allowed), picks the proxy, applies the idle timeout, and
answers libgit2's credential callback. Only `https://` remotes get the saved token. Any URL
with a colon before its first slash (`ssh://`, `git+ssh://`, `host:path`) counts as SSH, which
tries ssh-agent then the default key files, each method once.

Three operations shell out to the `git` command instead of libgit2, which is why `git` must
be on `PATH` at runtime: merge and rebase (`merge.rs`, with a timeout, reading git's output
while it runs) and restoring a file from another branch (`branch.rs`).

`repo_cache.rs` keeps a few open `Repository` handles per thread for the read-only
functions; `merge.rs` re-reads the index before it lists conflicts.

## `auth`: tokens

`auth/mod.rs` has `AuthManager`, which validates a token against the host's API before saving
it, and `TokenStorage`, which keeps it. Tokens go to the operating system keyring through the
`keyring` crate (Keychain, Credential Manager or Secret Service, chosen at compile time).
With the encrypted fallback on, they are also written to `tokens.enc` (ChaCha20-Poly1305,
fresh nonce per write) under a key that lives in the keyring, or in `token-local-key.bin`
when the keyring is unavailable, optionally mixed with `GITSPACE_TOKEN_MASTER_PASSWORD`
through Argon2id. If the keyring key cannot decrypt the file, the local key is tried and the
file is written again under the keyring key. A file that exists but cannot be decrypted is
never overwritten, and secret files are created owner-only (0600) on Unix. Tokens are saved
under the lowercase host name without scheme or port, the form lookups use; a plain `http://`
host is refused before any request. Signing out removes the token from the keyring and from
`tokens.enc`.

`auth/oauth.rs` runs the two OAuth sign-ins, both through its own reqwest client with
redirects turned off:

- **GitHub** uses the device flow (RFC 8628). `DeviceFlow::start` asks `github.com` for a
  one-time code and refuses a verification page that is not `https://`; the Auth tab shows
  the code, and `DeviceFlow::wait` polls the token endpoint at the interval GitHub sets. It
  adds 5 seconds on `slow_down`, doubles the interval up to a minute while the endpoint cannot
  be reached or answers with a rate limit or a server error, and stops when the code is approved, denied or expired, or the user cancels. Only
  the client ID is sent, never a client secret.
- **GitLab** uses the authorization code flow with PKCE: `OAuthFlow` opens the browser and
  listens for the callback on `127.0.0.1` port 9876, the redirect URI registered with the
  GitLab application. If that port is in use, sign-in stops with an error before the browser
  opens. A request without the expected `state` gets a 400 and the listener keeps waiting.
  An expired GitLab token is refreshed with its refresh token when it is next needed.

The client IDs come from `GITSPACE_GITHUB_CLIENT_ID` and `GITSPACE_GITLAB_CLIENT_ID` at build
time (`ui/auth.rs`), and otherwise from the IDs entered under Configure OAuth Apps.

## `config`, `logging`, `error`

`config.rs` loads and saves `config.json`. An unknown value falls back to its default
without discarding the rest, out-of-range numbers are clamped when read, an unparseable file is backed up before defaults are used, and
every write goes through `write_atomic` (temporary file, sync, rename). Where the files live
is in [configuration](../guide/configuration.md).

`logging.rs` sets up `tracing`: JSON lines to stderr and to a daily file in the logs
directory from `error.rs`, filtered by `GITSPACE_LOG` (default `gitspace=info,info`), pruning
files past the configured retention at startup.

## Files and paths

All locations come from the `dirs` crate: settings and token files in
`<config dir>/gitspace/`, logs in `<data dir>/gitspace/logs/`. Each module builds its own path
from those two roots; there is no central paths module yet.

## Features

| Feature | Effect |
|---|---|
| `mock-providers` | logs that external services are mocked |
| `fake-repos` | logs that repository operations use synthetic data |
| `dev-tools` | both of the above |

Today these only log a warning at startup; nothing else reads them.

## Tests

Unit tests sit next to the code, in a `#[cfg(test)] mod tests` block, or in `git/tests.rs`
for the larger git suite. The git tests build throwaway repositories with
`tempfile`. CI runs the built binary with `--version`, as the release smoke test does.

## Release chain

`release.yml` runs release-please on every push to `main`. Merging its release PR tags the
version, builds the four binaries on GitHub-hosted runners, and hands them to the shared
sign-and-publish workflow of Project-Colony-Resources, which signs and publishes them. The
steps are in [releasing.md](releasing.md).
