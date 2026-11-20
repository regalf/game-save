#!/bin/sh
# Write latest.json, the release description the updater reads.
#
# The server and `gamesave update` read this file from the latest release to
# know the version and the sha256 of every server binary.
#
# usage: scripts/latest-json.sh <version> <linux-amd64> <linux-arm64> <windows-amd64.exe>
#
# example:
#   scripts/latest-json.sh 0.3.0 \
#     dist/gamesave-server-linux-amd64 \
#     dist/gamesave-server-linux-arm64 \
#     dist/gamesave-server-windows-amd64.exe

set -eu

if [ "$#" -ne 4 ]; then
    echo "usage: $0 <version> <linux-amd64> <linux-arm64> <windows-amd64.exe>" >&2
    exit 1
fi

version=$1
tag="v$version"
linux_amd64=$2
linux_arm64=$3
windows_amd64=$4

# The sha256 of a file, with the tools a Linux or a Git Bash shell has.
sha() {
    if command -v sha256sum >/dev/null 2>&1; then
        hash=$(sha256sum "$1" | cut -d' ' -f1)
    elif command -v shasum >/dev/null 2>&1; then
        hash=$(shasum -a 256 "$1" | cut -d' ' -f1)
    elif command -v openssl >/dev/null 2>&1; then
        hash=$(openssl dgst -sha256 "$1" | sed 's/.*= //')
    else
        echo "no sha256 tool found (sha256sum, shasum or openssl)" >&2
        exit 1
    fi
    # sha256sum marks a file whose name contains a backslash with a leading
    # backslash, which a Windows path does: drop it.
    printf '%s' "${hash#\\}"
}

cat > latest.json <<EOF
{
  "version": "$version",
  "tag": "$tag",
  "assets": {
    "linux-amd64":   { "name": "$(basename "$linux_amd64")",   "sha256": "$(sha "$linux_amd64")" },
    "linux-arm64":   { "name": "$(basename "$linux_arm64")",   "sha256": "$(sha "$linux_arm64")" },
    "windows-amd64": { "name": "$(basename "$windows_amd64")", "sha256": "$(sha "$windows_amd64")" }
  }
}
EOF

echo "wrote latest.json for $tag"
cat latest.json
