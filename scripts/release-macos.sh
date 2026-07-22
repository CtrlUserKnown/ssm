#!/usr/bin/env bash
# Build, sign, package, and upload the macOS release tarballs for a tag.
#
# GitHub Actions builds the Linux targets on a tag push; macOS is built HERE, on
# your Mac, because we have no Apple Developer account to notarize in CI and the
# stable self-signed identity that keeps biometric unlock working lives only in
# your local keychain. This script produces both darwin arches from one Apple
# Silicon Mac (Xcode ships both SDKs) and attaches them to the same Release.
#
# Prerequisites:
#   - Xcode command-line tools, Rust, and the `gh` CLI (authenticated).
#   - The self-signed code-signing identity `sign-macos.sh` expects
#     (default: ssm-codesign) present in your keychain.
#
# Usage:
#   scripts/release-macos.sh vX.Y.Z [SIGNING_IDENTITY]
#
# Order of operations for a release:
#   1. git tag -a vX.Y.Z -m "…" && git push origin main --follow-tags   (CI builds Linux)
#   2. scripts/release-macos.sh vX.Y.Z                                    (this: uploads macOS)
# The two steps are order-independent — whichever runs second just adds its
# assets to the existing Release.
set -euo pipefail

TAG="${1:?usage: release-macos.sh vX.Y.Z [signing-identity]}"
IDENTITY="${2:-ssm-codesign}"

if [ "$(uname -s)" != "Darwin" ]; then
  echo "error: run this on macOS (got $(uname -s))." >&2
  exit 1
fi

# Resolve the repo root so the script works from anywhere.
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

TARGETS="aarch64-apple-darwin x86_64-apple-darwin"
ARTIFACTS=""

for target in $TARGETS; do
  echo "==> $target: building"
  rustup target add "$target" >/dev/null 2>&1 || true
  # Bake the tag in exactly as CI does (build.rs strips the leading `v`).
  SSM_VERSION="$TAG" cargo build --release --locked --target "$target"

  bin="target/$target/release/ssm"
  echo "==> $target: signing as '$IDENTITY'"
  scripts/sign-macos.sh "$IDENTITY" "$bin"

  # Package identically to build.yml so the updater + install.sh find the binary.
  dist="ssm-$target"
  rm -rf "$dist" "$dist.tar.gz" "$dist.tar.gz.sha256"
  mkdir -p "$dist"
  cp "$bin" "$dist/"
  cp README.md LICENSE "$dist/" 2>/dev/null || true
  tar -czf "$dist.tar.gz" "$dist"
  shasum -a 256 "$dist.tar.gz" > "$dist.tar.gz.sha256"
  rm -rf "$dist"

  ARTIFACTS="$ARTIFACTS $dist.tar.gz $dist.tar.gz.sha256"
done

# Make sure the Release exists (CI may not have run yet), then upload.
if ! gh release view "$TAG" >/dev/null 2>&1; then
  echo "==> creating release $TAG"
  gh release create "$TAG" --title "ssm $TAG" --generate-notes
fi

echo "==> uploading macOS assets to $TAG"
# shellcheck disable=SC2086
gh release upload "$TAG" $ARTIFACTS --clobber

echo "Done. macOS tarballs attached to release $TAG."
