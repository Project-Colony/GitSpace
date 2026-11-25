# Changelog

## [0.2.0](https://github.com/Project-Colony/GitSpace/compare/v0.1.2...v0.2.0) (2026-11-25)


### Features

* store files under Colony/GitSpace and move existing ones once ([1ed9b7d](https://github.com/Project-Colony/GitSpace/commit/1ed9b7d3cffd40de0979e2186bb86b55f049ff81))
* **ui:** add a branch relation card, grouped merge and rebase, and a menu for rare branch actions ([1ed9b7d](https://github.com/Project-Colony/GitSpace/commit/1ed9b7d3cffd40de0979e2186bb86b55f049ff81))
* **ui:** add a repository from GitHub, GitLab, a URL or a local folder ([1ed9b7d](https://github.com/Project-Colony/GitSpace/commit/1ed9b7d3cffd40de0979e2186bb86b55f049ff81))
* **ui:** list repositories in the sidebar with an in-place search, pinned and recent groups, and per-repository status ([1ed9b7d](https://github.com/Project-Colony/GitSpace/commit/1ed9b7d3cffd40de0979e2186bb86b55f049ff81))
* **ui:** move Preferences to the colony-ui page with Accounts, Git and Network, 26 theme families and English and French ([1ed9b7d](https://github.com/Project-Colony/GitSpace/commit/1ed9b7d3cffd40de0979e2186bb86b55f049ff81))
* **ui:** open a repository on Changes, History, Branches and Stashes tabs under a header with Fetch, Pull and Push ([1ed9b7d](https://github.com/Project-Colony/GitSpace/commit/1ed9b7d3cffd40de0979e2186bb86b55f049ff81))
* **ui:** rebuild the interface on colony-ui ([#53](https://github.com/Project-Colony/GitSpace/issues/53)) ([1ed9b7d](https://github.com/Project-Colony/GitSpace/commit/1ed9b7d3cffd40de0979e2186bb86b55f049ff81))
* **ui:** rebuild the interface on iced ([#1](https://github.com/Project-Colony/GitSpace/issues/1)) ([af99248](https://github.com/Project-Colony/GitSpace/commit/af99248eaf0c46ff138acf2dcc16008189b79075))
* **ui:** show branch, remote and tag badges and merge markers in History ([1ed9b7d](https://github.com/Project-Colony/GitSpace/commit/1ed9b7d3cffd40de0979e2186bb86b55f049ff81))
* **ui:** show line counts, discard a file and commit with a summary counter, templates and sign-off in Changes ([1ed9b7d](https://github.com/Project-Colony/GitSpace/commit/1ed9b7d3cffd40de0979e2186bb86b55f049ff81))
* **ui:** show the branch, date, file count and contents of each stash ([1ed9b7d](https://github.com/Project-Colony/GitSpace/commit/1ed9b7d3cffd40de0979e2186bb86b55f049ff81))


### Fixes

* **auth:** create secret files owner-only ([fd0099d](https://github.com/Project-Colony/GitSpace/commit/fd0099df6fb8c71501a9a128f36f79e5f4878de3))
* **auth:** save tokens under the host name lookups use, remove them everywhere on sign-out, and refresh GitLab OAuth tokens ([fd0099d](https://github.com/Project-Colony/GitSpace/commit/fd0099df6fb8c71501a9a128f36f79e5f4878de3))
* **auth:** stop sending a GitHub Enterprise token to api.github.com ([fd0099d](https://github.com/Project-Colony/GitSpace/commit/fd0099df6fb8c71501a9a128f36f79e5f4878de3))
* **config:** keep valid values when one has the wrong type, and clamp the auto-fetch interval ([fd0099d](https://github.com/Project-Colony/GitSpace/commit/fd0099df6fb8c71501a9a128f36f79e5f4878de3))
* correct data-loss, security and UI-thread bugs found in an audit ([#52](https://github.com/Project-Colony/GitSpace/issues/52)) ([fd0099d](https://github.com/Project-Colony/GitSpace/commit/fd0099df6fb8c71501a9a128f36f79e5f4878de3))
* **deps:** update dependencies and Rust to 1.100 ([#48](https://github.com/Project-Colony/GitSpace/issues/48)) ([39f85c9](https://github.com/Project-Colony/GitSpace/commit/39f85c9de044bfa9a34fe87a87b9d6cd7f48428d))
* **git:** check the stash id before dropping or applying a stash ([fd0099d](https://github.com/Project-Colony/GitSpace/commit/fd0099df6fb8c71501a9a128f36f79e5f4878de3))
* **git:** count real lines in history, filter before the page limit, and open an empty repository ([fd0099d](https://github.com/Project-Colony/GitSpace/commit/fd0099df6fb8c71501a9a128f36f79e5f4878de3))
* **git:** keep uncommitted changes on checkout, pull and stash apply ([fd0099d](https://github.com/Project-Colony/GitSpace/commit/fd0099df6fb8c71501a9a128f36f79e5f4878de3))
* **git:** offer the token only to https remotes and treat git+ssh and scp-style URLs as SSH ([fd0099d](https://github.com/Project-Colony/GitSpace/commit/fd0099df6fb8c71501a9a128f36f79e5f4878de3))
* **git:** pull and push only the checked-out branch, and refuse when HEAD is another branch ([fd0099d](https://github.com/Project-Colony/GitSpace/commit/fd0099df6fb8c71501a9a128f36f79e5f4878de3))
* **git:** read git's output while merge and rebase run, and reload the index before listing conflicts ([fd0099d](https://github.com/Project-Colony/GitSpace/commit/fd0099df6fb8c71501a9a128f36f79e5f4878de3))
* **ui:** run git, keyring and network work off the interface thread, confirm destructive actions, and add a Commit button ([fd0099d](https://github.com/Project-Colony/GitSpace/commit/fd0099df6fb8c71501a9a128f36f79e5f4878de3))
* **ui:** stage a modified file instead of deleting it when GitSpace starts from another folder ([fd0099d](https://github.com/Project-Colony/GitSpace/commit/fd0099df6fb8c71501a9a128f36f79e5f4878de3))

## [0.1.2](https://github.com/Project-Colony/GitSpace/compare/v0.1.1...v0.1.2) (2026-11-09)


### Fixes

* **deps:** update dependencies and Rust to 1.99 ([#42](https://github.com/Project-Colony/GitSpace/issues/42)) ([50c58ff](https://github.com/Project-Colony/GitSpace/commit/50c58ffc23b4e554e8950b311ea7335b08a6befc))

## [0.1.1](https://github.com/Project-Colony/GitSpace/compare/v0.1.0...v0.1.1) (2026-10-14)


### Fixes

* authenticate SSH remotes, keep long clones running, use the configured proxy and open terminals safely ([#30](https://github.com/Project-Colony/GitSpace/issues/30)) ([22b8f64](https://github.com/Project-Colony/GitSpace/commit/22b8f642c949b06b041da3dc91472862d972cdb0))
* **auth:** never send an access token to a self-hosted host over plain HTTP ([#28](https://github.com/Project-Colony/GitSpace/issues/28)) ([9d41496](https://github.com/Project-Colony/GitSpace/commit/9d414969341af0b70aeb45f7faa9369489a72515))
* **auth:** sign in to GitHub with the device flow ([#33](https://github.com/Project-Colony/GitSpace/issues/33)) ([21e1590](https://github.com/Project-Colony/GitSpace/commit/21e1590d0c807177929c45005e0bab5d9b1f0b40))
* **deps:** bump Tmds.DBus to 0.95.1 ([#5](https://github.com/Project-Colony/GitSpace/issues/5)) ([2851346](https://github.com/Project-Colony/GitSpace/commit/28513463d957363dc79dc6e7faa4455759cf5107))
* **deps:** update git2 to 0.21 ([#15](https://github.com/Project-Colony/GitSpace/issues/15)) ([8ed0d84](https://github.com/Project-Colony/GitSpace/commit/8ed0d84716c3335d7a64747b0eb7f0d08d807754))
* **deps:** update the crates with RustSec advisories ([#6](https://github.com/Project-Colony/GitSpace/issues/6)) ([da87e2b](https://github.com/Project-Colony/GitSpace/commit/da87e2b90725acdb3aed0d8ca9b0a19016f46ef8))
* get CI green on Linux, macOS and Windows ([#2](https://github.com/Project-Colony/GitSpace/issues/2)) ([89633eb](https://github.com/Project-Colony/GitSpace/commit/89633ebee2dab83c3b436a8d28ac286b5e9fc626))
* keep symlinked config files and saved tokens when a write fails ([#21](https://github.com/Project-Colony/GitSpace/issues/21)) ([de4e659](https://github.com/Project-Colony/GitSpace/commit/de4e659297a1dc69ab61bf8627382430629ecf91))
* remove the .NET helper that ran `dotnet run` from the working directory at startup ([#18](https://github.com/Project-Colony/GitSpace/issues/18)) ([7cf01d1](https://github.com/Project-Colony/GitSpace/commit/7cf01d179c0085440718eca159c20ad1323b9381))
* remove the built-in updater and its compromised signing key ([#4](https://github.com/Project-Colony/GitSpace/issues/4)) ([8740490](https://github.com/Project-Colony/GitSpace/commit/87404908d8a88713a35875fa49b80efad7bc8e0e))
* stop losing saved tokens and preferences when a file cannot be read ([#19](https://github.com/Project-Colony/GitSpace/issues/19)) ([e17f041](https://github.com/Project-Colony/GitSpace/commit/e17f041472e51155c533fa8aebf247cbd55a5a5f))


### Internals

* **auth:** generate the credential nonce with generate_nonce ([#3](https://github.com/Project-Colony/GitSpace/issues/3)) ([828996b](https://github.com/Project-Colony/GitSpace/commit/828996b16c70002db757f53884ae302bd94b1344))
