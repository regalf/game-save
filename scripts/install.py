#!/usr/bin/env python3
"""Install the Game Save server on Linux.

The script is interactive: it asks how the server should run (a native binary
or Docker), writes the configuration, installs a systemd service and starts
it, then prints the fingerprint the client has to pin.

It is read from the repository and run by ``install.sh``, which lives at the
same ref:

    curl -fsSL https://raw.githubusercontent.com/regalf/game-save/main/scripts/install.sh | sh

The installer resolves the latest release itself and downloads the server
binary for the architecture it runs on, so nothing has to be attached to the
release except the binaries.

Everything uses the standard library, so there is nothing to pip install.
Windows is not supported yet.
"""

from __future__ import annotations

import argparse
import os
import platform
import re
import shutil
import socket
import ssl
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

REPO = "regalf/game-save"
SERVICE = "gamesave"
USER = "gamesave"

# The native installation.
BIN_PATH = Path("/usr/local/bin/gamesave")
CONFIG_PATH = Path("/etc/gamesave/config.yaml")
UNIT_PATH = Path("/etc/systemd/system/gamesave.service")

# The Docker installation.
OPT_PATH = Path("/opt/gamesave")

DEFAULT_DATA_DIR = "/var/lib/gamesave"
DEFAULT_LISTEN = ":8443"


class Config:
    """What the installer was told, and what it worked out."""

    def __init__(
        self, listen: str, san: str, data_dir: str, port: int, service_user: str
    ) -> None:
        self.listen = listen
        self.san = san
        self.data_dir = data_dir
        self.port = port
        self.service_user = service_user


def main() -> int:
    args = parse_args()

    if platform.system() != "Linux":
        die("this installer is for Linux; Windows is not supported yet")

    if args.uninstall:
        ensure_root()
        return uninstall(args)

    method = args.method or ask_method()
    tag = args.version or latest_tag(args.repo)
    cfg = collect_config(args, method)

    if args.dry_run:
        print()
        print(f"  release:    {tag}")
        print(f"  method:     {method}")
        print(f"  listen:     {cfg.listen}")
        print(f"  san:        {cfg.san}")
        print(f"  data dir:   {cfg.data_dir}")
        if method == "native":
            print(f"  user:       {cfg.service_user}")
        print()
        print("dry run, nothing was done")
        return 0

    ensure_root()
    if method == "docker":
        install_docker(tag, cfg, args)
    else:
        install_native(tag, cfg, args)

    report(cfg)
    return 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Install the Game Save server on Linux.",
    )
    parser.add_argument(
        "--method",
        choices=["native", "docker"],
        help="how the server runs (default: asked)",
    )
    parser.add_argument(
        "--version",
        help="the release to install (default: the latest)",
    )
    parser.add_argument(
        "--repo",
        default=REPO,
        help="the GitHub repository (default: %(default)s)",
    )
    parser.add_argument("--listen", help="the listen address (default: :8443)")
    parser.add_argument("--san", help="comma separated subject alternative names")
    parser.add_argument(
        "--data-dir",
        help="where the saves live (default: /var/lib/gamesave)",
    )
    parser.add_argument(
        "--user",
        help="the user that runs the server, native only (default: asked)",
    )
    parser.add_argument(
        "--yes",
        action="store_true",
        help="never ask, use the defaults",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="print what would be done and exit",
    )
    parser.add_argument(
        "--uninstall",
        action="store_true",
        help="remove the server",
    )
    return parser.parse_args()


def die(message: str) -> None:
    print(f"error: {message}", file=sys.stderr)
    raise SystemExit(1)


def ensure_root() -> None:
    """The installer writes a service and a binary, so it needs root.

    It re-runs itself with sudo, which is what the interactive prompts expect.
    """
    if os.geteuid() == 0:
        return
    if shutil.which("sudo") is None:
        die("run the installer as root, or install sudo")
    print("==> the installer needs root, it will re-run with sudo")
    os.execvp("sudo", ["sudo", sys.executable, os.path.abspath(__file__), *sys.argv[1:]])


def run(cmd: list[str], optional: bool = False) -> None:
    print(f"    $ {' '.join(cmd)}")
    result = subprocess.run(cmd)
    if result.returncode != 0 and not optional:
        die(f"this command failed: {' '.join(cmd)}")


def ask(prompt: str, default: str) -> str:
    answer = input(f"{prompt} [{default}]: ").strip()
    return answer or default


def ask_yes_no(prompt: str, default: bool = True) -> bool:
    suffix = "Y/n" if default else "y/N"
    answer = input(f"{prompt} [{suffix}]: ").strip().lower()
    if not answer:
        return default
    return answer in ("y", "yes", "s", "si")


def ask_method() -> str:
    print()
    print("How should the server run?")
    print("  1) a native binary, started by systemd")
    print("  2) a Docker container, started by systemd")
    while True:
        answer = input("Choose 1 or 2 [1]: ").strip() or "1"
        if answer in ("1", "2"):
            return "native" if answer == "1" else "docker"
        print("please answer 1 or 2")


def latest_tag(repo: str) -> str:
    """The tag of the latest release, from the redirect of /releases/latest."""
    url = f"https://github.com/{repo}/releases/latest"
    request = urllib.request.Request(
        url, method="HEAD", headers={"User-Agent": "gamesave-installer"}
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            final = response.geturl()
    except urllib.error.HTTPError as err:
        if err.code in (401, 403, 404):
            die(
                f"cannot read the latest release of {repo} (HTTP {err.code}). "
                "If the repository is private, the installer cannot download the "
                "release: make it public, or point --repo at a public fork."
            )
        die(f"cannot read the latest release of {repo}: {err}")
    except Exception as err:  # noqa: BLE001 - the message is what matters here
        die(f"cannot read the latest release of {repo}: {err}")
    return final.rsplit("/", 1)[-1]


def detect_arch() -> str:
    machine = platform.machine().lower()
    if machine in ("x86_64", "amd64"):
        return "amd64"
    if machine in ("aarch64", "arm64"):
        return "arm64"
    die(f"unsupported architecture: {machine}")


def default_san() -> str:
    """localhost, the hostname and the IPv4 addresses of the machine."""
    parts = ["localhost", "127.0.0.1"]
    host = socket.gethostname()
    if host:
        parts.append(host)
    try:
        for info in socket.getaddrinfo(host, None, socket.AF_INET):
            address = info[4][0]
            if address not in parts:
                parts.append(address)
    except socket.gaierror:
        pass
    return ",".join(parts)


def parse_port(listen: str) -> int:
    match = re.search(r":(\d+)$", listen)
    return int(match.group(1)) if match else 8443


def user_exists(name: str) -> bool:
    import pwd

    try:
        pwd.getpwnam(name)
        return True
    except KeyError:
        return False


def resolve_service_user(args: argparse.Namespace, method: str) -> str:
    """The user the service runs as.

    Only the native install has one: the Docker container runs as the uid its
    image says, and systemd runs the compose command as root.
    """
    if method == "docker":
        return USER
    if args.user:
        return args.user
    if args.yes:
        return USER

    print()
    print("Which user should run the server?")
    print(f"  1) create a dedicated user (recommended, default: {USER})")
    print("  2) use an existing user")
    while True:
        answer = input("Choose 1 or 2 [1]: ").strip() or "1"
        if answer == "1":
            return ask("Username", USER)
        if answer == "2":
            name = ask("Existing username", "")
            if not name:
                print("please type a username")
            elif not user_exists(name):
                print(f"there is no user named {name} on this machine")
            else:
                return name
            continue
        print("please answer 1 or 2")


def collect_config(args: argparse.Namespace, method: str) -> Config:
    listen = args.listen or DEFAULT_LISTEN
    san = args.san or default_san()
    data_dir = args.data_dir or DEFAULT_DATA_DIR
    service_user = resolve_service_user(args, method)

    # With --yes, or when everything was given on the command line, there is
    # nothing to ask.
    if args.yes or args.listen or args.san or args.data_dir:
        return Config(listen, san, data_dir, parse_port(listen), service_user)

    print()
    print("The default configuration is fine for a home server.")
    if ask_yes_no("Use the default configuration?", True):
        return Config(listen, san, data_dir, parse_port(listen), service_user)

    listen = ask("Listen address", listen)
    san = ask("Subject alternative names (comma separated)", san)
    data_dir = ask("Data directory", data_dir)
    return Config(listen, san, data_dir, parse_port(listen), service_user)


def config_text(cfg: Config) -> str:
    san = "\n".join(
        f'      - "{part.strip()}"' for part in cfg.san.split(",") if part.strip()
    )
    return f"""server:
  listen: "{cfg.listen}"
  data_dir: "{cfg.data_dir}"
  tls:
    mode: "selfsigned"
    san:
{san}
    validity_days: 3650
    fingerprint_file: "certs/fingerprint.txt"
auth:
  require_api_key: true
limits:
  max_file_size: 1073741824
"""


def write_config(path: Path, cfg: Config) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(config_text(cfg))
    print(f"    wrote {path}")


def download(url: str, dest: Path) -> None:
    print(f"    {url}")
    request = urllib.request.Request(
        url, headers={"User-Agent": "gamesave-installer"}
    )
    try:
        with urllib.request.urlopen(request, timeout=300) as response, open(dest, "wb") as out:
            shutil.copyfileobj(response, out)
    except urllib.error.HTTPError as err:
        if err.code in (401, 403, 404):
            die(
                f"cannot download {url} (HTTP {err.code}). "
                "The repository may be private, or the release has no such asset."
            )
        die(f"cannot download {url}: {err}")


def install_native(tag: str, cfg: Config, args: argparse.Namespace) -> None:
    arch = detect_arch()
    name = f"gamesave-server-linux-{arch}"
    url = f"https://github.com/{args.repo}/releases/download/{tag}/{name}"

    print(f"==> downloading the server ({tag}, linux/{arch})")
    with tempfile.NamedTemporaryFile(delete=False) as handle:
        staging = Path(handle.name)
    try:
        download(url, staging)
        staging.chmod(0o755)
        shutil.move(staging, BIN_PATH)
    finally:
        staging.unlink(missing_ok=True)

    if CONFIG_PATH.exists():
        if args.yes or not ask_yes_no("keep the existing configuration?", True):
            write_config(CONFIG_PATH, cfg)
    else:
        write_config(CONFIG_PATH, cfg)

    prepare_service_user(cfg.service_user, cfg.data_dir)
    prepare_data_dir(cfg.data_dir, cfg.service_user)
    write_unit(native_unit(cfg.service_user, cfg.data_dir))
    systemctl("daemon-reload")
    systemctl("enable", "--now", SERVICE)
    wait_for_server(cfg)


def native_unit(user: str, data_dir: str) -> str:
    return f"""[Unit]
Description=Game Save server
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User={user}
Group={user}
ExecStart={BIN_PATH} serve --config {CONFIG_PATH}
Restart=on-failure
RestartSec=5
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
NoNewPrivileges=true
ReadWritePaths={data_dir}

[Install]
WantedBy=multi-user.target
"""


def prepare_service_user(name: str, data_dir: str) -> None:
    """Make sure the user that runs the service exists, creating it if not."""
    if user_exists(name):
        if name == "root":
            print("    warning: running the server as root is not recommended")
        return
    if shutil.which("useradd") is None:
        die(f"useradd is missing and the user {name} does not exist")
    run(
        [
            "useradd",
            "--system",
            "--home-dir",
            data_dir,
            "--shell",
            "/usr/sbin/nologin",
            name,
        ]
    )


def prepare_data_dir(data_dir: str, user: str) -> None:
    """Create the data directory and give it to the service user."""
    import pwd

    path = Path(data_dir)
    path.mkdir(parents=True, exist_ok=True)
    try:
        info = pwd.getpwnam(user)
    except KeyError:
        return
    os.chown(path, info.pw_uid, info.pw_gid)


def prepare_docker_data_dir(data_dir: str) -> None:
    """Create the volume of the container.

    The image runs as uid 10001 (see server/Dockerfile), so the directory that
    is mounted as /data has to be writable by that uid.
    """
    path = Path(data_dir)
    path.mkdir(parents=True, exist_ok=True)
    os.chown(path, 10001, 10001)


def compose_command() -> list[str]:
    if shutil.which("docker") is None:
        die("docker is required for the docker installation")
    if subprocess.run(
        ["docker", "compose", "version"], capture_output=True
    ).returncode == 0:
        return ["docker", "compose"]
    if shutil.which("docker-compose"):
        return ["docker-compose"]
    die("the docker compose plugin is required")


def install_docker(tag: str, cfg: Config, args: argparse.Namespace) -> None:
    compose = compose_command()

    # The image is built from the Dockerfile of the server, so the source of
    # the release is what the installer needs.
    print(f"==> downloading the source of {tag} to build the image")
    OPT_PATH.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        tarball = Path(tmp) / "source.tar.gz"
        download(f"https://github.com/{args.repo}/archive/refs/tags/{tag}.tar.gz", tarball)
        with tarfile.open(tarball) as archive:
            try:
                archive.extractall(tmp, filter="data")
            except TypeError:  # Python before 3.12 has no filter
                archive.extractall(tmp)
        candidates = list(Path(tmp).glob("*/server"))
        if not candidates:
            die("the source archive does not contain the server directory")
        source = candidates[0]
        target = OPT_PATH / "server"
        if target.exists():
            shutil.rmtree(target)
        shutil.copytree(source, target)

    prepare_docker_data_dir(cfg.data_dir)
    write_compose(cfg, tag)
    write_unit(docker_unit(compose))
    systemctl("daemon-reload")
    systemctl("enable", "--now", SERVICE)
    wait_for_server(cfg)


def write_compose(cfg: Config, tag: str) -> None:
    text = f"""services:
  gamesave:
    build:
      context: ./server
      args:
        VERSION: "{tag}"
    image: gamesave:latest
    container_name: gamesave
    restart: unless-stopped
    ports:
      - "{cfg.port}:8443"
    volumes:
      - {cfg.data_dir}:/data
    environment:
      GAMESAVE_LISTEN: ":8443"
      GAMESAVE_DATA_DIR: "/data"
      GAMESAVE_TLS_MODE: "selfsigned"
      GAMESAVE_SANS: "{cfg.san}"
    healthcheck:
      test: ["CMD", "wget", "--spider", "-q", "--no-check-certificate", "https://127.0.0.1:8443/healthz"]
      interval: 30s
      timeout: 5s
      retries: 3
"""
    (OPT_PATH / "docker-compose.yml").write_text(text)
    print(f"    wrote {OPT_PATH / 'docker-compose.yml'}")


def docker_unit(compose: list[str]) -> str:
    up = " ".join([*compose, "up", "-d", "--build"])
    down = " ".join([*compose, "down"])
    return f"""[Unit]
Description=Game Save server (Docker)
After=docker.service network-online.target
Requires=docker.service
Wants=network-online.target

[Service]
Type=oneshot
RemainAfterExit=yes
WorkingDirectory={OPT_PATH}
ExecStart={up}
ExecStop={down}

[Install]
WantedBy=multi-user.target
"""


def write_unit(text: str) -> None:
    UNIT_PATH.write_text(text)
    print(f"    wrote {UNIT_PATH}")


def systemctl(*args: str) -> None:
    run(["systemctl", *args])


def wait_for_server(cfg: Config, timeout: int = 60) -> None:
    url = f"https://127.0.0.1:{cfg.port}/healthz"
    # The server presents a certificate signed by its own authority, so the
    # fingerprint is pinned in the client instead of the system roots here.
    context = ssl.create_default_context()
    context.check_hostname = False
    context.verify_mode = ssl.CERT_NONE
    print("==> waiting for the server to answer")
    for _ in range(timeout * 2):
        try:
            with urllib.request.urlopen(url, context=context, timeout=2) as response:
                if response.status == 200:
                    return
        except Exception:  # noqa: BLE001 - it is normal to fail while starting
            pass
        time.sleep(0.5)
    print("    the server did not answer; check: systemctl status gamesave")


def read_fingerprint(data_dir: str) -> str | None:
    path = Path(data_dir) / "certs" / "fingerprint.txt"
    try:
        text = path.read_text()
    except FileNotFoundError:
        return None
    for line in text.splitlines():
        if line.lower().startswith("authority"):
            return line.split(":", 1)[1].strip()
    return None


def report(cfg: Config) -> None:
    fingerprint = read_fingerprint(cfg.data_dir)
    print()
    print("  Game Save is up.")
    print(f"  URL:  https://<this-machine>:{cfg.port}")
    if fingerprint:
        print("  Pin this fingerprint in the client:")
        print(f"    {fingerprint}")
    else:
        path = Path(cfg.data_dir) / "certs" / "fingerprint.txt"
        print(f"  The fingerprint is in {path}")
    print()
    print("  systemctl status gamesave")
    print()


def uninstall(args: argparse.Namespace) -> int:
    run(["systemctl", "stop", SERVICE], optional=True)
    run(["systemctl", "disable", SERVICE], optional=True)
    if UNIT_PATH.exists():
        UNIT_PATH.unlink()

    if args.method == "docker" or (not args.method and OPT_PATH.exists()):
        shutil.rmtree(OPT_PATH, ignore_errors=True)
    else:
        BIN_PATH.unlink(missing_ok=True)
        CONFIG_PATH.unlink(missing_ok=True)

    run(["systemctl", "daemon-reload"], optional=True)

    data_dir = args.data_dir or DEFAULT_DATA_DIR
    if ask_yes_no(f"remove the data directory {data_dir}?", False):
        shutil.rmtree(data_dir, ignore_errors=True)

    print("Game Save was removed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
