# Decisions

The structural decisions Game Save is built on, and why.

Every one of them was taken for a reason, and most of them have a cost. This
document keeps both halves, so that nobody has to re-litigate a choice and so
that a future contributor knows what is a constraint and what is a habit.

A decision here is not a law: if the reason stops being true, the decision
should change. Say so in an issue, with the reason.

---

## 1. The server

### Go

**Decision:** the server is written in Go.

**Why:** one static binary, no runtime to install, cross compilation to
Linux and ARM in one command, and concurrency that comes for free when several
machines upload at the same time.

**Cost:** the language is deliberately boring. Metaprogramming, fancy type
systems and a large ecosystem of frameworks are not available, and nobody
misses them for a service this size.

### SQLite with a pure Go driver

**Decision:** the database is SQLite, through `modernc.org/sqlite`.

**Why:** the driver is pure Go, so **CGO is not needed**. That means the
server cross compiles to Linux and ARM without a C toolchain, and it runs in a
container without `gcc`. A single file, a single process, no database to
install or administer.

**Cost:** the driver is large, and SQLite is slower than PostgreSQL for some
workloads. At the scale of a household, the difference is not measurable.

### A content addressed blob store

**Decision:** the files are stored by digest, under `blobs/ab/cd/<sha256>`.

**Why:** identical content is stored once, whatever the game or the account.
Backups are a copy of a directory, and a file that comes back is recognised
without reading it.

**Cost:** deleting a file does not free the space until `gamesave gc` runs.
That is a choice, not an oversight: the space is cheap and the history is not.

### The certificate authority

**Decision:** the server runs its own certificate authority and signs the
certificate it presents.

**Why:** no public certificate authority, no cost, no renewal. And it gives the
client something to pin: the client trusts **only** the authority whose
fingerprint the user copied, so a server whose authority changed simply stops
working instead of being silently accepted.

**Cost:** the fingerprint has to travel out of band, from the server console
to the client. That is one copy-paste, and it is the price of not having a
certificate authority involved.

### Optimistic locking

**Decision:** an upload states the digest it believes the server holds; the
server refuses with `409 Conflict` when that is not true.

**Why:** the server cannot know what the client knows. Stating it makes the
conflict explicit instead of silently overwriting a file that changed
elsewhere.

**Cost:** the client has to handle a `409`. It does, by fetching the manifest
again and asking the user.

### The history of every version

**Decision:** every version of every file is kept, not only the current one.

**Why:** a conflict is recoverable. The version that lost is never thrown
away, and the server can show who changed what and when.

**Cost:** the database grows. For game saves, slowly; and `gc` takes care of
the blobs.

### API keys, one per machine

**Decision:** authentication is an API key per device, hashed with SHA-256 and
shown once.

**Why:** there is no password to forget, no session to expire, no account
recovery. A key is revoked and it stops working immediately.

**Cost:** a key that leaks is as good as the account until it is revoked. One
key per machine keeps the blast radius small.

---

## 2. The client

### Rust, with egui, without a webview

**Decision:** the companion app is Rust with egui, and it has no webview.

**Why:** small binaries, no runtime, and filesystem handling that the compiler
checks. egui gives a native tray icon and no browser engine to ship.

**Cost:** the interface is drawn, not native. The controls are not the ones of
the operating system, so the look is coherent everywhere and native nowhere.
For a utility that is the right trade.

### Fira Code travels with the program

**Decision:** the font is bundled, under the SIL Open Font License.

**Why:** the interface looks the same on every machine, whatever the system has
installed. A missing font changes the whole rhythm of a dense interface.

**Cost:** five megabytes in the binary, for two weights.

### The client owns the filesystem, the server owns the bytes

**Decision:** the client reads the save folders and writes its own state; the
server stores the files.

**Why:** it divides the responsibilities along the trust line. The server never
knows where a game keeps its saves, and the client never keeps a second copy
of the cloud content outside its own mirror.

**Cost:** the two halves have to agree on the protocol, and they do.

### The save folder is only ever read

**Decision:** the app never writes, moves or deletes anything inside the save
folder of a game. The only exception is an explicit restore.

**Why:** it is the rule that makes the app safe to leave running. A bug, a
wrong configuration, a crash cannot damage a save.

**Cost:** a game that writes its saves somewhere the user cannot point at
cannot be followed. That is accepted: the app needs to know the folder.

### The session is a process, not a window

**Decision:** a session opens when the registered executable appears and closes
when it goes away.

**Why:** a crash, a normal exit and a kill are the same event, and the signal is
the process disappearing, not its exit status. Reading the saves halfway
through a write would upload a truncated file.

**Cost:** the detection is platform specific. Today it is Linux, through the
device and the inode of `/proc/<pid>/exe`. Windows is planned.

### Sync at the end of the session, not while it runs

**Decision:** nothing is uploaded while the game is running.

**Why:** the game may be writing. Reading it halfway is worse than reading it
late.

**Cost:** if the machine loses power during a session, the last minutes are not
in the cloud. Accepted: the session is usually minutes, and the alternative is
corruption.

### The digest is the truth, the timestamp is a filter

**Decision:** the scan skips a file whose size and modification time did not
change; everything else is hashed.

**Why:** hashing a whole save folder at every session is waste. The timestamp
is a cheap filter, not a source of truth: it changes when a file is copied or
restored, and the clocks of two machines rarely agree.

**Cost:** a file touched without changing size and mtime is not noticed. In
practice the game rewrites the file and the timestamp moves.

### The conflicts are decided by the user

**Decision:** when both sides changed, the app asks. Three choices, and the
loser is archived.

**Why:** last-writer-wins is what the other services do, and it is how saves
get lost silently. Here the user knows there was a divergence and chooses.

**Cost:** the sync stops and waits. For a small number of files, that is fine.

---

## 3. The project

### Not for commercial use, not for the internet

**Decision:** the program is made for one household, on a home network, and it
is not built to resist an adversary.

**Why:** the alternative is a different program: rate limiting, account
lockout, encryption at rest, a security team. None of that is here, and saying
it out loud is better than letting somebody find out.

**Cost:** it must never be exposed to the public internet. The ways to reach it
from outside are in [DEPLOY.md](docs/DEPLOY.md), and the reasons in
[SECURITY.md](SECURITY.md).

### GPLv3

**Decision:** the project is under the GPLv3.

**Why:** a derivative stays open. The font is under the SIL Open Font License,
which is compatible with the GPL, and the copies are credited in
`desktop/assets/fonts/`.

### One issue, one problem; one pull request, one fix

**Decision:** an issue describes one problem, a pull request fixes one issue.

**Why:** a change that touches two things is harder to review, and a revert of
one of them loses the other.

**Cost:** more issues, more pull requests, more linking. Cheaper than the
alternative.

### Say when an AI agent helped

**Decision:** the description of an issue or a pull request that was written
with the help of an AI agent carries one line at the end, in a fixed form.

**Why:** to know how much of the project is written with these tools, and to be
able to ask better questions about how the code was made.

**Cost:** one line. The code written by an agent follows the same rules as the
rest: it is read, understood, and answered for.

---

## 4. Deferred, with the reason

| Thing | Why not now | What it would cost |
|---|---|---|
| **End to end encryption** | the server deduplicates by digest, and encryption makes every file unique; the recovery of a lost passphrase is unsolved | the deduplication is lost, and a recovery key is designed |
| **Chunk level delta upload** | whole files are enough for game saves, and the first versions of a save are the ones that grow | a chunker, a chunk store, a second protocol |
| **Windows session watcher** | the Linux detection is through `/proc`, and the Windows one needs the Win32 process enumeration | a platform module and its tests |
| **Automatic discovery of the save folder** | the engines put them in different places, and a wrong guess is worse than a question | heuristics per engine, and a confirmation |
| **A daemon, without the interface** | the tray is enough for a household; a headless machine can run the interface | a second entry point, a second configuration |

---

## 5. Roadmap

| Phase | Content | State |
|---|---|---|
| M0 | toolchain, repository skeleton, CI | done |
| M1 | server: config, SQLite, migrations, users, keys, TLS | done |
| M2 | server: games, files, manifest, optimistic locking, history | done |
| M3 | client: configuration, API client, certificate pinning | done |
| M4 | client: library of games, scanner, session watcher, sync engine | done |
| M5 | client: conflict dialog, mirror, tray, autostart, icons, theme | done |
| M6 | packaging: AppImage, deb, systemd user unit, Windows build | planned |
| M7 | chunk level delta upload for large files | planned |
| M8 | end to end encryption, opt-in per user | planned |
| M9 | session watcher on Windows | planned |

---

## 6. Open items

- The client is Linux-first. On Windows the interface works but the sync does
  not start on its own, because the session watcher is Linux only.
- The save folder is given by hand. Automatic discovery is planned.
- The API has no rate limiting and no account lockout: see
  [SECURITY.md](SECURITY.md) for why that is acceptable and what it is not.
- The files are not encrypted at rest. The design of the end to end encryption
  is in section 4.
