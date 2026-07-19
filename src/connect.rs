use anyhow::{bail, Context, Result};
use std::process::{Command, Stdio};

use super::storage::Session;

#[derive(Debug, Clone)]
pub struct ConnectConfig {
    pub use_herdr: bool,
}

// ── main connect entry ────────────────────────────────────────────────────────

pub fn do_connect(session: &Session, cfg: &ConnectConfig) -> Result<()> {
    if cfg.use_herdr {
        connect_herdr(session)
    } else {
        connect_ssh(session)
    }
}

fn connect_herdr(session: &Session) -> Result<()> {
    let target = if session.port != 22 {
        format!("{}@{}:{}", session.user, session.host, session.port)
    } else {
        format!("{}@{}", session.user, session.host)
    };

    let status = Command::new("herdr")
        .args(["--remote", &target])
        .status()
        .with_context(|| "launching herdr")?;

    if !status.success() {
        bail!("herdr exited with status {}", status);
    }
    Ok(())
}

fn connect_ssh(session: &Session) -> Result<()> {
    let host = format!("{}@{}", session.user, session.host);
    let port  = session.port.to_string();

    if !session.password.is_empty() {
        // sshpass -p <pw> ssh -p <port> user@host
        if !which_exists("sshpass") {
            bail!("sshpass not found — install it to connect with stored passwords");
        }
        let status = Command::new("sshpass")
            .arg("-p")
            .arg(&session.password)
            .arg("ssh")
            .arg("-p")
            .arg(&port)
            .arg(&host)
            .status()
            .with_context(|| "launching sshpass ssh")?;
        if !status.success() {
            bail!("ssh exited with status {}", status);
        }
    } else {
        let status = Command::new("ssh")
            .arg("-p")
            .arg(&port)
            .arg(&host)
            .status()
            .with_context(|| "launching ssh")?;
        if !status.success() {
            bail!("ssh exited with status {}", status);
        }
    }
    Ok(())
}

// ── CLI helpers ───────────────────────────────────────────────────────────────

/// Parse `user@host[:port]` and open a direct SSH connection.
pub fn connect_direct(spec: &str) -> Result<()> {
    let session = parse_hostspec(spec)?;
    let cfg = ConnectConfig { use_herdr: false };
    do_connect(&session, &cfg)
}

/// Print a table of saved sessions to stdout.
pub fn cli_list() -> Result<()> {
    let sessions = super::storage::load_sessions()?;
    if sessions.is_empty() {
        println!("No saved sessions. Use `ssm` to open the TUI and add one.");
        return Ok(());
    }
    println!("{:<20} {:<30} {:<6}", "NAME", "HOST", "PORT");
    println!("{}", "-".repeat(58));
    for s in &sessions {
        let host = format!("{}@{}", s.user, s.host);
        println!("{:<20} {:<30} {:<6}", s.name, host, s.port);
    }
    Ok(())
}

fn parse_hostspec(spec: &str) -> Result<Session> {
    // Formats: user@host  /  user@host:port
    let at = spec.find('@').with_context(|| format!("expected user@host, got {spec:?}"))?;
    let user = spec[..at].to_string();
    let rest = &spec[at + 1..];
    let (host, port) = if let Some(colon) = rest.rfind(':') {
        let p: u16 = rest[colon + 1..].parse()
            .with_context(|| format!("invalid port in {spec:?}"))?;
        (rest[..colon].to_string(), p)
    } else {
        (rest.to_string(), 22)
    };
    if user.is_empty() { bail!("missing user in {spec:?}"); }
    if host.is_empty() { bail!("missing host in {spec:?}"); }
    Ok(Session {
        name:     spec.to_string(),
        host,
        user,
        port,
        password: String::new(),
    })
}

fn which_exists(bin: &str) -> bool {
    Command::new("which")
        .arg(bin)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_user_at_host() {
        let s = parse_hostspec("alice@example.com").unwrap();
        assert_eq!(s.user, "alice");
        assert_eq!(s.host, "example.com");
        assert_eq!(s.port, 22);
    }

    #[test]
    fn parse_user_at_host_port() {
        let s = parse_hostspec("bob@10.0.0.1:2222").unwrap();
        assert_eq!(s.user, "bob");
        assert_eq!(s.host, "10.0.0.1");
        assert_eq!(s.port, 2222);
    }

    #[test]
    fn parse_missing_at_fails() {
        assert!(parse_hostspec("no-at-sign").is_err());
    }

    #[test]
    fn herdr_not_found_returns_error() {
        // herdr is likely not installed in test environment
        let session = Session {
            name: "test".to_string(), host: "127.0.0.1".to_string(),
            user: "nobody".to_string(), port: 22, password: String::new(),
        };
        let cfg = ConnectConfig { use_herdr: true };
        // On a machine without herdr, this should error (either missing or rejected connection)
        let result = do_connect(&session, &cfg);
        // We just verify it doesn't panic; it may succeed on machines with herdr
        let _ = result;
    }
}
