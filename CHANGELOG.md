# Changelog

Every release, with what changed and why. The body of a release on GitHub is the
section of its tag, from `## vX.Y.Z` to the next one.

## v0.3.1

**Game Save 0.3.1** — a hotfix: the command line and the service agree on the
database.

### The fix

`gamesave user add`, `gamesave key create` and every other command read
`/etc/gamesave/config.yaml` when `--config` is not given, so they work on an
installed server. Before, they fell back to `./data`: an administrator who
created an account from the command line was writing to another database, and
the server answered `401 invalid_api_key` to every key it issued, also from
outside localhost.

### Also

- the installer prints the exact commands for the first account and its key,
  with the `--config` they need;
- `README.md`, `docs/DEPLOY.md` and `docs/INSTALL.md` show the `--config` and say
  that another configuration means another database.

### Licence

GPLv3. See `LICENSE`.

## v0.3.0

**Game Save 0.3.0** — the sync works on Windows, the server can be installed and
updated without a terminal, and every account has a limit.

### The session watcher on Windows

`desktop/src/session.rs` now detects the game on Windows: it enumerates the
processes with `CreateToolhelp32Snapshot`, reads the image path with
`QueryFullProcessImageNameW` and compares it with the registered executable,
canonicalised and lower-cased. A session opens when the game appears and closes
when it disappears, exactly like on Linux.

### Closing the window hides it in the tray

The **X** does not quit any more. It hides the window and the app keeps
synchronising; on Windows a toast says that it is still running. **Quit** in the
tray menu is the only way to close it. The tick moved into `App::logic`, so the
watcher runs even while the window is hidden.

### A storage quota per account

Every account has a quota, **5 GiB** by default, `0` for no limit. The server
counts the logical size of the current files and refuses an upload that would
pass the quota with `413 quota_exceeded`. The command is
`gamesave user quota <user> [size]`, and the client shows `used / quota` in the
bottom bar.

### The Linux installer

```bash
curl -fsSL https://raw.githubusercontent.com/regalf/game-save/main/scripts/install.sh | sh
```

The bootstrap downloads `install.py` from the repository (not from a release),
and the installer asks how the server runs (native or Docker), writes the
configuration, chooses the service user, installs a systemd unit and prints the
fingerprint. `--uninstall`, `--dry-run`, `--yes`, `--user`, `--update-check`.

### The update check, and the manual update

The server can look for a newer release when `update.check` is on (off by
default) and tell the clients, which show a notification. The server never
updates itself: `gamesave update` downloads the binary, checks the sha256
published in the release and replaces the running executable.

### The release workflow

A `v*` tag builds the server (Linux x86-64 and ARM64, Windows) and the client
(Linux and Windows), writes `latest.json` with the version and the sha256 of
every binary, and creates the release with all the files. `latest.json` now has
a `clients` section next to `assets`.

### Artefacts

| File | What it is |
|---|---|
| `gamesave-server-linux-amd64` | the server, Linux x86-64 |
| `gamesave-server-linux-arm64` | the server, Linux ARM64 |
| `gamesave-server-windows-amd64.exe` | the server, Windows |
| `gamesave-client-linux-amd64` | the companion app, Linux x86-64 |
| `gamesave-client-windows-amd64.exe` | the companion app, Windows |
| `latest.json` | the version and the sha256 of every binary, for the updater |

### Not in this release

From `DECISIONS.md`:

- the end to end encryption, opt-in per user;
- the chunk level delta upload for large files;
- the automatic discovery of the save folder;
- the packaging: AppImage, deb, a systemd user unit, a Windows installer.

The repository has to be public for the installer and the updater to work: they
read from `raw.githubusercontent.com` and from the releases without
authentication.

### Licence

GPLv3. See `LICENSE`. The interface font is Fira Code, under the SIL Open Font
License, which is compatible with the GPL.

## v0.2.0

**The code is the same as 0.1.0.** A documentation release: the presentation,
the decisions, the architecture, the protocol, the deployment and the security
policy, each with one job. The version string of the server became `0.2.0`.

## v0.1.0

First release: the Go server (SQLite, blob store, API keys, manifest, optimistic
locking, history, local CA) and the Rust/egui companion app (library of games,
scanner, session watcher, sync engine, conflicts, tray, autostart, icons,
theme).
