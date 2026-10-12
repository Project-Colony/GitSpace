# GitSpace

[![License: GPL-3.0-or-later](https://img.shields.io/badge/License-GPL--3.0--or--later-blue.svg)](LICENSE)

GitSpace is a desktop Git client that gathers your GitHub and GitLab repositories in one
panel-based interface inspired by GitKraken. It is written in Rust with egui.

> **Status:** early development. The app builds and runs from source, and CI builds and
> tests it on Linux, macOS and Windows. There is no published release yet. GitSpace has no
> built-in updater; updates will be delivered through the Colony launcher.

## What it does

- Clone repositories from GitHub, GitLab or any Git URL, and keep a list of recent ones.
- Browse a repository: overview, history, local and remote branches, staging with diffs,
  stashes and remotes.
- Branch actions: create, rename, delete, check out, merge, rebase and compare.
- Sign in to GitHub and GitLab through OAuth or a personal access token. Tokens are kept in
  the system keyring or in an encrypted local file, and are only ever sent over HTTPS: a
  self-hosted host entered as `http://` is refused, and a plain-HTTP remote never receives a
  token.
- Structured logs with rotation (`GITSPACE_LOG` sets the filter).

## Build from source

Requirements:

- Rust 1.88 or newer, and Git.
- On Linux, the usual egui/glow (OpenGL) system libraries (see `.github/workflows/ci.yml`).

```bash
git clone https://github.com/Project-Colony/GitSpace
cd GitSpace
cargo run --release
```

## Project layout

- `src/ui/`: egui panels and layout.
- `src/git/`: `git2` wrappers for repository operations.
- `src/auth/`: OAuth and token storage.
- `docs/`: design notes, the contributor guide ([docs/contrib.md](docs/contrib.md)) and where
  settings and tokens are stored ([docs/guide/configuration.md](docs/guide/configuration.md)).

## Code signing policy

Every release asset is signed by the Project Colony organisation in CI, never on
a developer machine. Next to each asset on the release page:

| File | What it is |
|---|---|
| `<asset>.sig` | an ed25519 signature over the asset, made with the organisation's release key |
| `<asset>.meta` | three lines binding the asset to its file name, its sha256 and the release version |
| `<asset>.meta.sig` | an ed25519 signature over the `.meta` |

The private key is an organisation secret, used only by the shared
[sign-and-publish workflow](https://github.com/Project-Colony/Project-Colony-Resources/blob/main/.github/workflows/sign-and-publish.yml)
in a job that builds nothing; the jobs that compile GitSpace never see it.
Colony checks all three files before it installs or updates GitSpace, and
refuses a release older than the one installed. To check a download yourself
with OpenSSL 3 (the same commands work for every asset):

```bash
cat > colony-release.pub <<'EOF'
-----BEGIN PUBLIC KEY-----
MCowBQYDK2VwAyEARNjg3Nn8H6/aBg1unwGjkUTcrdTxERNefVaqU8cFu0s=
-----END PUBLIC KEY-----
EOF
a=gitspace-linux
openssl pkeyutl -verify -pubin -inkey colony-release.pub -rawin -in "$a" -sigfile "$a.sig"
openssl pkeyutl -verify -pubin -inkey colony-release.pub -rawin -in "$a.meta" -sigfile "$a.meta.sig"
cat "$a.meta"     # version=<tag>, asset=<file name>, sha256=<digest>
sha256sum "$a"    # the digest must equal the sha256 line
```

How releases are built, signed and published:
[design/releases.md](https://github.com/Project-Colony/Project-Colony-Resources/blob/main/design/releases.md#5-signing).

## Privacy

GitSpace sends no telemetry, no analytics and no crash reports.

| Data | Stored or sent | Where, and why |
|---|---|---|
| Settings | stored | `config.json` in the `gitspace` folder of the system configuration directory (`~/.config/gitspace/` on Linux, see [configuration](docs/guide/configuration.md)): preferences, network options, the paths of recent repositories and the log retention |
| Logs | stored | `gitspace/logs/` in the system data directory (`~/.local/share/gitspace/logs/` on Linux), one file per day, the oldest deleted past the retention set in Preferences. They record operations and errors, never a token |
| Access tokens | stored | the operating system's keyring (service `gitspace`, one entry per host, plus `oauth:<host>` with the refresh token and expiry after an OAuth sign-in). With "Allow encrypted file storage if the native keyring is unavailable" on, they are also written to `tokens.enc` in the settings folder, encrypted with a key kept in the keyring. When the keyring cannot be used, that key is the file `token-local-key.bin` in the same folder instead, readable by your user account, so `tokens.enc` is then only as safe as that account. Setting `GITSPACE_TOKEN_MASTER_PASSWORD` mixes a password into that key (Argon2id, salt in `token-salt.bin`). `token-hosts.json` lists the hosts that have a token |
| Token check | sent | when you save a personal access token: the token to `https://api.github.com/user`, `https://gitlab.com/api/v4/user`, or the matching API of a self-hosted GitHub or GitLab host you entered. A self-hosted host must use `https://`; an `http://` host is refused, so a token never travels in clear text |
| OAuth sign-in | sent | when you choose to sign in with GitHub or GitLab: your browser opens `github.com` or `gitlab.com`, which sends the authorization code back to GitSpace on `127.0.0.1` (the first free port from 9876 to 9886), and GitSpace exchanges it for a token at the same site, with PKCE. It needs the client ID of an OAuth app you register yourself; a personal access token works without one |
| Repository search | sent | when you search in the Clone dialog: the search to `https://api.github.com` (`/user`, `/user/repos`, `/users/<name>/repos`, `/orgs/<name>/repos`) or `https://gitlab.com/api/v4/projects`, with your token for that site when one is saved |
| Git traffic | sent | clone, fetch, pull and push talk to the remote of the repository, only when you start them. Over HTTPS the token saved for that host is sent as the password; a plain-HTTP remote never receives a token. Over SSH, GitSpace signs in with the keys in ssh-agent or `~/.ssh/id_ed25519`, `id_ecdsa` and `id_rsa`, and never sends a token |

When a proxy is set in Preferences, the repository search and HTTP(S) git traffic
go through it; SSH remotes never do.
GitSpace connects to no server except the ones in this table, and only for the
action named there. Their privacy policies:
[GitHub](https://docs.github.com/site-policy/privacy-policies/github-general-privacy-statement),
[GitLab](https://about.gitlab.com/privacy/).

## License

GitSpace is licensed under the [GNU General Public License v3.0 or later](LICENSE).

The bundled JetBrains Mono Nerd Font files in `assets/JetBrainsMonoNerdFont/` are licensed
under the SIL Open Font License 1.1 (see [OFL.txt](assets/JetBrainsMonoNerdFont/OFL.txt)).
