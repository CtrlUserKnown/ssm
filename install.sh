#!/bin/sh
# ssm installer — bootstrap the first install (self-update handles the rest).
#
#   curl -fsSL https://raw.githubusercontent.com/CtrlUserKnown/ssm/main/install.sh | sh
#
# Resolves the same release asset the in-app updater uses: detects this machine's
# Rust target triple, downloads ssm-<target>.tar.gz from the latest GitHub
# Release, verifies its .sha256, extracts the binary, and drops it on PATH.
set -eu

REPO="CtrlUserKnown/ssm"
# Where to install. Override with: PREFIX=/usr/local/bin sh install.sh
PREFIX="${PREFIX:-$HOME/.local/bin}"

err() { printf 'error: %s\n' "$1" >&2; exit 1; }

# ── detect target triple (must match build.yml's matrix) ──────────────────────
os="$(uname -s)"
arch="$(uname -m)"
case "$os" in
  Linux)  vendor_os="unknown-linux-gnu" ;;
  Darwin) vendor_os="apple-darwin" ;;
  *) err "unsupported OS: $os" ;;
esac
case "$arch" in
  x86_64|amd64) cpu="x86_64" ;;
  arm64|aarch64) cpu="aarch64" ;;
  *) err "unsupported architecture: $arch" ;;
esac
target="${cpu}-${vendor_os}"

# ── pick a downloader ─────────────────────────────────────────────────────────
if command -v curl >/dev/null 2>&1; then
  dl() { curl -fsSL "$1" -o "$2"; }
elif command -v wget >/dev/null 2>&1; then
  dl() { wget -qO "$2" "$1"; }
else
  err "need curl or wget to download"
fi

asset="ssm-${target}.tar.gz"
base="https://github.com/${REPO}/releases/latest/download"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

printf 'Downloading %s…\n' "$asset"
dl "${base}/${asset}"        "${tmp}/${asset}"        || err "download failed"
dl "${base}/${asset}.sha256" "${tmp}/${asset}.sha256" || err "checksum download failed"

# ── verify checksum ───────────────────────────────────────────────────────────
printf 'Verifying checksum…\n'
expected="$(awk '{print $1}' "${tmp}/${asset}.sha256")"
if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "${tmp}/${asset}" | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
  actual="$(shasum -a 256 "${tmp}/${asset}" | awk '{print $1}')"
else
  err "need sha256sum or shasum to verify"
fi
[ "$expected" = "$actual" ] || err "checksum mismatch (expected $expected, got $actual)"

# ── extract + install ─────────────────────────────────────────────────────────
tar -xzf "${tmp}/${asset}" -C "$tmp"
bin="${tmp}/ssm-${target}/ssm"
[ -f "$bin" ] || err "binary not found in archive"

mkdir -p "$PREFIX"
install -m 0755 "$bin" "${PREFIX}/ssm" 2>/dev/null || {
  cp "$bin" "${PREFIX}/ssm" && chmod 0755 "${PREFIX}/ssm"
}

# macOS: clear the quarantine bit so Gatekeeper doesn't block the first launch.
if [ "$os" = "Darwin" ]; then
  xattr -d com.apple.quarantine "${PREFIX}/ssm" 2>/dev/null || true
fi

printf '\nInstalled ssm to %s/ssm\n' "$PREFIX"
case ":$PATH:" in
  *":$PREFIX:"*) : ;;
  *) printf 'Note: %s is not on your PATH — add it to your shell profile.\n' "$PREFIX" ;;
esac
