# Architecture

This document describes the two halves of Game Save, how they talk to each
other, and what is stored where.

---

## 1. The two halves

```
┌───────────────────────┐        HTTPS, pinned certificate authority
│  Desktop companion    │◄──────────────────────────────────────────┐
│  Rust + egui          │                                           │
│                       │   ┌─────────────────────────────────────┐ │
│  ┌──────────────────┐ │   │       Game Save server (Go)         │ │
│  │ session watcher  │ │   │                                     │ │
│  │ read only scanner│ │   │  ┌───────────┐  ┌───────────────┐   │ │
│  │ sync engine      │ │   │  │  REST API │  │  sync engine  │   │ │
│  │ icons            │ │   │  └─────┬─────┘  └───────┬───────┘   │ │
│  │ theme and widgets│ │   │        │                │           │ │
│  └──────────────────┘ │   │  ┌─────▼─────┐   ┌──────▼───────┐   │ │
└───────────────────────┘   │  │  SQLite   │   │  blob store  │   │ │
                            │  │ (meta)    │   │  (CAS disk)  │   │ │
                            │  └───────────┘   └──────────────┘   │ │
                            └─────────────────────────────────────┘
```

The division is deliberate:

- **the client owns the filesystem.** It reads the save folder of the games,
  it reads the icons of the executables, it owns the interface and the state
  of the local library;
- **the server owns the bytes.** It stores the files, the manifests, the
  history and the accounts. It never writes to the save folder of a game,
  and it does not know where that folder is.

The client never downloads a whole archive: it diffs manifests and transfers
only what changed.

---

## 2. The server

### Modules

| Package | What it does |
|---|---|
| `cmd/gamesave` | the command line: `serve`, `user`, `key`, `gc`, `version` |
| `internal/api` | the HTTP layer: routing, authentication, handlers |
| `internal/auth` | issuing API keys, hashing them, parsing the header |
| `internal/blobstore` | the content addressed store: `ab/cd/<sha256>` |
| `internal/config` | the YAML file and the environment variables |
| `internal/db` | SQLite: connection, migrations, queries |
| `internal/tlscert` | the local certificate authority and the certificates |

### Data model

```
users(id, username, display_name, disabled, created_at)

api_keys(id, user_id, label, key_prefix, key_hash, created_at, last_used_at, revoked_at)
    key_hash is the SHA-256 of the key; the plaintext is never stored

games(id, user_id, name, slug, version, created_at, updated_at)
    id is a random identifier, slug the URL friendly name, unique per user
    version is bumped on every change

game_files(game_id, path, size, sha256, mtime_ns, version, device, updated_at)
    the current state of every file of a game

game_file_versions(id, game_id, path, size, sha256, version, device, created_at)
    the whole history, so a version that loses a conflict is never lost

blobs(sha256, size, created_at)
    the index of the content addressed store
```

Every row that belongs to an account carries the `user_id`, so two accounts
never see each other, not even by mistake.

### Optimistic locking

An upload carries `expect_sha256`: the digest the client believes the server
holds. The server compares it with what it actually holds.

- the same → the upload is accepted, a new version is recorded;
- different → **`409 Conflict`**, nothing is written.

That is the whole conflict detection on the server side. Nothing is ever
overwritten without the client stating what it believes, and the client always
gets an answer it can act on.

### Layout on disk

```
<data_dir>/
├── gamesave.db            SQLite: accounts, keys, games, manifests, history
├── gamesave.db-wal
├── blobs/                 content addressed store, ab/cd/<sha256>
└── certs/
    ├── ca.crt             the local certificate authority, the one to pin
    ├── ca.key
    ├── server.crt         the certificate the server presents
    ├── server.key
    └── fingerprint.txt    the fingerprints to copy into the client
```

### The certificate authority

The server runs its own certificate authority and signs the certificate it
presents. This is what makes the client's pinning work without a public
certificate authority:

- at the first start it generates the authority and the certificate;
- the client fetches the authority once, checks its fingerprint against the
  value the user pinned, and trusts nothing else;
- from then on the client gets ordinary certificate validation, including the
  check that the address it connects to appears in the certificate.

The fingerprints are printed at the start and written to
`certs/fingerprint.txt`.

### Configuration

The file is YAML; every value can be overridden with an environment variable,
which is what the container uses.

| Variable | Meaning |
|---|---|
| `GAMESAVE_LISTEN` | listen address, default `:8443` |
| `GAMESAVE_DATA_DIR` | directory holding the database, the blobs and the certificates |
| `GAMESAVE_TLS_MODE` | `selfsigned`, `files` or `off` |
| `GAMESAVE_CERT_FILE` / `GAMESAVE_KEY_FILE` | certificate pair when the mode is `files` |
| `GAMESAVE_SANS` | comma separated subject alternative names |
| `GAMESAVE_MAX_FILE_SIZE` | largest accepted file, in bytes |

The listen address must appear in the subject alternative names, otherwise the
client refuses the connection: add the address you connect to, for example the
IP of the machine or its hostname.

---

## 3. The client

The companion app is written in Rust with egui, without a webview. The modules
are:

| Module | What it does |
|---|---|
| `app` | the interface: the library of games, the panels, the events |
| `api` | the HTTP client, with the certificate pinning |
| `config` | the configuration, the library, the state, the paths |
| `icons` | the icon of a game, read from its executable |
| `notify` | the notification that says the app is still running |
| `overlay` | the panels that come in the middle of the screen |
| `scanner` | the **read only** walk of the save folder, with hashing |
| `session` | is the game running? |
| `sync` | the synchronisation engine |
| `theme` | fonts, colours and spacing |
| `widgets` | the buttons and the boxes, painted by hand |
| `worker` | the background thread: network and disk work |

### Layout on disk

```
Linux                                  Windows
~/.config/gamesave/config.toml          %APPDATA%\gamesave\config.toml
~/.local/share/gamesave/                %LOCALAPPDATA%\gamesave\
├── library.json                        the library: one row per game
├── state/<slug>.json                   per game manifest and sync state
├── icons/<slug>.icon                   the icon of the game, cached
└── mirror/<slug>/                      copy of the cloud content
    ├── saves/slot1.dat
    └── .conflicts/slot1.dat.<ts>       versions that lost a conflict
```

The `GAMESAVE_HOME` environment variable overrides both directories, which is
handy for a portable installation.

### The rule that keeps it safe

**The save folder of a game is only ever read.** The scanner is the only
module that touches it, and it has no write, rename or delete function. The
only exception is the explicit restore of a cloud version, which the user asks
for with a click.

### The session

A session runs from the moment the game process appears until it goes away. A
crash, a normal exit and a kill are the same event, because the signal is the
process disappearing, not its exit status.

On Linux the process is found by comparing the device and the inode of
`/proc/<pid>/exe` with the registered executable. On Windows the processes are
enumerated and the full path of their image is compared, canonicalised and
lower cased. Both survive a different spelling of the path, spaces in the path
and the short name of a component.

When the session ends the app scans, diffs and applies the plan. The scan is
described in [PROTOCOL.md](PROTOCOL.md).

### The icons

The icon of a game is embedded in its executable as a PE resource group. It is
read with a pure Rust parser, so it works on every platform: a Windows game
played with Wine on Linux gives its icon as well.

The resources are mapped read only, so opening the icon of a hundred megabyte
executable costs nothing, and the result is cached next to the state.

An icon file can be given by hand in the panel that adds a game; when it is
there, it wins over the one of the executable.

### The theme

The font is **Fira Code**, under the Open Font License, and it travels with
the program: the interface looks the same on every machine. The colours are
the ones of the mockup, measured on the picture rather than guessed.

Nothing is rounded. A button under the pointer turns white with black letters,
and the two that destroy something turn red.

---

## 4. Where things are decided

| Decision | Where |
|---|---|
| What to upload, what to download | `desktop/src/sync.rs` |
| Whether the game is running | `desktop/src/session.rs` |
| What the scan sees | `desktop/src/scanner.rs` |
| Whether an upload is accepted | `server/internal/db/files.go` |
| The fingerprints and the certificates | `server/internal/tlscert` |
| The colours and the sizes | `desktop/src/theme.rs` |

The protocol is described in [PROTOCOL.md](PROTOCOL.md), the deployment in
[DEPLOY.md](DEPLOY.md), the interface in [CLIENT.md](CLIENT.md).
