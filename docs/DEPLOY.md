# Deploying Game Save

The server is one binary and one data directory. That is the whole thing to
install.

On Linux, the installer does all of it:

```bash
curl -fsSL https://raw.githubusercontent.com/regalf/game-save/main/scripts/install.sh | sh
```

See [INSTALL.md](INSTALL.md).

**Before reading on, read [../SECURITY.md](../SECURITY.md):** the program is
made for a home network, it is not built to resist an adversary, and it must
not be exposed to the internet. The last section of this document says how to
reach it from outside without opening a port.

---

## The binary

```bash
cd server
go build -o gamesave ./cmd/gamesave
```

The result is a static binary: the SQLite driver is pure Go, so CGO is not
needed and the same binary runs on any Linux distribution, on a Raspberry Pi
and on a NAS.

Cross compilation:

```bash
GOOS=linux GOARCH=amd64 go build -o gamesave-linux-amd64 ./cmd/gamesave
GOOS=linux GOARCH=arm64 go build -o gamesave-linux-arm64 ./cmd/gamesave
```

---

## First run

```bash
./gamesave user add alice --config /etc/gamesave/config.yaml
./gamesave key create --user alice --label laptop --config /etc/gamesave/config.yaml
./gamesave serve --config /etc/gamesave/config.yaml
```

The key is printed once. The server prints the fingerprint of its certificate
authority, which the client pins.

The fingerprints are also written to `<data_dir>/certs/fingerprint.txt`:

```
authority : AA:BB:CC:…   ← this is the one the client pins
server    : DD:EE:FF:…   ← the certificate the server presents
```

Copy the **authority** one into the client. If the certificate authority is
regenerated, the fingerprint changes and every client refuses to connect
until it is updated: that is the point of pinning.

---

## Where the data lives

```
<data_dir>/
├── gamesave.db            SQLite: accounts, keys, games, manifests, history
├── gamesave.db-wal
├── blobs/                 the files themselves, content addressed
└── certs/                 the authority, the certificate, the fingerprints
```

Two things follow from that:

- **the backup is the directory.** Copy it while the server is stopped, or
  use a snapshot of the filesystem;
- **the permissions matter.** The directory holds the saves; only the user
  that runs the server should read it.

---

## Storage quota

Every account has a storage quota, **5 GiB by default**. The quota counts the
sum of the sizes of the current files of the account; the same blob referenced
by two games counts twice, because that is what the manifests show.

```bash
gamesave user quota alice --config /etc/gamesave/config.yaml        # show the quota
gamesave user quota alice 10GB --config /etc/gamesave/config.yaml   # 10 GiB
gamesave user quota alice 0 --config /etc/gamesave/config.yaml      # no limit
gamesave user add bob --quota 2GB --config /etc/gamesave/config.yaml # at creation
```

An upload that would pass the quota is refused with `413` and the code
`quota_exceeded`. Replacing a file with a bigger one is judged on the
difference, not on the whole size.

**The server needs at least 6 GB of free storage** for one account with the
default quota: 5 GiB of saves, plus the database and the overhead. Plan for
more if you give more. The units of the command are binary, like the rest of
the program: `5GB` is 5 × 1024³ bytes.

---

## Knowing when a new release is out

The server can look, once in a while, whether a newer release exists:

```yaml
update:
  check: true
  interval_hours: 24
  repo: "regalf/game-save"
```

It reads `latest.json` from the latest release, which holds the version and the
sha256 of every binary. When the latest is newer, the clients are told
and the app shows a notification.

The server **never updates itself**. To update:

```bash
sudo systemctl stop gamesave
gamesave update          # downloads, checks the sha256, replaces the binary
sudo systemctl start gamesave
```

`gamesave update --check` only says what is available. The download is verified
against the sha256 published in the release before the binary is replaced, so a
corrupted or tampered package is refused.

The check is off by default: it is one HTTPS request every `interval_hours` to
GitHub, and it needs the repository to be public.

---

## Reaching it from outside, without opening a port

Opening the port on the router exposes the service to the internet, and the
program is not built for that. Use one of these instead:

| Way | When |
|---|---|
| **Tailscale**, or a similar mesh VPN | the simplest: every machine joins the tailnet and the server listens on its Tailscale address |
| **WireGuard** | one interface, one peer per device; only the WireGuard port is exposed |
| **ZeroTier / Nebula / OpenVPN** | the same idea, a different tool |
| **Reverse SSH tunnel** | if you only need it from your laptop: `ssh -N -R 8443:localhost:8443 user@home` |
| **Reverse proxy with a real certificate** | only if you already have a domain and a proxy; it still exposes the service, so it needs authentication in front of it |

What does **not** count: the port forwarding alone, a firewall rule limited to
some IP addresses, and "nobody will find it on port 8443".

---

## systemd

```ini
# /etc/systemd/system/gamesave.service
[Unit]
Description=Game Save server
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=gamesave
Group=gamesave
ExecStart=/usr/local/bin/gamesave serve --config /etc/gamesave/config.yaml
Restart=on-failure
RestartSec=5
StateDirectory=gamesave
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
NoNewPrivileges=true
ReadWritePaths=/var/lib/gamesave

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl enable --now gamesave
journalctl -u gamesave -f
```

---

## Docker

```bash
cd server
docker compose up -d
docker compose logs -f
```

The compose file keeps the data on a volume. The fingerprint of the authority
is printed at the first start; read it with:

```bash
docker compose exec gamesave cat /data/certs/fingerprint.txt
```

The image is built in two stages and the runtime is small; the binary is
static because CGO is disabled at build time.

---

## Backups

The data directory holds everything. While the server is stopped:

```bash
sudo systemctl stop gamesave
sudo tar czf gamesave-backup-$(date +%F).tar.gz -C /var/lib gamesave
sudo systemctl start gamesave
```

If the server cannot be stopped, take a snapshot of the filesystem (ZFS,
btrfs, LVM) or copy the directory while the server is idle and accept that
the database and the blobs may be a little out of step: the history keeps
every version, so nothing is lost permanently.

**Test the backup.** A backup that has never been restored is a hope, not a
backup.

---

## Housekeeping

```bash
gamesave gc
```

Blobs are never removed on their own: a file deleted from every game does not
free its space immediately. `gc` removes the blobs no game references any
more.

---

## Updating

1. stop the service;
2. back up the data directory;
3. replace the binary;
4. start the service;
5. check that the fingerprint has not changed.

The schema is migrated at the start, in a transaction: an interrupted start
leaves the previous version intact.

---

## The client

The companion app lives on the desktop machines. It is configured with:

- the server URL, for example `https://gamesave-machine:8443`;
- the fingerprint of the authority, printed by the server;
- an API key, one per machine.

Its directories are `~/.config/gamesave` and `~/.local/share/gamesave` on
Linux, `%APPDATA%\gamesave` and `%LOCALAPPDATA%\gamesave` on Windows, and
`GAMESAVE_HOME` overrides both.

See [CLIENT.md](CLIENT.md) for the interface and the state.

---

## Before turning it on

The checklist is in [../SECURITY.md](../SECURITY.md), at the end. Read it.
## Accounts and keys

```bash
gamesave user add bob --config /etc/gamesave/config.yaml
gamesave user list --config /etc/gamesave/config.yaml
gamesave user disable bob          # the keys stop working at once
gamesave user enable bob

gamesave key create --user alice --label laptop --config /etc/gamesave/config.yaml
gamesave key list --user alice --config /etc/gamesave/config.yaml
gamesave key revoke --user alice --id 3 --config /etc/gamesave/config.yaml
```

An installed server reads `/etc/gamesave/config.yaml` when `--config` is not
given, so the commands above work on an installed machine. Passing another
`--config` (or setting `GAMESAVE_DATA_DIR`) points them at a different database,
and a key issued for one database is refused by the other with `401`.

One key per machine is the sensible habit: a key can be revoked without
disturbing the other machines.

The storage quota of an account is in [Storage quota](#storage-quota).

---

## A firewall on the LAN

If the machines of the household are on more than one interface, or the
server shares the network with machines that should not reach it:

```bash
sudo ufw allow from 192.168.1.0/24 to any port 8443
```

Nothing more is needed on a home network. What is **not** enough is
restricting the rule to some addresses and calling the service
protected: see [../SECURITY.md](../SECURITY.md).
