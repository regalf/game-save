# Deploying Game Save

The server is one binary and one data directory. That is the whole thing to
install.

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
./gamesave user add alice
./gamesave key create --user alice --label laptop
./gamesave serve --config /etc/gamesave/config.yaml
```

The key is printed once. The server prints the fingerprint of its certificate
authority, which the client pins.

---

## Configuration

Copy `server/config.example.yaml` and edit it. Every value can be overridden
with an environment variable, which is handy for containers.

```yaml
server:
  listen: ":8443"
  data_dir: "/var/lib/gamesave"
  tls:
    mode: "selfsigned"
    san:
      - "192.168.1.10"      # the address the client connects to
      - "gamesave.local"    # or the hostname
      - "localhost"
      - "127.0.0.1"
    validity_days: 3650
    fingerprint_file: "certs/fingerprint.txt"
auth:
  require_api_key: true
limits:
  max_file_size: 1073741824
```

The subject alternative names matter: the client connects to an address that
has to appear in the certificate. If the client complains about the
certificate not covering the address, add it to `san` and restart the server.

The first start creates the certificate authority and the certificate:

```
<data_dir>/certs/
├── ca.crt        the authority, the fingerprint to pin
├── ca.key
├── server.crt    the certificate the server presents
├── server.key
└── fingerprint.txt
```

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

The compose file keeps the data on a volume. The fingerprint of the
authority is printed on the first start; read it with:

```bash
docker compose exec gamesave cat /data/certs/fingerprint.txt
```

---

## Accounts and keys

```bash
gamesave user add bob
gamesave user list
gamesave user disable bob          # the keys stop working at once
gamesave user enable bob

gamesave key create --user alice --label laptop
gamesave key list --user alice
gamesave key revoke --user alice --id 3
```

One key per machine is the sensible habit: a key can be revoked without
disturbing the other machines.

---

## Housekeeping

Blobs are never removed on their own, so a file deleted from every game does
not free its space immediately:

```bash
gamesave gc
```

It removes the blobs no game references any more.

---

## Backups

The data directory holds everything: the database and the blobs. Copy it
while the server is stopped, or use the SQLite backup properly if the server
is running:

```bash
sudo systemctl stop gamesave
sudo tar czf gamesave-backup-$(date +%F).tar.gz -C /var/lib gamesave
sudo systemctl start gamesave
```

---

## Reaching it from another machine

The port has to be open on the LAN:

```bash
sudo ufw allow 8443/tcp
```

That is all for a home network. If the server has to be reachable from
outside, put a reverse proxy in front of it and let the proxy terminate TLS
with a real certificate; keep the Game Save port on the LAN only.

Do not expose the port to the internet as it is: the transport is fine, but
the API key is the only thing standing between a stranger and your saves.
