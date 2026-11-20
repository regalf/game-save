#!/bin/sh
# Game Save installer bootstrap.
#
# It downloads the installer and runs it. The quickstart is:
#
#   curl -fsSL https://github.com/regalf/game-save/releases/latest/download/install.sh | sh
#
# This file is deliberately small: everything that matters is in install.py,
# which travels with the same release. Keeping the two apart means the logic is
# a real file in the repository, and not a heredoc inside a shell one-liner.
#
# The environment variable GAMESAVE_REPO overrides the repository, which is
# handy for a fork.
#
# The repository has to be public: both this script and the server binary are
# downloaded without authentication. For a private repository, make it public
# or point GAMESAVE_REPO at a public fork.

set -eu

REPO="${GAMESAVE_REPO:-regalf/game-save}"
BASE="https://github.com/${REPO}/releases/latest/download"

if ! command -v curl >/dev/null 2>&1; then
    echo "curl is required" >&2
    exit 1
fi
if ! command -v python3 >/dev/null 2>&1; then
    echo "python3 is required" >&2
    exit 1
fi

TMP="$(mktemp "${TMPDIR:-/tmp}/gamesave-install.XXXXXX")"
trap 'rm -f "$TMP"' EXIT INT TERM

echo "==> downloading the Game Save installer"
curl -fsSL "${BASE}/install.py" -o "$TMP"

echo "==> running the installer"
python3 "$TMP" "$@"
status=$?
exit $status
