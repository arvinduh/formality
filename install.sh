#!/bin/sh
# formality (fml) installer for Linux and macOS
# https://github.com/arvinduh/formality

set -eu

# Color formatting helpers (disabled if not terminal or NO_COLOR set)
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
  BOLD="\033[1m"
  GREEN="\033[1;32m"
  BLUE="\033[1;34m"
  YELLOW="\033[1;33m"
  RED="\033[1;31m"
  RESET="\033[0m"
else
  BOLD=""
  GREEN=""
  BLUE=""
  YELLOW=""
  RED=""
  RESET=""
fi

info() {
  printf "${BLUE}info:${RESET} %s\n" "$*"
}

success() {
  printf "${GREEN}success:${RESET} %s\n" "$*"
}

warn() {
  printf "${YELLOW}warning:${RESET} %s\n" "$*"
}

error() {
  printf "${RED}error:${RESET} %s\n" "$*" >&2
  exit 1
}

# Detect operating system
OS="$(uname -s)"
case "$OS" in
  Linux)
    PLATFORM="unknown-linux-gnu"
    ;;
  Darwin)
    PLATFORM="apple-darwin"
    ;;
  *)
    error "Unsupported operating system '$OS'. install.sh supports Linux and macOS. For Windows, see install.ps1."
    ;;
esac

# Detect architecture
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64 | amd64)
    ARCH="x86_64"
    ;;
  aarch64 | arm64)
    ARCH="aarch64"
    ;;
  *)
    error "Unsupported architecture '$ARCH'. install.sh supports x86_64 and aarch64 (ARM64)."
    ;;
esac

TARGET="${ARCH}-${PLATFORM}"
ASSET_NAME="fml-${TARGET}.tar.gz"
DOWNLOAD_URL="https://github.com/arvinduh/formality/releases/latest/download/${ASSET_NAME}"
INSTALL_DIR="${FML_INSTALL_DIR:-$HOME/.local/bin}"

info "Detected platform: ${TARGET}"
info "Installing formality into ${INSTALL_DIR}..."

# Prepare temporary directory for download and extraction
TMP_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t 'fml-install')"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

# Download release archive
ARCHIVE_PATH="${TMP_DIR}/${ASSET_NAME}"
if command -v curl >/dev/null 2>&1; then
  curl -fsSL "$DOWNLOAD_URL" -o "$ARCHIVE_PATH" || error "Failed to download $DOWNLOAD_URL using curl."
elif command -v wget >/dev/null 2>&1; then
  wget -q "$DOWNLOAD_URL" -O "$ARCHIVE_PATH" || error "Failed to download $DOWNLOAD_URL using wget."
else
  error "Neither curl nor wget was found. Please install curl or wget to continue."
fi

# Extract binary
tar -xzf "$ARCHIVE_PATH" -C "$TMP_DIR" || error "Failed to extract ${ASSET_NAME}."

if [ ! -f "${TMP_DIR}/fml" ]; then
  error "Extracted archive did not contain the 'fml' binary."
fi

# Install binary to target directory
mkdir -p "$INSTALL_DIR"
mv "${TMP_DIR}/fml" "${INSTALL_DIR}/fml"
chmod +x "${INSTALL_DIR}/fml"

# Verify execution
if ! VERSION_OUTPUT="$("${INSTALL_DIR}/fml" --version 2>&1)"; then
  error "Installed binary at ${INSTALL_DIR}/fml failed to execute."
fi

success "Successfully installed ${BOLD}${VERSION_OUTPUT}${RESET} to ${INSTALL_DIR}/fml"

# Check if INSTALL_DIR is in PATH
PATH_CLEAN=":${PATH}:"
case "$PATH_CLEAN" in
  *":${INSTALL_DIR}:"* | *":${INSTALL_DIR}/:"*)
    # Already in PATH
    ;;
  *)
    printf "\n"
    warn "${INSTALL_DIR} is not in your PATH."
    printf "To make 'fml' accessible from your shell, add %s to your PATH.\n\n" "$INSTALL_DIR"
    printf "Add the following line to your shell profile (%s, %s, or %s):\n\n" "${BOLD}~/.bashrc${RESET}" "${BOLD}~/.zshrc${RESET}" "${BOLD}~/.profile${RESET}"
    printf "  ${BOLD}export PATH=\"%s:\$PATH\"${RESET}\n\n" "$INSTALL_DIR"
    printf "Then restart your terminal or run:\n\n"
    printf "  ${BOLD}export PATH=\"%s:\$PATH\"${RESET}\n\n" "$INSTALL_DIR"
    ;;
esac
