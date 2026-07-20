//! Import sessions from an OpenSSH `~/.ssh/config` file.
//!
//! ssm doesn't speak the SSH protocol itself — it shells out to `ssh` — so it
//! can lean on the config a user already maintains. This parser walks the file
//! and turns each concrete `Host` block into a [`Session`], carrying across the
//! handful of directives ssm models: `HostName`, `Port`, `User`, and
//! `ProxyJump`. Wildcard blocks (`Host *`, `Host web-*`) are patterns rather
//! than real hosts, so they're skipped.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::storage::Session;

/// Default location of the user's SSH client config.
pub fn default_ssh_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".ssh/config")
}

/// Parse the given ssh_config file into a list of importable sessions.
///
/// Returns an empty vector if the file doesn't exist, so callers can treat "no
/// config" and "empty config" the same way.
pub fn parse_ssh_config(path: &Path) -> Result<Vec<Session>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(parse_ssh_config_str(&raw))
}

/// Parse ssh_config text into sessions. Split out from [`parse_ssh_config`] so
/// it can be unit-tested without touching the filesystem.
pub fn parse_ssh_config_str(text: &str) -> Vec<Session> {
    let mut sessions = Vec::new();
    let mut current: Option<Session> = None;

    // Push the in-progress block if it names a concrete, connectable host.
    let flush = |cur: Option<Session>, out: &mut Vec<Session>| {
        if let Some(mut s) = cur {
            // `Host foo` with no `HostName` still connects to `foo`.
            if s.host.is_empty() {
                s.host = s.name.clone();
            }
            if !s.name.is_empty() {
                out.push(s);
            }
        }
    };

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Directives are `Key Value`, optionally separated by `=`.
        let (key, value) = match split_directive(line) {
            Some(kv) => kv,
            None => continue,
        };

        match key.to_lowercase().as_str() {
            "host" => {
                flush(current.take(), &mut sessions);
                // A `Host` line can list several aliases/patterns. Take the
                // first concrete (wildcard-free) alias as the session name.
                let alias = value.split_whitespace().find(|a| !is_pattern(a));
                current = alias.map(|a| Session {
                    name: a.to_string(),
                    ..Session::default()
                });
            }
            "hostname" => {
                if let Some(s) = current.as_mut() {
                    s.host = value.to_string();
                }
            }
            "port" => {
                if let Some(s) = current.as_mut() {
                    if let Ok(p) = value.parse::<u16>() {
                        s.port = p;
                    }
                }
            }
            "user" => {
                if let Some(s) = current.as_mut() {
                    s.user = value.to_string();
                }
            }
            "proxyjump" => {
                if let Some(s) = current.as_mut() {
                    // `none` explicitly disables a jump inherited from a wider block.
                    s.proxy_jump = if value.eq_ignore_ascii_case("none") {
                        None
                    } else {
                        Some(value.to_string())
                    };
                }
            }
            _ => {}
        }
    }
    flush(current.take(), &mut sessions);
    sessions
}

/// Merge freshly parsed sessions into an existing list, skipping any whose name
/// already exists. Returns the number actually added.
pub fn merge_new(existing: &mut Vec<Session>, imported: Vec<Session>) -> usize {
    let mut added = 0;
    for s in imported {
        if !existing.iter().any(|e| e.name == s.name) {
            existing.push(s);
            added += 1;
        }
    }
    added
}

/// Split an ssh_config directive line into `(key, value)`, tolerating both
/// `Key Value` and `Key = Value` forms. Returns `None` for value-less lines.
fn split_directive(line: &str) -> Option<(&str, &str)> {
    let bytes = line.as_bytes();
    let key_end = bytes
        .iter()
        .position(|&b| b == b' ' || b == b'\t' || b == b'=')?;
    let key = line[..key_end].trim();
    let value = line[key_end..].trim_start_matches([' ', '\t', '=']).trim();
    if key.is_empty() || value.is_empty() {
        return None;
    }
    Some((key, value))
}

/// A `Host` token is a glob pattern (rather than a concrete host) if it
/// contains `*`, `?`, or a `!` negation.
fn is_pattern(token: &str) -> bool {
    token.contains('*') || token.contains('?') || token.starts_with('!')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_block() {
        let cfg = "\
Host web
    HostName 10.0.0.1
    User deploy
    Port 2222
";
        let s = parse_ssh_config_str(cfg);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "web");
        assert_eq!(s[0].host, "10.0.0.1");
        assert_eq!(s[0].user, "deploy");
        assert_eq!(s[0].port, 2222);
    }

    #[test]
    fn skips_wildcard_blocks() {
        let cfg = "\
Host *
    User root
Host db
    HostName 10.0.0.2
";
        let s = parse_ssh_config_str(cfg);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "db");
    }

    #[test]
    fn host_without_hostname_uses_alias() {
        let s = parse_ssh_config_str("Host example.com\n");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].host, "example.com");
        assert_eq!(s[0].port, 22);
    }

    #[test]
    fn carries_proxy_jump() {
        let cfg = "\
Host internal
    HostName 192.168.1.5
    ProxyJump bastion
";
        let s = parse_ssh_config_str(cfg);
        assert_eq!(s[0].proxy_jump.as_deref(), Some("bastion"));
    }

    #[test]
    fn handles_equals_and_tabs() {
        let cfg = "Host=api\n\tHostName = api.example.com\n\tPort=443\n";
        let s = parse_ssh_config_str(cfg);
        assert_eq!(s[0].name, "api");
        assert_eq!(s[0].host, "api.example.com");
        assert_eq!(s[0].port, 443);
    }

    #[test]
    fn multi_alias_takes_first_concrete() {
        let s = parse_ssh_config_str("Host prod prod-* p1\n    HostName 10.0.0.9\n");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "prod");
    }

    #[test]
    fn merge_skips_existing_names() {
        let mut existing = vec![Session {
            name: "web".to_string(),
            ..Session::default()
        }];
        let imported = vec![
            Session {
                name: "web".to_string(),
                ..Session::default()
            },
            Session {
                name: "db".to_string(),
                ..Session::default()
            },
        ];
        let added = merge_new(&mut existing, imported);
        assert_eq!(added, 1);
        assert_eq!(existing.len(), 2);
    }
}
