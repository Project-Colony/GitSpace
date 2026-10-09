# Settings and saved tokens

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
