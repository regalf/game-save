# The companion app

The companion app is the half that lives on the desktop. It watches the games
of the library and keeps their saves in sync with the server.

---

## 1. The screen

```
┌──────────────────────────────────────────────────────────────┐
│ GameSave          │        [Sync all] [Add game] [Settings] │
├──────────────────────────────────────────────────────────────┤
│ ┌──────────────────┐ ┌──────────────────┐ ┌────────────────┐  │
│ │ Hollow Knight    │ │ Celeste          │ │ Stardew Valley │  │
│ │ Last sync: never │ │ Last sync: never │ │ Last sync: ... │  │
│ │ ┌──────────────┐ │ │ ┌──────────────┐ │ │ ┌────────────┐ │  │
│ │ │     (icon)   │ │ │ │     (icon)   │ │ │ │   (icon)   │ │  │
│ │ └──────────────┘ │ │ └──────────────┘ │ │ └────────────┘ │  │
│ │ [     Sync     ] │ │ [     Sync     ] │ │ [    Sync    ] │  │
│ │ [Delete] [Edit]  │ │ [Delete] [Edit]  │ │ [Delete][Edit]│  │
│ └──────────────────┘ └──────────────────┘ └────────────────┘  │
│ ┌──────────────────┐                                          │
│ │        +         │   ← the last card adds a game            │
│ │    Add game      │                                          │
│ └──────────────────┘                                          │
├──────────────────────────────────────────────────────────────┤
│ connected to https://… as alice (key laptop)   [Reconnect]   │
└──────────────────────────────────────────────────────────────┘
```

The library is a grid. Every card is a game; the last one of the grid adds a
game. The grid fills the rows, and the number of columns follows the width of
the window.

---

## 2. The cards

Each card holds:

- the name of the game, and when it was last synchronised;
- the icon of the game, in a square box in the middle;
- **Sync**, the whole width of the card;
- **Delete** and **Edit**, half each.

Delete is red, because it is the only destructive gesture: it removes the game
from the library and its namespace from the server. It is the only button that
turns red under the pointer, together with **Default** in the settings.

Edit opens the panel that adds a game, filled with what is already known, and
its title says **Edit a game**. One panel, two jobs.

---

## 3. The panels

The panels that add a game and that hold the settings do not float: they come
in the middle of the screen with a quick fade, and they stay where they are.
They are closed with the cross in their corner, or with a click on the dimmed
background.

The header has the title, a vertical line after it, and the cross on the right.
The line under the header goes from one edge of the panel to the other.

**Add a game** asks for:

| Field | Notes |
|---|---|
| Name | mandatory |
| Executable | where the game runs, and where the icon comes from |
| Save Folder | the folder the game writes to, **read only** |
| SubFolder to sync | empty means the whole folder |
| Exclusion | glob patterns, comma separated |
| Game icon (Optional) | an icon file, when the executable has none or has the wrong one |

If the name is missing or the save folder is empty, the reason is written in
red above the Save button. The button is the whole width, and it is the only
one: the cross closes the panel.

**Settings** holds the server URL, the certificate fingerprint, the API key,
the device name, the poll interval, and four switches:

- **Synchronise when game close** — the sync happens at the end of a session;
- **Remove from server the deleted game files** — when the game deletes a file,
  the server deletes it too;
- **Start with session** — the app starts with the desktop session;
- **Enable logs** — the log goes to a file, next to the state.

At the bottom: **Default** (red, back to the built-in values) and **Save**.

---

## 4. The session

The app polls the processes every *poll interval* seconds. When the registered
executable appears, a session opens. When it disappears, the session closes
and the sync runs.

A crash closes the session exactly like a normal exit, because what is looked
at is the process going away.

While the session is open nothing is uploaded: the game may be writing the
saves, and reading them halfway would upload a truncated file.

---

## 5. The sync

At the end of the session:

1. the save folder is walked **read only**;
2. files whose size and modification time did not change are skipped, their
   digest is reused. This is what keeps the scan cheap;
3. the remaining files are hashed;
4. the result is compared with the local state and with the server manifest:

   | local | server | action |
   |---|---|---|
   | changed | unchanged | **upload** |
   | unchanged | changed | **download** into the mirror |
   | changed | changed | **conflict**, you choose |
   | gone | unchanged | offered for deletion on the server |

5. the local state is rewritten atomically.

The digests, the diff, the conflicts and the API are described in
[PROTOCOL.md](PROTOCOL.md).

---

## 6. The conflicts

When a file changed on both sides, the app does not decide: it shows the two
versions, the local one and the cloud one, and offers three choices.

- **Keep the cloud version** — the cloud file is written into the save folder.
  This is the only time the app writes to the save folder, and it is an
  explicit request.
- **Keep the local version** — the cloud file is archived under
  `mirror/<game>/.conflicts/`, and the local file is uploaded.
- **Keep both** — the cloud file is archived, the local file is uploaded.

In every case nothing is lost: the loser is archived, and the server keeps the
history of every version.

---

## 7. The mirror

Downloads never land in the save folder of the game. They land in
`mirror/<game>/`, a copy of the cloud content, so the state can be inspected
without touching the game.

The versions that lost a conflict are kept in `mirror/<game>/.conflicts/`,
with the time in the name.

---

## 8. The icons

The icon of a game is embedded in its executable, as a PE resource group:

```
IconExtractor::from_path(exe)      ← maps the file read only
  → get_icon(Index(0))             ← the first group
  → to_ico_bytes()
  → oxideav_ico::decode_all()      ← every resolution, BMP and PNG
  → to_rgba8()                     ← pixels
```

The pure Rust parser means it works on every platform: a Windows game played
with Wine on Linux gives its icon as well.

The app picks the resolution closest to the wanted size, keeping the
proportions, and caches it in `icons/<slug>.icon`. When the user points at an
icon file, that file is decoded directly and it wins over the executable.

Not every executable carries an icon: a shell library, for one, has none. The
card shows a placeholder then.

---

## 9. The state

- `library.json` — the games of this machine: name, executable, save folder,
  subfolders, exclusions, the icon, the namespace on the server;
- `state/<slug>.json` — per game: what the local side looked like at the last
  sync, what the server holds, and the version of the namespace;
- `icons/<slug>.icon` — the cached icon, raw RGBA with a small header;
- `mirror/<slug>/` — the cloud content, and the conflicts that lost.

---

## 10. The tray

The icon sits in the tray with the state of the connection. The menu offers
**Sync now**, **Show Game Save** and **Quit**.

---

## 11. Starting with the session

The app can register itself to start with the desktop session:

- **Linux**: an entry in the XDG autostart directory;
- **Windows**: the run key of the current user.

The API key is never written to disk in clear: it stays in the configuration
file of the user, as the configuration file itself is the private thing.

---

## 12. Development

```bash
cd desktop
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The end to end test runs the real client against a real server, both started on
the spot:

```powershell
./scripts/e2e.ps1
```

The whole architecture is in [ARCHITECTURE.md](ARCHITECTURE.md), the protocol
in [PROTOCOL.md](PROTOCOL.md).
