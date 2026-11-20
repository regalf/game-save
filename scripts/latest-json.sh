#!/bin/sh
# Write latest.json, the release description the updater reads.
#
# The server and `gamesave update` read this file from the latest release to
# know the version and the sha256 of every binary. The server binaries are in
# "assets"; the companion app is in "clients".
#
# usage: scripts/latest-json.sh <version> <linux-amd64> <linux-arm64> <windows-amd64.exe> [linux-client [windows-client.exe]]
#
# example:
#   scripts/latest-json.sh 0.3.0 \
#     dist/gamesave-server-linux-amd64 \
#     dist/gamesave-server-linux-arm64 \
#     dist/gamesave-server-windows-amd64.exe \
#     dist/gamesave-client-linux-amd64 \
#     dist/gamesave-client-windows-amd64.exe

set -eu

if [ "$#" -ne 4 ] && [ "$#" -ne 6 ]; then
    echo "usage: $0 <version> <linux-amd64> <linux-arm64> <windows-amd64.exe> [linux-client [windows-client.exe]]" >&2
    exit 1
fi

version=$1
tag="v$version"
linux_amd64=$2
linux_arm64=$3
windows_amd64=$4
linux_client=${5:-}
windows_client=${6:-}

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

# One "key": { "name": ..., "sha256": ... } entry, without the trailing comma.
entry() {
    printf '    "%s": { "name": "%s", "sha256": "%s" }' \
        "$1" "$(basename "$2")" "$(sha "$2")"
}

{
    printf '{\n'
    printf '  "version": "%s",\n' "$version"
    printf '  "tag": "%s",\n' "$tag"
    printf '  "assets": {\n'
    entry "linux-amd64" "$linux_amd64"
    printf ',\n'
    entry "linux-arm64" "$linux_arm64"
    printf ',\n'
    entry "windows-amd64" "$windows_amd64"
    printf '\n  }'

    if [ -n "$linux_client" ]; then
        printf ',\n  "clients": {\n'
        entry "linux-amd64" "$linux_client"
        if [ -n "$windows_client" ]; then
            printf ',\n'
            entry "windows-amd64" "$windows_client"
        fi
        printf '\n  }'
    fi

    printf '\n}\n'
} > latest.json

echo "wrote latest.json for $tag"
cat latest.json
