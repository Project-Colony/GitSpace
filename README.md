<div align="center">

# GitSpace

**A desktop Git client for developers whose repositories live on both GitHub and GitLab.**

</div>

[![License: GPL-3.0-or-later](https://img.shields.io/badge/License-GPL--3.0--or--later-blue.svg)](LICENSE)
[![Colony app](https://img.shields.io/badge/Colony-development-purple)](https://github.com/Project-Colony/Colony)
[![Platforms](https://img.shields.io/badge/platforms-linux%20%7C%20windows%20%7C%20macOS-lightgrey)](#installation)

Graphical Git clients either serve one hosting site, like GitHub Desktop, or are closed
source and tied to an account of their own, like GitKraken. GitSpace is an open source,
native Rust client that signs in to GitHub, GitLab and their self-hosted instances side by
side, finds and clones your repositories from both, and then works on them in one
panel-based window inspired by GitKraken.

> **Status:** early development, no release yet. CI builds GitSpace and runs its tests on
> Linux, macOS and Windows, and checks that it still builds on Rust 1.88. The first signed
> release is assembled by release-please and has not been published. Personal access tokens
> work out of the box. OAuth sign-in needs the client ID of an OAuth app you register
> yourself, because no build embeds one yet. The git operations are covered by tests on
> local repositories; SSH sign-in, proxies and OAuth are covered by unit tests of their
> logic, not by tests against live servers. The interface has no language setting yet: most
> of it is in English, the Preferences window in French. It is being moved from egui to iced
> and the shared Colony UI kit, which brings English and French locales.

## Why GitSpace

- **Both sites in one place.** Tokens for GitHub, GitLab and self-hosted instances sit side
  by side, and the Clone dialog searches your repositories on either site without leaving
  GitSpace.
- **Tokens stay with you.** They go to the operating system's keyring, are only ever sent
  over HTTPS, and only to the host they belong to. GitSpace has no account of its own and no
  server: it talks to the hosts you use and nothing else.
- **Native and light.** A single Rust binary, no embedded web view.
- **No telemetry.** Nothing is collected, see [Privacy](#privacy).

## What it does

- Clone from GitHub, GitLab or any Git URL over HTTPS or SSH, and reopen recent repositories.
- Repository overview with fetch, pull (fast-forward), push, open in the file manager and
  open a terminal, plus optional auto-fetch on an interval.
- Stage and unstage files with their diffs, restore a file from another branch, and create,
  apply and drop stashes.
- History with commit diffs, filtered by branch.
- Branches: create, rename, delete, check out and track, merge or rebase into the current
  branch, compare with it, pin favourites, and archive a branch as a tag.
- Sign in to GitHub with a one-time code entered on github.com (OAuth device flow), to
  GitLab through OAuth in the browser (PKCE), or to GitHub, GitLab and self-hosted hosts with
  a personal access token.
- Network preferences: HTTP and HTTPS proxies, transfer timeout, preferring HTTPS and
  allowing SSH.
- Four Catppuccin themes, adjustable control sizes and a reduced-motion setting.
- Structured logs with daily rotation (`GITSPACE_LOG` sets the filter).

## Installation

### Via Colony (recommended)

Search for **GitSpace** in [Colony](https://github.com/Project-Colony/Colony) and install
it. Updates arrive through the launcher. GitSpace has no updater of its own. This works
once the first release is published.

### Direct binary download

Grab the asset for your platform from the [latest release](../../releases/latest):

| Platform | Asset |
|---|---|
| Linux | `gitspace-linux` |
| Windows | `gitspace-windows.exe` |
| macOS (Apple Silicon) | `gitspace-macos` |
| macOS (Intel) | `gitspace-macos-x86` |

```bash
chmod +x gitspace-linux && ./gitspace-linux
```

Every asset is signed: see [Code signing policy](#code-signing-policy) to check one.

### Build from source

```bash
git clone https://github.com/Project-Colony/GitSpace
cd GitSpace
cargo build --release
```

Requires Rust 1.88 or newer, and on Linux the system packages listed in
[docs/guide/install.md](docs/guide/install.md). At runtime GitSpace needs the `git`
command on your `PATH`: merge, rebase and restoring a file from another branch run it.

OAuth sign-in needs the client ID of an OAuth app: a GitHub OAuth app with "Enable Device
Flow" ticked, and a GitLab application with "Confidential" unticked and the redirect URI
`http://127.0.0.1:9876/callback`. Set `GITSPACE_GITHUB_CLIENT_ID` and
`GITSPACE_GITLAB_CLIENT_ID` when building to build them in, or enter them under Configure
OAuth Apps on the Auth tab. A personal access token works without one.

## Documentation

Full documentation is in [docs/](docs/). Start at [docs/README.md](docs/README.md).

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
| GitHub sign-in | sent | when you choose to sign in with GitHub: GitSpace asks `https://github.com/login/device/code` for a one-time code, you enter it at `https://github.com/login/device` in your browser, and GitSpace asks `https://github.com/login/oauth/access_token` every few seconds whether you approved it, until you approve or deny it, cancel, or the code expires. Only the OAuth app's client ID is sent, never a client secret |
| GitLab sign-in | sent | when you choose to sign in with GitLab: your browser opens `gitlab.com`, which sends the authorization code back to GitSpace on `127.0.0.1` (the first free port from 9876 to 9886), and GitSpace exchanges it for a token at `gitlab.com`, with PKCE |
| Repository search | sent | when you search in the Clone dialog: the search to `https://api.github.com` (`/user`, `/user/repos`, `/users/<name>/repos`, `/orgs/<name>/repos`) or `https://gitlab.com/api/v4/projects`, with your token for that site when one is saved |
| Git traffic | sent | clone, fetch, pull and push talk to the remote of the repository when you start them, and fetch also runs on its own while a repository is open if you turn on auto-fetch in Preferences (off by default, every 1, 5 or 15 minutes or an interval you choose). Over HTTPS the token saved for that host is sent as the password; a plain-HTTP remote never receives a token. Over SSH, GitSpace signs in with the keys in ssh-agent or `~/.ssh/id_ed25519`, `id_ecdsa` and `id_rsa`, and never sends a token |
| Local git commands | neither | merge, rebase and restoring a file from another branch run your own `git` command in the repository folder. These commands work on the local repository only and do not contact a remote |

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
