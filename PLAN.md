# Game Save — development plan

This document is the reference for what Game Save is, how it is built and in
which order the work happens. It is written in English on purpose: the code,
the protocol and the documentation all use it.

---

## 1. Goals

- A self-hosted cloud for game saves, usable on a homelab without any external
  service.
- A companion app that works on Linux first, Windows afterwards, with the same
  code base.
- **Zero risk to the user's save files.** The companion reads them, never
  writes them.
- Honest conflict handling: no silent overwrite, ever.
- Small operational footprint: one binary, one SQLite file, one blob folder.

### Non goals

- Not a Steam Cloud replacement for games that already integrate the Steam
  Cloud SDK. Those games keep talking to Steam.
- Not a general purpose file sync tool. The unit of work is a game and its
  save folder.
- Not a hosted service. There is no multi tenant billing, no public signup.

---

## 2. Decisions taken

| Area | Decision | Why |
|---|---|---|
| Server language | Go | static binary, trivial cross compilation, great concurrency, no runtime dependencies |
| SQLite driver | `modernc.org/sqlite` | pure Go, **no CGO**: cross compiles to Linux/ARM without a C toolchain |
| Desktop language | Rust | small binaries, no runtime, safe filesystem handling |
| Desktop UI | egui (native, `wgpu`) | no webview, one code path for Linux and Windows |
| HTTP | `net/http` with the Go 1.22+ router | no third party router, no dependency |
| TLS | local certificate authority | no certificate authority needed on a LAN, and the client gets real validation |
| Client trust | pinning of the authority by fingerprint | the client never trusts anything it was not told to trust |
| Authentication | API keys, multi user | stateless, revocable, no password to forget |
| Data isolation | every row scoped by `user_id` | accounts never see each other |
| Conflicts | last writer wins **only after asking**, losing version archived | a save is never lost silently |
| Client writes | never inside the game's save folder | protects the user from the tool itself |
| Upload trigger | end of game session | the game is not writing while we read |
| Manifest location | application data directory, outside the save folder | the save folder stays untouched |
| License | GPLv3 | keeps derivatives open |

---

## 3. Layout on disk

### Server

```
<data_dir>/
├── gamesave.db            SQLite: users, keys, games, file metadata, history
├── gamesave.db-wal
├── blobs/                 content addressed store, ab/cd/<sha256>
└── certs/
    ├── ca.crt              the local certificate authority, clients pin it
    ├── ca.key
    ├── server.crt          the certificate the server presents
    ├── server.key
    └── fingerprint.txt     the fingerprints to copy into the client
```

### Client

```
Linux                                  Windows
~/.config/gamesave/config.toml          %APPDATA%\gamesave\config.toml
~/.local/share/gamesave/                %LOCALAPPDATA%\gamesave\
├── library.json                        the library: one row per game
├── state/<slug>.json                   per game manifest and sync state
└── mirror/<slug>/                      copy of the cloud content
    ├── saves/slot1.dat
    └── .conflicts/slot1.dat.<ts>       versions that lost a conflict
```

The game's own save folder is **never** written to.

---

## 4. Server

### Data model

```
users(id, username, display_name, disabled, created_at)
api_keys(id, user_id, label, key_prefix, key_hash, created_at, last_used_at, revoked_at)
games(id, user_id, name, slug, version, created_at, updated_at)
game_files(game_id, path, size, sha256, mtime_ns, version, device, updated_at)
game_file_versions(id, game_id, path, size, sha256, version, device, created_at)
blobs(sha256, size, created_at)
```

`games.version` is a monotonic counter bumped on every mutation; it is the
value the client uses to reason about freshness.

### API

```
GET    /healthz                     public
GET    /v1/cert                     public: the certificate and its fingerprint
GET    /v1/me                       identity behind the key
GET    /v1/games                    list games
POST   /v1/games                    create a game namespace
GET    /v1/games/{id}               read one game
PATCH  /v1/games/{id}               rename
DELETE /v1/games/{id}               delete the game and its files
GET    /v1/games/{id}/manifest      the complete state: the diffing baseline
GET    /v1/games/{id}/files         the file list
GET    /v1/games/{id}/file?path=    download
PUT    /v1/games/{id}/file?path=    upload, with optimistic locking
DELETE /v1/games/{id}/file?path=    delete a file
GET    /v1/games/{id}/file/versions history of one file
```

Authentication is `Authorization: Bearer gsk_...`. The key is hashed with
SHA-256 and looked up; revoked keys and disabled accounts resolve to nothing.

### Optimistic locking

An upload may carry `expect_sha256`, the digest the client believes the server
holds. If it does not match, the server answers `409 Conflict` and changes
nothing. This is what turns "last writer wins" into "last writer wins, but the
loser is told".

### Command line

```
gamesave serve [--config <file>]
gamesave user add|list|disable|enable <username>
gamesave key create|list|revoke --user <name>
gamesave gc                 remove blobs no game references any more
gamesave version
```

### Deployment

- Single binary, `systemd` unit for a NAS or a Raspberry Pi.
- `Dockerfile` with a multi stage build and a distroless style runtime; the
  binary is static because CGO is disabled.
- `docker-compose.yml` with the data directory on a volume.

---

## 5. Client

### Modules

| Module | Responsibility |
|---|---|
| `config` | paths, configuration file, library and state persistence |
| `api` | HTTP client, certificate pinning, typed endpoints |
| `model` | library entries and the per game manifest |
| `scanner` | **read only** walk of the save folder, with hashing |
| `sync` | the diff engine: upload, download, conflict detection |
| `session` | process detection: is the game running |
| `ui` | the egui library table, settings and conflict dialog |

### The scanner

The scanner is the only module that touches the game's save folder, and it
only ever opens files for reading. It applies, in order:

1. the configured exclusions (`*.tmp`, `*.lock`, ...);
2. a fast filter: if size and modification time are unchanged since the last
   scan, the known digest is reused and the file is not read;
3. a stability check: a file whose size or timestamp changes while it is being
   read is retried, and skipped with a warning if it never settles. The game
   may have been writing it.

Timestamps are a cheap filter, never a source of truth. The digest is.

### The session watcher

The unit of work is the game session: from the moment the game process is
visible until it disappears. A crash, a normal exit and a kill are the same
event, because the signal is the process going away, not the exit status.

On Linux the process is found by reading `/proc/<pid>/exe` and comparing the
resolved path with the registered executable, which is robust against
renames, spaces in paths and launcher scripts.

### The sync engine

For every path in the union of the local scan and the server manifest:

| Local changed | Cloud changed | Action |
|---|---|---|
| yes | no | upload |
| no | yes | download into the mirror, notify |
| yes | yes | conflict: ask the user |
| file gone locally | no | offer to delete it on the server |
| no | file gone on the server | upload it again |

Downloads always land in the mirror. Writing into the game's save folder is a
separate, explicit action the user triggers from the interface.

### Interface

The library is a table:

| Game | Executable | Save folder | State | Actions |
|---|---|---|---|---|
| Hollow Knight | `/games/hk/hk` | `~/.local/share/Hollow Knight` | idle | sync, open, remove |

Plus a settings panel (server URL, pinned fingerprint, API key, polling
interval) and a conflict dialog offering *keep the cloud version*, *keep the
local version* and *keep both*.

---

## 6. Security model

- The server runs its own certificate authority and signs the certificate it
  presents. A client fetches the authority once, checks its fingerprint
  against the value the user pinned, and trusts nothing else. The client then
  benefits from ordinary certificate validation, including the check that the
  address it connects to appears in the certificate.
- API keys are stored hashed, are revocable, and are shown once.
- File paths coming from the client are normalised and rejected if they are
  absolute, drive qualified or traverse upwards.
- Uploads are size limited and streamed to disk while hashing, so a large
  file never sits in memory.
- The server is meant to live on a LAN. Put it behind a reverse proxy if it
  has to be reachable from outside.

---

## 7. Roadmap

| Phase | Content | State |
|---|---|---|
| M0 | toolchain, repository skeleton, CI | done |
| M1 | server: config, SQLite, migrations, users, keys, TLS | done |
| M2 | server: games, files, manifest, optimistic locking, history | done |
| M3 | client: configuration, API client, certificate pinning | done |
| M4 | client: library table, scanner, session watcher, sync engine | done |
| M5 | client: conflict dialog, mirror, tray, autostart | in progress |
| M6 | packaging: AppImage, deb, systemd user unit, Windows build | planned |
| M7 | chunk level delta upload for large files | planned |
| M8 | end to end encryption, opt-in per user | planned |

---

## 8. End to end encryption (designed, not implemented)

When it arrives, it will be opt-in per user and compatible with the current
architecture:

- a key derived from a passphrase with Argon2id, held only by the client;
- files encrypted with ChaCha20-Poly1305, one nonce per file version;
- the server stores ciphertext, and with it the salt and the nonce, but it can
  no longer deduplicate or read anything;
- the fingerprint of the certificate and the API key stay in clear.

Consequences to accept: no server side deduplication any more, and a lost
passphrase means lost data. The recovery key printed at setup is the only way
back.

---

## 9. Open items

- Windows support for the session watcher: the process detection is Linux
  specific for now, the Windows implementation is a Win32 process query.
- Automatic discovery of the save folder: heuristics over the usual locations
  per engine, always overridable by hand.
- The client should be able to work without the UI, as a daemon, for a headless
  machine.
