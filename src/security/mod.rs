//! Optional biometric gating for revealing stored SSH passwords.
//!
//! This is strictly opt-in (`biometric_unlock` in [`crate::config`], off by
//! default). When enabled, ssm asks a platform [`Verifier`] to confirm the user
//! before it releases a stored password from the OS keychain at connect time.
//!
//! Platform reality:
//! - **Linux:** shells out to `fprintd-verify` (see [`fprintd`]). This is a
//!   *presence gate* — a fingerprint check in front of the Secret Service
//!   release — not a secret cryptographically bound to the finger.
//! - **macOS:** a stub today (see [`macos`]); a real Touch ID implementation
//!   needs `LocalAuthentication`/Secure-Enclave keychain items and a stably
//!   *code-signed* binary. See `scripts/sign-macos.sh` and the README.
//! - **Other:** no verifier; the option reports unavailable and we fall back.

use anyhow::{bail, Result};

/// A platform mechanism that can confirm the local user is present/authorized.
pub trait Verifier {
    /// Whether this verifier can be used on this machine right now.
    fn available(&self) -> bool;
    /// Prompt for verification. `reason` is a short human string describing why.
    /// Returns `Ok(true)` on success, `Ok(false)` on a clean rejection (e.g. no
    /// match), and `Err` only for unexpected failures.
    fn verify(&self, reason: &str) -> Result<bool>;
}

/// The verifier for the current platform.
pub fn default_verifier() -> Box<dyn Verifier> {
    #[cfg(target_os = "linux")]
    {
        Box::new(fprintd::FprintdVerifier)
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::TouchIdVerifier)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Box::new(NullVerifier)
    }
}

/// A verifier that is never available — used on unsupported platforms.
pub struct NullVerifier;
impl Verifier for NullVerifier {
    fn available(&self) -> bool {
        false
    }
    fn verify(&self, _reason: &str) -> Result<bool> {
        Ok(false)
    }
}

/// Fetch the stored password for `name`, applying the biometric gate when
/// `biometric` is on.
///
/// - Returns `Ok("")` when there is no stored password (key/agent auth, or an
///   interactive prompt) — biometric never blocks a passwordless connection.
/// - When `biometric` is on and a verifier is available, prompts first and
///   returns `Err` if verification fails.
/// - When `biometric` is on but no verifier is available, warns and proceeds
///   (opt-in convenience shouldn't lock a user out of their own hosts).
pub fn reveal(name: &str, biometric: bool) -> Result<String> {
    let Some(password) = crate::storage::kr_load(name) else {
        return Ok(String::new());
    };
    if password.is_empty() {
        return Ok(String::new());
    }
    if biometric {
        let verifier = default_verifier();
        if verifier.available() {
            eprintln!("🔒 Authenticate to unlock the password for '{name}'…");
            if !verifier.verify(&format!("unlock the SSH password for {name}"))? {
                bail!("biometric verification was not successful");
            }
        } else {
            eprintln!(
                "warning: biometric unlock is enabled but no verifier is available on this \
                 platform/build; proceeding without it"
            );
        }
    }
    Ok(password)
}

#[cfg(target_os = "linux")]
pub mod fprintd {
    //! Linux fingerprint verification via the `fprintd-verify` command.
    //!
    //! We deliberately shell out to `fprintd-verify` rather than driving the
    //! `net.reactivated.Fprint` D-Bus API directly: the CLI manages the
    //! `Claim → VerifyStart → VerifyStop → Release` lifecycle for us, which
    //! sidesteps the well-documented "device stays claimed forever" failure
    //! mode when a client forgets to `Release` (e.g. after suspend/resume).

    use super::{Result, Verifier};
    use std::process::{Command, Stdio};

    pub struct FprintdVerifier;

    impl Verifier for FprintdVerifier {
        fn available(&self) -> bool {
            which("fprintd-verify")
        }

        fn verify(&self, _reason: &str) -> Result<bool> {
            // fprintd-verify prints "Verify result: verify-match (done)" and
            // exits 0 on a match, non-zero otherwise. We inherit stdio so the
            // user sees its "place your finger" guidance.
            let status = Command::new("fprintd-verify")
                .status()
                .map_err(|e| anyhow::anyhow!("launching fprintd-verify: {e}"))?;
            Ok(status.success())
        }
    }

    fn which(bin: &str) -> bool {
        Command::new("which")
            .arg(bin)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

#[cfg(target_os = "macos")]
pub mod macos {
    //! macOS Touch ID verification.
    //!
    //! NOT YET IMPLEMENTED. A real implementation authenticates via
    //! `LAContext.evaluatePolicy` (LocalAuthentication) and, for a
    //! cryptographically bound secret, stores the password as a keychain item
    //! with a biometric `SecAccessControl`. Both require the binary to carry a
    //! *stable* code-signing identity so the item's ACL survives rebuilds — a
    //! self-signed cert is enough for local use (see `scripts/sign-macos.sh`),
    //! or a Developer ID + notarization for distribution.
    //!
    //! Until that lands, this verifier reports unavailable so `reveal` falls
    //! back to a plain (un-gated) keychain read with a warning.

    use super::{Result, Verifier};

    pub struct TouchIdVerifier;

    impl Verifier for TouchIdVerifier {
        fn available(&self) -> bool {
            false // TODO: LAContext + signed binary; see module docs.
        }
        fn verify(&self, _reason: &str) -> Result<bool> {
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_verifier_unavailable() {
        let v = NullVerifier;
        assert!(!v.available());
        assert!(!v.verify("x").unwrap());
    }

    #[test]
    fn default_verifier_constructs() {
        // Just ensure the platform selection compiles and returns something.
        let _ = default_verifier();
    }
}
