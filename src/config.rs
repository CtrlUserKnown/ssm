//! ssm's own configuration.
//!
//! ssm is a standalone tool: it owns its preferences instead of borrowing them
//! from dots. Config lives next to the session store at `~/.config/ssm/`
//! (overridable with `DOTS_SSM_DIR`, matching [`crate::storage`]), so a user's
//! sessions and settings travel together.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SsmConfig {
    /// Route SSH connections through `herdr --remote` instead of plain `ssh`.
    pub use_herdr: bool,
    /// Name of the active color theme (see [`crate::tui_core::theme`]). "auto"
    /// follows the terminal's ANSI palette.
    pub theme: String,
    /// Whether to run background reachability probes and show latency in the
    /// list (see [`crate::probe`]).
    pub probe: bool,
    /// Require a biometric check (opt-in) before revealing a stored password at
    /// connect time (see [`crate::security`]). Off by default.
    pub biometric_unlock: bool,
}

impl Default for SsmConfig {
    fn default() -> Self {
        Self {
            use_herdr: true,
            theme: "auto".to_string(),
            probe: true,
            biometric_unlock: false,
        }
    }
}

/// `~/.config/ssm/config.toml`, or `$DOTS_SSM_DIR/config.toml` when set.
pub fn config_path() -> PathBuf {
    if let Ok(dir) = std::env::var("DOTS_SSM_DIR") {
        return PathBuf::from(dir).join("config.toml");
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".config/ssm/config.toml")
}

/// Load ssm's config, falling back to defaults if the file is absent or invalid.
pub fn load() -> SsmConfig {
    let path = config_path();
    let Ok(text) = std::fs::read_to_string(&path) else {
        return SsmConfig::default();
    };
    match toml::from_str::<SsmConfig>(&text) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "warning: {} is malformed ({e}); using defaults",
                path.display()
            );
            SsmConfig::default()
        }
    }
}

/// Persist config atomically (write to a temp file, then rename).
pub fn save(cfg: &SsmConfig) -> Result<()> {
    let path = config_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let tmp = path.with_extension("toml.tmp");
    let text = toml::to_string_pretty(cfg).context("serializing ssm config")?;
    std::fs::write(&tmp, &text).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, &path).with_context(|| format!("renaming into {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_uses_herdr() {
        assert!(SsmConfig::default().use_herdr);
    }

    #[test]
    fn roundtrip_parse() {
        let text = toml::to_string_pretty(&SsmConfig {
            use_herdr: false,
            theme: "gruvbox".into(),
            probe: false,
            biometric_unlock: true,
        })
        .unwrap();
        let parsed: SsmConfig = toml::from_str(&text).unwrap();
        assert!(!parsed.use_herdr);
        assert_eq!(parsed.theme, "gruvbox");
        assert!(!parsed.probe);
        assert!(parsed.biometric_unlock);
    }

    #[test]
    fn biometric_defaults_off() {
        let parsed: SsmConfig = toml::from_str("").unwrap();
        assert!(!parsed.biometric_unlock);
    }

    #[test]
    fn probe_defaults_on() {
        let parsed: SsmConfig = toml::from_str("").unwrap();
        assert!(parsed.probe);
    }

    #[test]
    fn empty_parses_to_default() {
        let parsed: SsmConfig = toml::from_str("").unwrap();
        assert!(parsed.use_herdr);
    }
}
