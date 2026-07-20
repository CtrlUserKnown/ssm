#!/usr/bin/env bash
# Sign the ssm binary on macOS with a *stable* code-signing identity.
#
# Why: the optional biometric (Touch ID) unlock binds a stored password to the
# binary's signing identity. An unsigned or ad-hoc binary gets a fresh identity
# on every build, so the biometric keychain item would break after each rebuild.
# A stable identity (self-signed for personal use, or Developer ID to
# distribute) keeps it working across rebuilds.
#
# Usage:
#   scripts/sign-macos.sh [IDENTITY] [BINARY]
#     IDENTITY  code-signing identity name (default: ssm-codesign)
#     BINARY    path to the binary (default: target/release/ssm)
#
# One-time setup of a free self-signed identity (personal/local use only):
#   Keychain Access -> Certificate Assistant -> Create a Certificate...
#     Name: ssm-codesign
#     Identity Type: Self Signed Root
#     Certificate Type: Code Signing
#   (this is NOT notarized, so it won't distribute cleanly to other machines)
#
# For distribution you'd instead use a "Developer ID Application" certificate
# and notarize; see the README.
set -euo pipefail

IDENTITY="${1:-ssm-codesign}"
BINARY="${2:-target/release/ssm}"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "error: this script only applies on macOS (got $(uname -s))." >&2
  exit 1
fi

if [[ ! -f "$BINARY" ]]; then
  echo "error: binary not found at '$BINARY' — build it first (cargo build --release)." >&2
  exit 1
fi

if ! security find-identity -v -p codesigning | grep -q "$IDENTITY"; then
  echo "error: code-signing identity '$IDENTITY' not found in your keychains." >&2
  echo "       Create a self-signed 'Code Signing' certificate named '$IDENTITY'" >&2
  echo "       (see the header of this script), or pass a different identity." >&2
  exit 1
fi

echo "Signing $BINARY as '$IDENTITY' (hardened runtime)…"
codesign --sign "$IDENTITY" --options runtime --force --timestamp=none "$BINARY"

echo "Verifying…"
codesign --verify --strict --verbose=2 "$BINARY"
codesign -dv "$BINARY" 2>&1 | grep -E 'Identifier|Authority|Signature' || true

echo "Done. Re-run this after every 'cargo build' so the signing identity stays stable."
