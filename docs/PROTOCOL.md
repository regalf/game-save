# The Game Save protocol

This document describes how a client and a server agree on what to upload and
what to download. It is the contract the two implementations share.

The API is described in [spec/openapi.yaml](../spec/openapi.yaml). This
document describes the parts that matter for the synchronisation: the states,
the diff, the locking and the errors.

---

## 1. The unit of work

A **game** is a namespace on the server. It holds a **manifest**: the current
state of every file, with its size, digest and modification time.

```
game_files(game_id, path, size, sha256, mtime_ns, version, device, updated_at)
```

The manifest is not an archive. It is the state, and it is what the client
diffs against.

Every file also keeps its history:

```
game_file_versions(id, game_id, path, size, sha256, version, device, created_at)
```

The history is what makes a conflict recoverable: the version that lost is
never thrown away.

---

## 2. The three states of a file

On the client, every file of a game is described twice in the state file:

```json
{
  "saves/slot1.dat": {
    "local": {
      "sha256": "3f9a…",
      "size": 12288,
      "mtime_ns": 1732100000000000000,
      "observed_at": "2026-11-15T10:42:00Z"
    },
    "cloud": {
      "sha256": "3f9a…",
      "size": 12288,
      "version": 42,
      "updated_at": "2026-11-15T10:42:00Z",
      "device": "desktop-pc"
    }
  }
}
```

- `local` is what the client last **observed**, read only, in the save folder.
- `cloud` is what the client last **knows** the server holds.

The difference between the two, compared with the fresh observation and the
fresh manifest, is what drives every decision.

---

## 3. The scan

The scan is read only. It never writes inside the save folder.

1. The configured exclusions are applied (`*.tmp`, `*.lock`, ...).
2. The configured sub folders are applied, when there are any.
3. Symlinks are not followed.
4. For every remaining file:
   - if its size and modification time are **unchanged** since the last scan,
     the digest in `local` is reused and the file is not read at all;
   - otherwise the file is hashed;
   - after the hash, the size and the modification time are read again: if
     they changed, the game was writing it, so the file is skipped with a
     warning and picked up at the next scan.

The fourth step matters: a file caught in the middle of being written would
be uploaded truncated.

Timestamps are a cheap filter. The digest is the truth. A timestamp alone
never decides anything, because it changes when a file is copied or restored,
and because two machines rarely agree on the clock.

---

## 4. The diff

For every path in the union of the scan and the server manifest:

| `local` changed | `cloud` changed | Action |
|---|---|---|
| yes | no | **upload** |
| no | yes | **download** into the mirror, and tell the user |
| yes | yes | **conflict**: ask the user |
| the file is gone | no | offer to remove it from the server |
| the file is gone | yes | keep the cloud version in the mirror |

"Changed" means the digest is not the one recorded in the state file.

---

## 5. Uploading

```
PUT /v1/games/{id}/file?path=…&device=…&mtime_ns=…&expect_sha256=…
```

The body is the raw content of the file. `expect_sha256` is the digest the
client believes the server holds.

The server compares it with what it holds:

- the same → the upload is accepted, a new version is recorded;
- different → **`409 Conflict`**, nothing is written.

That is the whole conflict detection on the server side: the client states
what it believes, the server refuses if reality differs. Nothing is ever
overwritten without the client knowing.

The server answers with the new version of the file.

---

## 6. Downloading

```
GET /v1/games/{id}/file?path=…
```

The answer carries `X-File-Sha256` and `X-File-Version`.

Downloads land in the mirror:

```
<data_dir>/mirror/<game>/saves/slot1.dat
```

**Never** in the save folder of the game. Putting a file back into the save
folder is a separate, explicit action, taken by the user when they decide
that the cloud version wins.

---

## 7. Conflicts

A conflict is a file that changed on both sides. The client does not decide:
it shows the two versions and offers three choices.

| Choice | What happens |
|---|---|
| keep the cloud version | the cloud content is written into the save folder, the local state follows |
| keep the local version | the cloud content is archived under `mirror/<game>/.conflicts/`, the local content is uploaded |
| keep both | the cloud content is archived, the local content is uploaded |

In every case nothing is lost: the version that did not win is archived and
the history on the server keeps it too.

---

## 8. The end of a session

A session runs from the moment the game process appears until it goes away.
A crash, a normal exit and a kill are the same event, because the signal is
the process disappearing, not its exit status.

On Linux the process is found by reading `/proc/<pid>/exe` and comparing the
device and the inode with the registered executable. On Windows the processes
are enumerated and the full path of their image is compared, canonicalised and
lower cased. Both survive a different spelling of the path, spaces in the path
and the short name of a component.

When the session ends the client scans, diffs and applies the plan above.

---

## 9. The trust anchor

The server runs its own certificate authority and signs the certificate it
presents.

```
GET /v1/cert
```

The answer holds the authority, in PEM, and its fingerprint. The client:

1. fetches it over a connection whose certificate is not verified yet;
2. computes the SHA-256 of the DER encoding itself;
3. compares it with the fingerprint the user pinned;
4. refuses to go further when they differ;
5. trusts the authority, and only it, for every later connection.

The client never trusts a certificate it was not told to trust, and the
address it connects to still has to appear in the certificate.

---

## 10. The API keys

```
Authorization: Bearer gsk_…
```

The key is hashed with SHA-256 and looked up. A revoked key, a key of a
disabled account and a key that never existed are the same thing: not found.

Keys belong to an account. Every row of the database is scoped by the
account, so two accounts never see each other, not even by mistake.

The key is printed once, at creation time:

```bash
gamesave key create --user alice --label laptop
gamesave key revoke --user alice --id 3
```

---

## 11. The API

The whole surface, with the parameters and the answers, is in
[spec/openapi.yaml](../spec/openapi.yaml). The endpoints, in short:

| Method | Path | What it does |
|---|---|---|
| `GET` | `/healthz` | liveness, no authentication |
| `GET` | `/v1/cert` | the trust anchor, no authentication |
| `GET` | `/v1/me` | the account, the key, the storage and the version of the server |
| `GET` | `/v1/games` | the games of the account |
| `POST` | `/v1/games` | create a namespace |
| `GET` / `PATCH` / `DELETE` | `/v1/games/{id}` | read, rename, delete |
| `GET` | `/v1/games/{id}/manifest` | the baseline the client diffs against |
| `GET` | `/v1/games/{id}/files` | the file list |
| `GET` | `/v1/games/{id}/file?path=` | download a file |
| `PUT` | `/v1/games/{id}/file?path=` | upload a new version |
| `DELETE` | `/v1/games/{id}/file?path=` | remove a file |
| `GET` | `/v1/games/{id}/file/versions` | the history of one file |

Every answer carries a structured error:

```json
{"error": {"code": "conflict", "message": "the file changed on the server…"}}
```

---

## 12. The errors, and what the client does with them

| Code | Meaning | The client |
|---|---|---|
| `401` | the key is unknown, revoked, or the account is disabled | tells the user, offers to reconnect |
| `404` | the game or the file is not there, or belongs to another account | treats it as absent, and offers to create it |
| `409` | the optimistic lock did not match | fetches the manifest again and asks the user |
| `413` | the file is over the configured limit | does not retry, says so |
| `5xx` | the server failed | logs it, tries again at the next session |

The connection itself failing is not an error to hide: the state of the
connection is in the bar at the bottom, and the **Reconnect** button appears
only when it is down.

---

## 13. What the client never does

- It never writes inside the save folder of a game, except when the user
  explicitly asks for a cloud version to be restored.
- It never deletes a file of a game, except when the user turns the option on
  and the game itself removed the file.
- It never overwrites a file on the server without telling the server which
  version it believes is there.
