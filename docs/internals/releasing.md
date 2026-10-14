# Releasing GitSpace

Releases are never made by hand: no manual tags, no hand-edited versions, no binaries
built on a developer machine. GitSpace has no built-in updater; Colony delivers each release.

## How a release happens

1. Every merge to `main` runs `.github/workflows/release.yml`. Its release-please job reads
   the conventional commit titles since the last release and opens, or updates, a release PR
   that bumps the version in `Cargo.toml` and `Cargo.lock` and adds the entries to
   `CHANGELOG.md`. The configuration is `release-please-config.json` and
   `.release-please-manifest.json`.
2. Merging that release PR creates the `vX.Y.Z` tag and a draft release.
3. The same run builds the tag on GitHub-hosted runners into four binaries:
   `gitspace-linux`, `gitspace-windows.exe`, `gitspace-macos` (Apple Silicon) and
   `gitspace-macos-x86` (Intel). Each one must answer `--version`, and `colony.json` is
   checked against these names with `colony validate-manifest`.
4. The shared `sign-and-publish` workflow of Project-Colony-Resources signs every binary
   (`.sig`, `.meta`, `.meta.sig`, see
   [Code signing policy](../../README.md#code-signing-policy)), checks the result and
   publishes the release.

## Before merging a release PR

- CI on the release PR is green.
- The diff bumps the version in both `Cargo.toml` and `Cargo.lock`. A release PR opened
  before a `Cargo.lock` change is not always regenerated. If the lock file is missing from
  the diff, do not merge: close the PR, and the next push to `main` opens a fresh one.
- The version and the changelog read right: `feat:` raises the minor version, `fix:` the
  patch version, and the entries are the PR titles.

## After the release

Download the assets into an empty folder and check each one as the
[Code signing policy](../../README.md#code-signing-policy) shows: both signatures verify, the
`.meta` names the tag, the file name and the sha256 of the asset.

## When a run fails

If the run fails after the tag exists, the release stays a draft. Fix the cause, then rebuild
that tag:

```bash
gh workflow run release.yml --ref vX.Y.Z -f tag=vX.Y.Z
```

"Re-run all jobs" does not work for this: release-please reports nothing to do for an
existing tag, and every job after it skips.
