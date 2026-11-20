#!/bin/sh
# Game Save installer bootstrap.
#
# The quickstart is:
#
#   curl -fsSL https://raw.githubusercontent.com/regalf/game-save/main/scripts/install.sh | sh
#
# This file is deliberately small: everything that matters is in install.py,
# which is read from the same repository at the same ref. Neither file is
# attached to a release, so an installer fix does not need a new release.
#
# GAMESAVE_REPO overrides the repository and GAMESAVE_INSTALL_REF the ref
# (default: main). Both are handy for a fork, or to pin a tag.

set -eu

REPO="${GAMESAVE_REPO:-regalf/game-save}"
REF="${GAMESAVE_INSTALL_REF:-main}"
SCRIPT="https://raw.githubusercontent.com/${REPO}/${REF}/scripts/install.py"

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
echo "    ${SCRIPT}"
curl -fsSL "${SCRIPT}" -o "$TMP"

echo "==> running the installer"
status=0
if [ -t 0 ]; then
    # stdin is already a terminal: `sh install.sh`, or a direct run.
    python3 "$TMP" --repo "$REPO" "$@" || status=$?
else
    # `curl | sh` leaves stdin on the pipe, which is at EOF: the installer
    # would read no answer at all. Read the answers from the terminal.
    if ! python3 -c 'import os; os.open("/dev/tty", os.O_RDWR)' 2>/dev/null; then
        echo "==> no terminal to answer the questions; pass --yes to use the defaults" >&2
        exit 1
    fi
    python3 "$TMP" --repo "$REPO" "$@" < /dev/tty || status=$?
fi
exit $status
