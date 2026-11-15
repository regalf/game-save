# Game Save

[![CI](https://github.com/regalf/game-save/actions/workflows/ci.yml/badge.svg)](https://github.com/regalf/game-save/actions/workflows/ci.yml)

Self-hosted cloud save for the games that have none.

Game Save is a small Go server that stores your game saves, and a companion
desktop app that watches your games and keeps the saves in sync between
machines. It is built for a homelab: one binary, one SQLite file, one blob
directory, no external service, no account, no telemetry.

It is for the games that keep their saves in a folder on disk and have no
cloud service of their own: the indie games, the games from GOG, Epic or
itch.io, and the Steam games whose developer never added the Steam Cloud SDK.

---

## When to use it, and when not to

| Game | What to use |
|---|---|
| A game that integrates the Steam Cloud SDK | **Steam Cloud.** It is already integrated, it is what the developer tested, and it works with the game's own cloud handling. |
| A game from GOG, Epic or itch.io | **Game Save.** Those stores have no cloud save at all, the saves live in a folder on disk and they are lost with the machine. |
| An indie game on Steam that does not use the Steam Cloud SDK | **Game Save.** Being on Steam does not mean the save is in the cloud: the SDK has to be part of the game. |
| A game that writes somewhere unusual, in a folder you cannot point at | **Game Save**, if you can find the folder. The app needs to know where the saves are. |

In short: Game Save covers the gap Steam Cloud leaves. It does not compete
with it, and running both on the same game is a good way to confuse yourself.

---

## The idea

You point the app at a game's executable and at the folder where that game
stores its saves. The app then:

1. notices when the game is running, and when it stops;
2. at the end of every session it reads the save folder, computes the SHA-256
   of each file and compares it with what it saw last time and with what the
   server holds;
3. uploads what is new or changed, downloads what is newer on the server, and
   asks you what to do when both sides changed.

The folder the game writes to has to be found by looking where the engines
put it: `~/.local/share/<publisher>/<game>` for Unity, `~/.config/unity3d` for
the older Unity builds, `~/Documents/My Games` for the GOG and Epic
installers, `~/.local/share/Steam/steamapps/common` for the games installed
with Steam. **Automatic discovery of that folder is planned**, for now you
point the app at it yourself.

**Your save files are never written to.** The companion only reads them. The
only thing it ever writes is its own state, in its own directory, and copies of
the cloud content in a mirror folder. Restoring a file into the game's save
folder is possible, but only when you explicitly ask for it.

That single rule is what makes the app safe to leave running.

---

## Status

| Component | State |
|---|---|
| `server/` — Go server, API, CLI, TLS, Docker | **done** |
| `desktop/` — Rust companion app (egui) | **in progress** |
| E2E encryption | planned, opt-in per user |
| Chunk level delta upload | planned |

---

## Architecture

```
┌───────────────────────┐        HTTPS, pinned self signed certificate
│  Desktop companion    │◄──────────────────────────────────────────┐
│  Rust + egui          │                                           │
│                       │   ┌─────────────────────────────────────┐  │
│  ┌──────────────────┐ │   │       Game Save server (Go)        │  │
│  │ session watcher  │ │   │                                     │  │
│  │ read only scanner│ │   │  ┌───────────┐  ┌───────────────┐ │  │
│  │ sync engine      │ │   │  │  REST API │  │  sync engine  │ │  │
│  │ egui library UI  │ │   │  └─────┬─────┘  └───────┬───────┘ │  │
│  └──────────────────┘ │   │        │                │         │  │
└───────────────────────┘   │  ┌─────▼─────┐   ┌──────▼───────┐  │
                            │  │  SQLite   │   │  blob store  │  │
                            │  │ (meta)    │   │  (CAS disk)  │  │
                            │  └───────────┘   └──────────────┘  │
                            └─────────────────────────────────────┘
```

The client owns the filesystem, the server owns the bytes. The client never
downloads a whole archive: it diffs manifests and transfers only what changed.

---

## Quick start: the server

```bash
cd server
go build -o gamesave ./cmd/gamesave

# create an account
./gamesave user add alice

# issue an API key (shown once)
./gamesave key create --user alice --label laptop

# run it
./gamesave serve --config config.yaml
```

On first start the server generates a local certificate authority and a server
certificate signed by it, and prints the fingerprint of the authority. Copy
that fingerprint, the client needs it: the client trusts nothing else, so a
server whose authority changed simply stops working instead of being silently
accepted.

With Docker:

```bash
cd server
docker compose up -d
```

Configuration lives in `server/config.example.yaml`; every value can be
overridden with a `GAMESAVE_*` environment variable.

---

## Quick start: the companion app

The companion talks to the server over HTTPS and pins the server authority.
On first run it asks for:

- the server URL, e.g. `https://192.168.1.10:8443`
- the authority fingerprint printed by the server
- an API key

Then add a game: give it the executable and the save folder. Everything else
happens on its own.

The URL must be covered by the subject alternative names of the server
certificate. The server is told which ones to use with `server.tls.san` in the
configuration file; add the address you connect to, for example the IP of the
machine or its hostname.

The client keeps its configuration and its state in the usual places:
`~/.config/gamesave` and `~/.local/share/gamesave` on Linux,
`%APPDATA%\gamesave` and `%LOCALAPPDATA%\gamesave` on Windows. The
`GAMESAVE_HOME` environment variable overrides both, which is handy for a
portable installation.

---

## Repository layout

```
.
├── server/            Go server
│   ├── cmd/gamesave/       CLI entry point
│   └── internal/
│       ├── api/            HTTP layer, auth middleware, handlers
│       ├── auth/           API key issuing and hashing
│       ├── blobstore/      content addressed store on disk
│       ├── config/         YAML + environment configuration
│       ├── db/             SQLite schema, migrations, queries
│       └── tlscert/        the local certificate authority
├── desktop/           Rust companion app (egui)
├── spec/openapi.yaml  the API, described
├── docs/
│   ├── PROTOCOL.md    how the synchronisation works
│   └── DEPLOY.md      how to run the server
└── scripts/e2e.ps1    the end to end test, with a throwaway server
```

---

## Multi user and API keys

The server supports several accounts. Every account has its own games, its own
files and its own keys, and nothing crosses over between accounts.

```
gamesave user add alice
gamesave key create --user alice --label laptop
gamesave key list --user alice
gamesave key revoke --user alice --id 3
```

Keys are stored as a SHA-256 hash. The plaintext is printed once, at creation
time, and cannot be recovered. Revoking a key takes effect immediately.

---

## How the sync works

Every game is a namespace on the server holding a manifest: the current state
of each file, with its size, digest and modification time.

At the end of a session the client walks the save folder **read only**:

1. files whose size and modification time are unchanged since the last scan are
   skipped, their digest is reused. This is what keeps the scan cheap;
2. the remaining files are hashed;
3. the result is compared with the previous state and with the server manifest,
   which gives the three cases:
   - changed locally, unchanged on the server → **upload**;
   - unchanged locally, changed on the server → **download** into the mirror;
   - changed on both sides → **conflict**, you choose;
4. the local state file is rewritten atomically.

The server refuses a blind overwrite: an upload carries the digest the client
believes the server holds, and the server answers `409 Conflict` when it does
not match. Nothing is ever overwritten without you knowing.

---

## Configuration reference

| Variable | Meaning |
|---|---|
| `GAMESAVE_LISTEN` | listen address, default `:8443` |
| `GAMESAVE_DATA_DIR` | directory holding the database, blobs and certificates |
| `GAMESAVE_TLS_MODE` | `selfsigned`, `files` or `off` |
| `GAMESAVE_CERT_FILE` / `GAMESAVE_KEY_FILE` | certificate pair when the mode is `files` |
| `GAMESAVE_SANS` | comma separated subject alternative names |
| `GAMESAVE_MAX_FILE_SIZE` | largest accepted file, in bytes |

---

## Development

```bash
cd server && go test ./... && go vet ./...
cd desktop && cargo check && cargo test
```

The end to end test runs the real client against a real server, both of them
started on the spot:

```powershell
./scripts/e2e.ps1
```

The Go server cross compiles to Linux without a C toolchain, because the SQLite
driver is pure Go:

```bash
cd server
GOOS=linux GOARCH=amd64 go build -o gamesave-linux-amd64 ./cmd/gamesave
GOOS=linux GOARCH=arm64 go build -o gamesave-linux-arm64 ./cmd/gamesave
```

---

## License

Game Save is released under the GNU General Public License v3.0. See
[LICENSE](LICENSE) for the full text.
