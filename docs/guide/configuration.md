# Settings, tokens and logs

GitSpace keeps its files in a `Colony/GitSpace` folder inside the system configuration directory,
the layout shared by every Colony program:

| Platform | Folder |
|---|---|
| Linux | `~/.config/Colony/GitSpace/` (`$XDG_CONFIG_HOME/Colony/GitSpace/` when that variable is set) |
| macOS | `~/Library/Application Support/Colony/GitSpace/` |
| Windows | `%LOCALAPPDATA%\Colony\GitSpace\` |

| File | What it holds |
|---|---|
| `config.json` | preferences, the repository list and log retention. Each repository entry has its path, whether it is pinned, when it was last opened, whether the clone-folder scan found it, and whether you removed it from the list (so the next scan does not add it back) |
| `config.json.bak` | a copy of a `config.json` that could not be parsed (see below) |
| `tokens.enc` | saved tokens, encrypted, only when "Allow an encrypted file" (Preferences → Accounts → Token storage) is on; otherwise tokens live in the system keyring only |
| `token-hosts.json` | the hosts that have a saved token |
| `token-local-key.bin`, `token-salt.bin` | the encryption key used when the system keyring is unavailable, and the salt that mixes `GITSPACE_TOKEN_MASTER_PASSWORD` into it |

Settings and token files are written to a temporary file that is then renamed into place, so a
crash or a full disk never leaves a half-written file.

## Moving from an older version

Older versions used a `gitspace` folder instead: `~/.config/gitspace/` and
`~/.local/share/gitspace/logs/` on Linux, `~/Library/Application Support/gitspace/` on macOS,
`%APPDATA%\gitspace\` on Windows. On the first start without a `config.json` in the new
folder, GitSpace moves the files listed above and the logs there, `config.json` last, and keeps
the owner-only permissions of the token files. If GitSpace stops during the move, the next start
finishes it. When a file exists in both folders with different content, GitSpace moves nothing,
uses the old folder for that run and shows an error; move or delete one of the two copies, then
restart. If another error stops the move, for example a full disk, GitSpace puts the files it already
moved back in the old folder, uses that folder for the run and tries again on the next start.

## When a file cannot be read

GitSpace never saves over a file it could not read:

- **A value this version does not know**, for example one written by a newer version: an
  unknown theme family is kept in the file and the default theme is shown until a version
  that knows it starts; an unknown language falls back to System. Every other setting is
  kept.
- **Settings from a version before the colony-ui interface**: the Catppuccin theme (Latte,
  Frappé, Macchiato or Mocha) becomes the same variant of colony-ui's Catppuccin family, and
  the recent repositories keep their order as last-opened times.
- **`config.json` cannot be parsed**: GitSpace copies it to `config.json.bak`, logs the error
  and starts with default settings. To get your settings back, close GitSpace, fix the JSON in
  the backup and move it back to `config.json`.
- **`config.json` cannot be read or backed up** (permissions, disk error): GitSpace starts
  with default settings and does not save over the file for the rest of the session.
- **`tokens.enc` cannot be decrypted**, for example because the keyring was locked when
  GitSpace started or `GITSPACE_TOKEN_MASTER_PASSWORD` changed or is no longer set: saving or
  removing a token fails with an error and the file is left as it is. Unlock the keyring or set
  the same master password, then restart GitSpace. If the key is gone for good, move
  `tokens.enc` aside to start over. Tokens saved while the keyring was locked are encrypted
  with `token-local-key.bin`; the next start with the keyring unlocked reads them with that
  key and saves them again under the keyring key.
- **`token-hosts.json` cannot be read**: the host list is left as it is and the error is
  logged.

## Network

The network preferences apply to clone, fetch, pull and push:

- **Proxy**: `https://` remotes go through the HTTPS proxy and `http://` remotes through the
  HTTP proxy, and so does the repository search on the Add repository screen. When the matching field is
  empty, git's `http.proxy` setting and the `https_proxy` and `http_proxy` environment variables
  apply. SSH remotes never go through a proxy. Token checks and OAuth sign-in do not use this
  proxy: they connect directly, or through the system proxy (the `https_proxy` and
  `http_proxy` environment variables, or the Windows and macOS proxy settings).
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
  every 1, 5, 15, 30 or 60 minutes (Preferences → Git → Background fetch), with the same
  proxy, timeout and token rules as a fetch you start yourself.

## Logs

GitSpace writes JSON logs to a `Colony/GitSpace/logs` folder inside the system data directory
(`~/.local/share/Colony/GitSpace/logs/` on Linux, `~/Library/Application Support/Colony/GitSpace/logs/`
on macOS, `%LOCALAPPDATA%\Colony\GitSpace\logs\` on Windows), one file per day. Files past the retention
count set in Preferences are deleted at startup. Logs never contain a token.

## Environment variables

| Variable | Effect |
|---|---|
| `GITSPACE_LOG` | the log filter, in [`tracing` `EnvFilter`](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html) syntax. The default is `gitspace=info,info`; `GITSPACE_LOG=gitspace=debug` shows GitSpace's own debug messages |
| `GITSPACE_TOKEN_MASTER_PASSWORD` | only used when the system keyring is unavailable: a password mixed (Argon2id, salt in `token-salt.bin`) into `token-local-key.bin`, the key that then encrypts `tokens.enc`. In that case it must be set to the same value every time GitSpace starts, or saved tokens cannot be decrypted. It is ignored while the keyring holds the key |
| `GITSPACE_GITHUB_CLIENT_ID`, `GITSPACE_GITLAB_CLIENT_ID` | read when GitSpace is **built**, not when it runs: the OAuth client IDs compiled into the binary, used instead of any ID entered under Preferences → Accounts → OAuth app client IDs. Without them, OAuth sign-in needs a client ID entered there: a GitHub OAuth app with "Enable Device Flow" ticked, or a GitLab application with "Confidential" unticked and the redirect URI `http://127.0.0.1:9876/callback` |
