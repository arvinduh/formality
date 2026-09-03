#!/bin/sh
# formality (fml) installer for Linux and macOS
# https://github.com/arvinduh/formality
#
# Thin compatibility shim. The real installer is generated and published by
# cargo-dist as a release asset (issue #134). This file stays at
# https://raw.githubusercontent.com/arvinduh/formality/main/install.sh only so
# the one-liner already printed by older `fml` binaries and copied into
# third-party docs keeps working. New docs point straight at the release asset:
#
#   curl --proto '=https' --tlsv1.2 -LsSf \
#     https://github.com/arvinduh/formality/releases/latest/download/fml-installer.sh | sh
#
# Any arguments passed to this script are forwarded to the dist installer
# (e.g. `--to-directory`, `--help`).

set -eu

DIST_INSTALLER_URL="https://github.com/arvinduh/formality/releases/latest/download/fml-installer.sh"

# POSIX sh has no `pipefail`, so `curl ... | sh` would mask a download failure
# (e.g. a 404) behind sh's own exit status. Stage the script to a temp file and
# check the fetch explicitly so any failure exits non-zero with a message.
tmp="$(mktemp 2>/dev/null || mktemp -t 'fml-installer')"
cleanup() { rm -f "$tmp"; }
trap cleanup EXIT INT TERM

if command -v curl >/dev/null 2>&1; then
  if ! curl --proto '=https' --tlsv1.2 -fsSL "$DIST_INSTALLER_URL" -o "$tmp"; then
    echo "error: failed to download the formality installer from $DIST_INSTALLER_URL" >&2
    exit 1
  fi
elif command -v wget >/dev/null 2>&1; then
  if ! wget -O "$tmp" "$DIST_INSTALLER_URL"; then
    echo "error: failed to download the formality installer from $DIST_INSTALLER_URL" >&2
    exit 1
  fi
else
  echo "error: neither curl nor wget was found; install one to continue." >&2
  exit 1
fi

if [ ! -s "$tmp" ]; then
  echo "error: the downloaded installer is empty ($DIST_INSTALLER_URL)" >&2
  exit 1
fi

sh "$tmp" "$@"
