# Changelog

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
