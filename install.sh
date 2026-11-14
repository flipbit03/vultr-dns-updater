#!/bin/sh
# Install vultr-dns: dynamic DNS for Vultr DNS.
#
#   curl -fsSL https://raw.githubusercontent.com/flipbit03/vultr-dns-updater/main/install.sh | sh
#
# Environment:
#   VULTR_DNS_VERSION=v1.2.3   install this tag instead of the latest release
#                              (prerelease tags such as v1.0.0-rc.1 work too)
#   VULTR_DNS_INSTALL_DIR=DIR  install into DIR (default: ~/.local/bin)
#
# The download is checked against the release's .sha256 before anything is
# installed; without sha256sum or shasum the install fails.

# Everything runs from main, called on the last line: a download cut short
# by `curl | sh` defines nothing and runs nothing.
main() {
  set -eu

  REPO="flipbit03/vultr-dns-updater"
  BIN="vultr-dns"
  INSTALL_DIR="${VULTR_DNS_INSTALL_DIR:-$HOME/.local/bin}"

  # Detect OS and architecture.
  OS="$(uname -s)"
  ARCH="$(uname -m)"

  case "${OS}" in
    Linux)  OS_TAG="linux" ;;
    Darwin) OS_TAG="macos" ;;
    *) fail "Unsupported OS: ${OS}" ;;
  esac

  case "${ARCH}" in
    x86_64|amd64)  ARCH_TAG="x86_64" ;;
    aarch64|arm64) ARCH_TAG="aarch64" ;;
    *) fail "Unsupported architecture: ${ARCH}" ;;
  esac

  # macOS x86_64 binaries are not provided.
  if [ "${OS_TAG}" = "macos" ] && [ "${ARCH_TAG}" = "x86_64" ]; then
    fail "macOS x86_64 binaries are not provided. Use: cargo install vultr-dns-updater --locked"
  fi

  ASSET="${BIN}_${OS_TAG}_${ARCH_TAG}"

  # Find a checksum tool before downloading anything.
  if command -v sha256sum >/dev/null 2>&1; then
    SHA_CHECK="sha256sum -c"
  elif command -v shasum >/dev/null 2>&1; then
    SHA_CHECK="shasum -a 256 -c"
  else
    fail "Neither sha256sum nor shasum was found: can't verify the download, not installing."
  fi

  # Which release.
  if [ -n "${VULTR_DNS_VERSION:-}" ]; then
    case "${VULTR_DNS_VERSION}" in
      v*) TAG="${VULTR_DNS_VERSION}" ;;
      *)  TAG="v${VULTR_DNS_VERSION}" ;;
    esac
    echo "Requested release: ${TAG}"
  else
    echo "Fetching latest release..."
    TAG=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | cut -d'"' -f4)
    [ -n "${TAG}" ] || fail "Failed to determine latest release"
    echo "Latest release: ${TAG}"
  fi
  VERSION="${TAG#v}"

  # Download the binary and its checksum.
  URL="https://github.com/${REPO}/releases/download/${TAG}/${ASSET}"
  TMPDIR=$(mktemp -d)
  STAGED="${INSTALL_DIR}/.${BIN}.install-$$"
  trap 'rm -rf "${TMPDIR}"; rm -f "${STAGED}"' EXIT

  echo "Downloading ${ASSET}..."
  curl -fsSL "${URL}" -o "${TMPDIR}/${ASSET}" || fail "Failed to download ${URL}"
  curl -fsSL "${URL}.sha256" -o "${TMPDIR}/${ASSET}.sha256" || fail "Failed to download ${URL}.sha256"

  echo "Verifying checksum..."
  (cd "${TMPDIR}" && ${SHA_CHECK} "${ASSET}.sha256") || fail "Checksum verification failed: not installing."

  # Stage it next to the target (the temp dir may be noexec), check that it
  # runs and is the release asked for, then rename over the target (atomic).
  mkdir -p "${INSTALL_DIR}"
  cp "${TMPDIR}/${ASSET}" "${STAGED}"
  chmod 755 "${STAGED}"
  GOT="$("${STAGED}" --version 2>/dev/null)" || fail "The downloaded binary doesn't run on this machine: not installing."
  [ "${GOT}" = "${BIN} ${VERSION}" ] || fail "The downloaded binary says '${GOT}', expected '${BIN} ${VERSION}': not installing."
  mv -f "${STAGED}" "${INSTALL_DIR}/${BIN}"

  echo "Installed ${BIN} ${TAG} to ${INSTALL_DIR}/${BIN}"

  # Check if install dir is in PATH.
  case ":${PATH}:" in
    *":${INSTALL_DIR}:"*) ;;
    *) echo "Add ${INSTALL_DIR} to your PATH:"; echo "  export PATH=\"${INSTALL_DIR}:\$PATH\"" ;;
  esac
}

fail() {
  echo "$*" >&2
  exit 1
}

main "$@"
