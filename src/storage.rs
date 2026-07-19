use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub name: String,
    pub host: String,
    pub user: String,
    pub port: u16,
    #[serde(skip)]
    pub password: String,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            name:     String::new(),
            host:     String::new(),
            user:     String::new(),
            port:     22,
            password: String::new(),
        }
    }
}

// ── paths ─────────────────────────────────────────────────────────────────────

pub fn sessions_path() -> PathBuf {
    if let Ok(dir) = std::env::var("DOTS_SSM_DIR") {
        return PathBuf::from(dir).join("sessions.json");
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".config/ssm/sessions.json")
}

// ── keychain ──────────────────────────────────────────────────────────────────

const SERVICE: &str = "dots-ssm";

pub fn keychain_available() -> bool {
    let Ok(e) = keyring::Entry::new(SERVICE, "__probe__") else { return false };
    match e.get_password() {
        Err(keyring::Error::NoStorageAccess(_)) => false,
        _ => true,
    }
}

pub fn kr_store(name: &str, password: &str) -> Result<()> {
    let e = keyring::Entry::new(SERVICE, name).context("keyring entry")?;
    e.set_password(password).context("keyring set_password")?;
    Ok(())
}

pub fn kr_load(name: &str) -> Option<String> {
    let e = keyring::Entry::new(SERVICE, name).ok()?;
    e.get_password().ok()
}

pub fn kr_delete(name: &str) {
    if let Ok(e) = keyring::Entry::new(SERVICE, name) {
        let _ = e.delete_credential();
    }
}

// ── load / save ───────────────────────────────────────────────────────────────

pub fn load_sessions() -> Result<Vec<Session>> {
    let path = sessions_path();
    load_sessions_from(&path)
}

pub fn load_sessions_from(path: &Path) -> Result<Vec<Session>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;

    // Parse as raw Value first to detect and migrate plaintext passwords
    let values: Vec<serde_json::Value> = serde_json::from_str(&raw)
        .with_context(|| "parsing sessions JSON")?;

    let mut sessions = Vec::with_capacity(values.len());
    let mut migrated = false;

    for v in &values {
        let mut s: Session = serde_json::from_value(v.clone())
            .with_context(|| "deserializing session")?;

        // Migration: old format stored password in JSON
        if let Some(pw) = v.get("password").and_then(|p| p.as_str()) {
            if !pw.is_empty() {
                kr_store(&s.name, pw).ok();
                migrated = true;
            }
        }

        // Load password from keychain
        if let Some(pw) = kr_load(&s.name) {
            s.password = pw;
        }

        sessions.push(s);
    }

    // Re-save without the plaintext passwords if migration happened
    if migrated {
        save_sessions_to(&sessions, path).ok();
    }

    Ok(sessions)
}

pub fn save_sessions(sessions: &[Session]) -> Result<()> {
    let path = sessions_path();
    save_sessions_to(sessions, &path)
}

pub fn save_sessions_to(sessions: &[Session], path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating dir {}", parent.display()))?;
    }

    // Persist non-empty passwords to keychain
    for s in sessions {
        if !s.password.is_empty() {
            kr_store(&s.name, &s.password).ok();
        }
    }

    let tmp  = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(sessions).context("serializing sessions")?;
    std::fs::write(&tmp, json).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| "renaming tmp to sessions.json")?;
    Ok(())
}

pub fn sessions_mtime() -> f64 {
    let path = sessions_path();
    std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .map(|t| t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64())
        .unwrap_or(0.0)
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn password_not_in_json() {
        let tmp  = tempdir().unwrap();
        let path = tmp.path().join("sessions.json");

        let s = Session {
            name:     "test-host".to_string(),
            host:     "192.168.1.1".to_string(),
            user:     "admin".to_string(),
            port:     22,
            password: "secret".to_string(),
        };

        save_sessions_to(&[s], &path).unwrap();

        let json = std::fs::read_to_string(&path).unwrap();
        assert!(!json.contains("secret"),   "password leaked into JSON: {json}");
        assert!(!json.contains("password"), "password key present in JSON: {json}");
    }

    #[test]
    fn migration_strips_plaintext_password() {
        let tmp  = tempdir().unwrap();
        let path = tmp.path().join("sessions.json");

        // Old-format JSON with plaintext password
        let old = r#"[{"name":"myhost","host":"1.2.3.4","user":"alice","port":22,"password":"hunter2"}]"#;
        std::fs::write(&path, old).unwrap();

        let sessions = load_sessions_from(&path).unwrap();
        assert_eq!(sessions.len(), 1);
        let new_json = std::fs::read_to_string(&path).unwrap();
        assert!(!new_json.contains("hunter2"), "plaintext password still in file after migration");
    }

    #[test]
    fn keychain_available_returns_bool() {
        let _ = keychain_available(); // either true or false is acceptable
    }

    #[test]
    fn roundtrip_name_host_port() {
        let tmp  = tempdir().unwrap();
        let path = tmp.path().join("sessions.json");

        let s = Session {
            name: "prod".to_string(),
            host: "10.0.0.1".to_string(),
            user: "root".to_string(),
            port: 2222,
            password: String::new(),
        };
        save_sessions_to(&[s], &path).unwrap();

        let loaded = load_sessions_from(&path).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "prod");
        assert_eq!(loaded[0].port, 2222);
    }
}
