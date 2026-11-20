# Installing the server

The fastest way on Linux:

```bash
curl -fsSL https://github.com/regalf/game-save/releases/latest/download/install.sh | sh
```

The script asks how the server should run, writes the configuration, installs a
systemd service and prints the fingerprint the client has to pin.

**The repository has to be public.** The installer and the server binary are
downloaded without authentication. For a private repository, make it public or
point `GAMESAVE_REPO` at a public fork.

---

## What it does

1. downloads the latest release of the server (or the one you choose with
   `--version`);
2. asks how the server runs: a native binary or a Docker container;
3. writes the configuration (`/etc/gamesave/config.yaml` for the native
   install, `/opt/gamesave/docker-compose.yml` for Docker);
4. installs and starts the systemd service `gamesave`;
5. waits for the server and prints the fingerprint of its certificate
   authority.

The release has to carry `install.sh` and `install.py` as assets, or the
one-liner has nothing to download.

---

## The two ways

| Way | Where it lives | The service runs |
|---|---|---|
| native | `/usr/local/bin/gamesave`, `/etc/gamesave/config.yaml`, `/var/lib/gamesave` | the executable |
| docker | `/opt/gamesave/server`, `/opt/gamesave/docker-compose.yml`, `/var/lib/gamesave` | `docker compose` |

The native install creates a system user named `gamesave`. The Docker install
needs the `docker` command and the compose plugin.

## Configuration

Answer "no" to "Use the default configuration?" to set:

- the listen address (default `:8443`);
- the subject alternative names (default `localhost`, `127.0.0.1`, the hostname
  and the IPv4 addresses of the machine);
- the data directory (default `/var/lib/gamesave`).

Or pass `--listen`, `--san` and `--data-dir` on the command line, and `--yes`
to never be asked.

## Uninstall

```bash
curl -fsSL https://github.com/regalf/game-save/releases/latest/download/install.py -o install.py
sudo python3 install.py --uninstall
```

It stops and disables the service, removes the binary, the configuration and
the unit, and asks whether to remove the data directory.

## Without the bootstrap

The bootstrap only downloads `install.py` and runs it, so the two steps can be
done by hand:

```bash
curl -fsSL https://github.com/regalf/game-save/releases/latest/download/install.py -o install.py
sudo python3 install.py
```

## A dry run

```bash
python3 scripts/install.py --dry-run --yes
```

prints what it would do, without touching the system. It works as a normal
user.

## Windows

The installer is Linux only for now. On Windows the server runs as a plain
executable; see [DEPLOY.md](DEPLOY.md).
