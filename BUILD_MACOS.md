# Building ssm on macOS

These are the steps to build the `ssm` CLI on a macOS machine. ssm can't be
cross-compiled from Linux (its keychain backend links Apple's
`Security.framework`, which needs the macOS SDK), so it's built natively here.

Works on both Apple Silicon (arm64) and Intel (x86_64).

## 1. Prerequisites

```bash
# Apple linker + system frameworks (Security.framework, etc.)
xcode-select --install

# Rust toolchain (skip if you already have rustup)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

No Homebrew packages are required. ssm depends on `keyring`, whose Linux
Secret-Service backend is target-gated *off* on macOS — the native
`apple-native` (Keychain) backend is used instead, so there is no `libdbus` /
OpenSSL / external dependency to install.

macOS already ships OpenSSH (9.x on recent releases), which is new enough
(≥ 8.4) for ssm's `SSH_ASKPASS`-based stored-password hand-off.

## 2. Build

```bash
cd ssm
cargo build --release
# binary: target/release/ssm
```

Quick check:

```bash
./target/release/ssm --version
./target/release/ssm --help
```

## 3. Test

```bash
cargo test
```

The macOS Keychain is available natively, so the keychain-backed storage tests
run for real. The `herdr` connection test is tolerant of `herdr` not being
installed.

## 4. (Optional) Universal binary (arm64 + x86_64)

To ship one binary that runs on both architectures:

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin

lipo -create -output ssm-universal \
  target/aarch64-apple-darwin/release/ssm \
  target/x86_64-apple-darwin/release/ssm

lipo -info ssm-universal   # -> "x86_64 arm64"
```

## 5. (Optional) Code signing — required for the biometric feature

The optional Touch ID unlock binds a stored password to the binary's
code-signing identity. An unsigned or ad-hoc binary gets a new identity on every
rebuild, which would break the biometric keychain item after each update. A
*stable* identity avoids that.

### Personal / local use — free self-signed cert

1. Keychain Access → **Certificate Assistant → Create a Certificate…**
   - Name: `ssm-codesign`
   - Identity Type: **Self Signed Root**
   - Certificate Type: **Code Signing**
2. Sign after each build:

   ```bash
   ./scripts/sign-macos.sh           # signs target/release/ssm as ssm-codesign
   # or: ./scripts/sign-macos.sh ssm-codesign path/to/ssm
   ```

This is not notarized, so it won't distribute cleanly to other machines — fine
for your own use.

### Distribution — Developer ID + notarization

Needs an Apple Developer Program membership ($99/yr) and a **Developer ID
Application** certificate:

```bash
codesign --sign "Developer ID Application: <You> (<TEAMID>)" \
  --options runtime --timestamp target/release/ssm

# Notarize (bare CLIs can't be stapled; wrap in a .pkg/.dmg to staple, or rely
# on Gatekeeper's online check):
ditto -c -k --keepParent target/release/ssm ssm.zip
xcrun notarytool submit ssm.zip \
  --key AuthKey.p8 --key-id <KEYID> --issuer <ISSUER> --wait
```

## Notes on the biometric implementation

As of now the macOS Touch ID verifier (`src/security/mod.rs::macos`) is a stub:
it reports "unavailable", so enabling biometric on macOS currently falls back to
a plain keychain read with a warning. Finishing it means wiring
`LocalAuthentication` (LAContext) and a biometric `SecAccessControl` keychain
item — that work must be done and tested on macOS, and relies on the stable
signing identity from step 5.

## Troubleshooting

- **`xcrun: error: unable to find utility`** → run `xcode-select --install`.
- **linker `ld: framework not found Security`** → Command Line Tools aren't
  installed or are stale; reinstall with `xcode-select --install`.
- **codesign "identity not found"** → the cert name doesn't match; list with
  `security find-identity -v -p codesigning`.
- **A build complains about `dbus`/`libdbus` on macOS** → unexpected; the
  Secret-Service backend should be gated off on macOS. Report it.
