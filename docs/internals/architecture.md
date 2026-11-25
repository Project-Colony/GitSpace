# Architecture

GitSpace is one binary crate, `gitspace`, with no workspace and no build script. `src/main.rs`
answers `--version`, moves old files, starts logging and hands the window to iced; everything
else lives in the modules below.

```
src/
├── main.rs        entry point: --version, the file move, logging, iced::application
├── ui/            the iced application: sidebar, repository tabs, Preferences, widgets
├── git/           repository operations on top of git2, plus the git CLI for three of them
├── auth/          tokens: validation, OAuth, keyring and encrypted-file storage
├── config.rs      config.json: preferences, the repository list, logging options, atomic writes
├── i18n.rs        GitSpace's labels, with i18n/en.json and i18n/fr.json
├── paths.rs       the Colony/GitSpace folders and the one-time move from the old ones
├── logging.rs     tracing setup: JSON to stderr and to a daily rotating file
└── error.rs       AppError and the logs directory
```

The dependency direction is one way: `ui` calls `git`, `auth` and `config`; `git` and `auth`
never call `ui`. `git` and `auth` meet in one place, `git/transport.rs`, which receives a
token as a plain argument from the UI.

## `ui`: the application

`ui/app.rs` holds `GitSpaceApp`, the iced application state. Its `update` routes each
message to the screen it belongs to, saves the preferences after each change, and reloads
the other tabs when one of them changes the repository (commit, checkout, merge, rebase,
pull, a stash job). A fetch or a push leaves HEAD and the working tree as they were, so it
reloads only the header and Branches. When the window gains focus it reads the header,
Changes, Branches and Stashes again, because another program may have changed the
repository. In both cases History is left as it is so that its selection stays. Its `view`
puts the sidebar next to the content area: the repository header and the active tab, the
Add repository screen when no repository is open, or Preferences while they are open (see
[gitkraken-layout.md](../project/gitkraken-layout.md)).
Its subscriptions are the keyboard shortcuts, window focus, the auto-fetch timer and the
toast timer. At startup it opens the last repository on Changes when "Reopen the last
repository at startup" is on and the folder still exists.

The screens:

| File | What it draws |
|---|---|
| `sidebar.rs`, `repo_list.rs` | the repository list, its search and its groups; `repo_list.rs` decides which rows show and in which order |
| `repo_header.rs` | the header of the open repository with Fetch, Pull and Push, and the tab strip |
| `changes.rs`, `history.rs`, `branches.rs`, `stashes.rs` | the four tabs; `diff_view.rs` draws the diffs that Changes, History and Stashes show |
| `add_repo.rs` | the Add repository screen: GitHub and GitLab search, URL, local folder, clone progress |
| `preferences/` | the Preferences page: `mod.rs` with the kit's General, Appearance and Accessibility, then `accounts.rs`, `git.rs`, `network.rs` and `about.rs` |
| `widgets/` | buttons, chips, tabs, the segmented control, menus and toasts the kit does not have yet |
| `context.rs` | `RepoContext`, the open repository, and `git_task`, which runs a git call off the UI thread |

Colours come from `colony_ui::theme::Palette`, sizes from `Typography::current()` and fonts
from colony-ui, read each time a widget is drawn; `main.rs` registers the font files. Every
visible string goes through `crate::i18n::t` or the kit's `colony_ui::i18n::t`. Two tests
keep this true: `ui_has_no_hex_colours` in `ui/widgets/mod.rs` and
`ui_has_no_literal_strings` in `i18n.rs`. Another test checks that English and French have
the same keys.

Every git, keyring, network and folder-scan call runs in an iced `Task`, on tokio's blocking
pool, and returns its result as a message. Results carry the repository path they were
computed for, and a screen drops a result for a repository that is no longer open. `view`
functions only read state. The sidebar status of every repository comes from
`git::status::quick_status`, at most four at a time, and the clone folder is scanned by
`git::discover::discover_repositories`.

## `git`: repository operations

Thin functions over [git2](https://docs.rs/git2) (libgit2, vendored OpenSSL), one file per
area: `clone.rs`, `remote.rs` (fetch, pull as a fast-forward of the checked-out branch only,
push of the checked-out branch to its upstream), `branch.rs`, `commit.rs`, `log.rs`,
`diff.rs`, `status.rs`, `stash.rs`, `merge.rs` and `discover.rs` (the repositories one level
down in a folder). `status.rs` reads changed files with their line counts from diff stats,
without building patches, and `quick_status` reads only what the sidebar shows. Checkout, pull and stash
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
  one-time code and refuses a verification page that is not `https://`; Preferences →
  Accounts shows the code, and `DeviceFlow::wait` polls the token endpoint at the interval
  GitHub sets. It adds 5 seconds on `slow_down`, doubles the interval up to a minute while
  the endpoint cannot be reached or answers with a rate limit or a server error, and stops
  when the code is approved, denied or expired, or the user cancels. Only the client ID is
  sent, never a client secret.
- **GitLab** uses the authorization code flow with PKCE: `OAuthFlow` opens the browser and
  listens for the callback on `127.0.0.1` port 9876, the redirect URI registered with the
  GitLab application. If that port is in use, sign-in stops with an error before the browser
  opens. A request without the expected `state` gets a 400 and the listener keeps waiting.
  An expired GitLab token is refreshed with its refresh token when it is next needed.

The client IDs come from `GITSPACE_GITHUB_CLIENT_ID` and `GITSPACE_GITLAB_CLIENT_ID` at build
time (`ui/preferences/accounts.rs`), and otherwise from the IDs entered under Preferences →
Accounts → OAuth app client IDs.

## `config`, `logging`, `error`

`config.rs` loads and saves `config.json`. An unknown value falls back to its default
without discarding the rest, out-of-range numbers are clamped when read, an unparseable file is backed up before defaults are used, and
every write goes through `write_atomic` (temporary file, sync, rename). Where the files live
is in [configuration](../guide/configuration.md).

`logging.rs` sets up `tracing`: JSON lines to stderr and to a daily file in the logs
directory from `error.rs`, filtered by `GITSPACE_LOG` (default `gitspace=info,info`), pruning
files past the configured retention at startup.

## Files and paths

`paths.rs` owns every location: settings and token files in `colony_ui::paths::config_dir("GitSpace")`,
logs in `logs/` under `colony_ui::paths::data_dir("GitSpace")`. `main` calls
`paths::init_at_startup` before logging starts; it moves the files of the old
`<config dir>/gitspace/` and `<data dir>/gitspace/` folders once and fixes the folders that
`paths::effective_dirs` returns for the run (the old ones when the move failed).

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
