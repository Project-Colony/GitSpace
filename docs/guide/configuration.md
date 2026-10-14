# Settings, tokens and logs

GitSpace keeps its files in a `gitspace` folder inside the system configuration directory:

| Platform | Folder |
|---|---|
| Linux | `~/.config/gitspace/` (`$XDG_CONFIG_HOME/gitspace/` when that variable is set) |
| macOS | `~/Library/Application Support/gitspace/` |
| Windows | `%APPDATA%\gitspace\` |

| File | What it holds |
|---|---|
| `config.json` | preferences, recent repositories and log retention |
| `config.json.bak` | a copy of a `config.json` that could not be parsed (see below) |
| `tokens.enc` | saved tokens, encrypted, only when "Allow encrypted file storage if the native keyring is unavailable" is on; otherwise tokens live in the system keyring only |
| `token-hosts.json` | the hosts that have a saved token |
| `token-local-key.bin`, `token-salt.bin` | the encryption key used when the system keyring is unavailable, and the salt that mixes `GITSPACE_TOKEN_MASTER_PASSWORD` into it |

Settings and token files are written to a temporary file that is then renamed into place, so a
crash or a full disk never leaves a half-written file.

## When a file cannot be read

GitSpace never saves over a file it could not read:

- **Unknown theme or motion value in `config.json`**, for example one written by another
  version: that value falls back to its default and every other setting is kept.
- **`config.json` cannot be parsed**: GitSpace copies it to `config.json.bak`, logs the error
  and starts with default settings. To get your settings back, close GitSpace, fix the JSON in
  the backup and move it back to `config.json`.
- **`config.json` cannot be read or backed up** (permissions, disk error): GitSpace starts
  with default settings and does not save over the file for the rest of the session.
- **`tokens.enc` cannot be decrypted**, for example because the keyring was locked when
  GitSpace started or `GITSPACE_TOKEN_MASTER_PASSWORD` changed or is no longer set: saving or
  removing a token fails with an error and the file is left as it is. Unlock the keyring or set
  the same master password, then restart GitSpace. If the key is gone for good, move
  `tokens.enc` aside to start over.
- **`token-hosts.json` cannot be read**: the host list is left as it is and the error is
  logged.

## Network

The network preferences apply to clone, fetch, pull and push:

- **Proxy**: `https://` remotes go through the HTTPS proxy and `http://` remotes through the
  HTTP proxy, as GitSpace's own GitHub and GitLab requests do. When the matching field is
  empty, git's `http.proxy` setting and the `https_proxy` and `http_proxy` environment variables
  apply. SSH remotes never go through a proxy.
- **Timeout**: a transfer stops when the connection sends nothing for that many seconds. A long
  clone keeps running as long as data arrives. `0` turns the timeout off. Git transfers read the
  value once, at the first clone, fetch or push, so restart GitSpace after changing it. On
  Windows, HTTPS remotes wait as long as the system allows.
- **SSH remotes** (`git@host:owner/repo.git` or `ssh://...`): GitSpace signs in with the keys
  loaded in ssh-agent, then with `~/.ssh/id_ed25519`, `~/.ssh/id_ecdsa` and `~/.ssh/id_rsa`.
  A key protected by a passphrase only works through ssh-agent. The host key must already be
  in `~/.ssh/known_hosts`: connect once with `ssh -T git@host` to add it. A saved token is
  never sent over SSH.
- **HTTPS remotes** use the saved token for the host. A remote reached over plain `http://`
  never receives the token.
- **Auto-fetch**: off by default. When on, GitSpace fetches the remote of the open repository
  every 1, 5 or 15 minutes, or at an interval you enter, with the same proxy, timeout and
  token rules as a fetch you start yourself.

## Logs

GitSpace writes JSON logs to a `gitspace/logs` folder inside the system data directory
(`~/.local/share/gitspace/logs/` on Linux, `~/Library/Application Support/gitspace/logs/` on
macOS, `%APPDATA%\gitspace\logs\` on Windows), one file per day. Files past the retention
count set in Preferences are deleted at startup. Logs never contain a token.

## Environment variables

| Variable | Effect |
|---|---|
| `GITSPACE_LOG` | the log filter, in [`tracing` `EnvFilter`](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html) syntax. The default is `gitspace=info,info`; `GITSPACE_LOG=gitspace=debug` shows GitSpace's own debug messages |
| `GITSPACE_TOKEN_MASTER_PASSWORD` | a password mixed into the key that encrypts `tokens.enc` (Argon2id). It must be set to the same value every time GitSpace starts, or saved tokens cannot be decrypted |
| `GITSPACE_PROFILE_UI` | when set, logs how long each part of the interface takes to draw, for profiling |
| `GITSPACE_GITHUB_CLIENT_ID`, `GITSPACE_GITLAB_CLIENT_ID` | read when GitSpace is **built**, not when it runs: the OAuth client IDs compiled into the binary, used instead of any ID entered in **Configure OAuth Apps** on the Auth tab. Without them, OAuth sign-in needs a client ID entered there: a GitHub OAuth app with "Enable Device Flow" ticked, or a GitLab application with "Confidential" unticked and the redirect URI `http://127.0.0.1:9876/callback` |
