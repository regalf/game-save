# Installing the server

The fastest way on Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/regalf/game-save/main/scripts/install.sh | sh
```

The script asks how the server should run, writes the configuration, installs a
systemd service and prints the fingerprint the client has to pin.

**The repository has to be public.** The installer is read from the repository
and the server binary from the release, both without authentication. For a
private repository, make it public or point `GAMESAVE_REPO` at a public fork.

The bootstrap reads `install.py` from the same repository and ref, so the
installer is not attached to a release: a fix to it is a push to `main`, not a
new release. Pin a tag with `GAMESAVE_INSTALL_REF=v0.3.0` if you prefer.

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

The installer resolves the latest release itself and downloads the binary for
the architecture it runs on (`amd64` or `arm64`), so the release only has to
carry the binaries.

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
curl -fsSL https://raw.githubusercontent.com/regalf/game-save/main/scripts/install.py -o install.py
sudo python3 install.py --uninstall
```

It stops and disables the service, removes the binary, the configuration and
the unit, and asks whether to remove the data directory.

## Without the bootstrap

The bootstrap only downloads `install.py` and runs it, so the two steps can be
done by hand:

```bash
curl -fsSL https://raw.githubusercontent.com/regalf/game-save/main/scripts/install.py -o install.py
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

---

## Making a release

The installer and the client download the binaries from the release, so the
release has to carry them. The scripts are **not** in the release: they are read
from the repository.

| File | What it is |
|---|---|
| `gamesave-server-linux-amd64` | the server, Linux x86-64 |
| `gamesave-server-linux-arm64` | the server, Linux ARM64 |
| `gamesave-server-windows-amd64.exe` | the server, Windows |
| `gamesave-client-linux-amd64` | the companion app, Linux |
| `gamesave-client-windows-amd64.exe` | the companion app, Windows |

```bash
git tag -a v0.3.0 -m "Game Save 0.3.0"
git push origin v0.3.0
gh release create v0.3.0 --title "Game Save 0.3.0" --notes-file release-notes.md \
  dist/gamesave-server-linux-amd64 \
  dist/gamesave-server-linux-arm64 \
  dist/gamesave-server-windows-amd64.exe \
  dist/gamesave-client-linux-amd64 \
  dist/gamesave-client-windows-amd64.exe
```

The repository has to be public. A fix to the installer is a push to `main`,
not a release.
