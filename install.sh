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

if command -v curl >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -LsSf "$DIST_INSTALLER_URL" | sh -s -- "$@"
elif command -v wget >/dev/null 2>&1; then
  wget -qO- "$DIST_INSTALLER_URL" | sh -s -- "$@"
else
  echo "error: neither curl nor wget was found; install one to continue." >&2
  exit 1
fi
