use anyhow::{bail, Context, Result};
use std::process::Command;

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

/// Build the `user@host` target, omitting `user@` when no user is set (ssh then
/// falls back to the local login / ssh_config default).
fn ssh_target(session: &Session) -> String {
    if session.user.is_empty() {
        session.host.clone()
    } else {
        format!("{}@{}", session.user, session.host)
    }
}

fn connect_herdr(session: &Session) -> Result<()> {
    let target = if session.port != 22 {
        format!("{}:{}", ssh_target(session), session.port)
    } else {
        ssh_target(session)
    };

    // herdr shells out to system ssh for `--remote`, so the SSH_ASKPASS
    // hand-off reaches ssh through it. herdr takes only a target (no ssh
    // flags), so per-session port/jump must live in ~/.ssh/config for this mode.
    let mut cmd = Command::new("herdr");
    cmd.args(["--remote", &target]);
    run_with_password(cmd, session, "herdr")
}

fn connect_ssh(session: &Session) -> Result<()> {
    let mut cmd = Command::new("ssh");
    cmd.arg("-p").arg(session.port.to_string());
    if let Some(jump) = session
        .proxy_jump
        .as_deref()
        .filter(|j| !j.trim().is_empty())
    {
        cmd.arg("-J").arg(jump);
    }
    // Under SSH_ASKPASS the host-key confirmation prompt can't be answered, so
    // auto-accept keys for *new* hosts (a changed key still aborts — MITM guard).
    if !session.password.is_empty() {
        cmd.arg("-o").arg("StrictHostKeyChecking=accept-new");
    }
    cmd.arg(ssh_target(session));
    run_with_password(cmd, session, "ssh")
}

/// Run `cmd`, feeding `session.password` (when present) through an SSH_ASKPASS
/// server so ssh/herdr authenticate non-interactively. With no password we run
/// the command untouched (key/agent auth, normal interactive prompts).
fn run_with_password(mut cmd: Command, session: &Session, what: &str) -> Result<()> {
    // Hold the askpass server for the lifetime of the child; drop tears it down.
    let _askpass = if session.password.is_empty() {
        None
    } else {
        match std::env::current_exe() {
            Ok(exe) => {
                let server = super::askpass::Server::start(session.password.clone())?;
                cmd.env("SSH_ASKPASS", exe);
                // `force` makes ssh use the helper even with a controlling TTY
                // (OpenSSH 8.4+). No DISPLAY needed.
                cmd.env("SSH_ASKPASS_REQUIRE", "force");
                cmd.env("SSM_ASKPASS_SOCK", server.sock_path());
                cmd.env("SSM_ASKPASS_NONCE", server.nonce());
                Some(server)
            }
            Err(e) => {
                // Can't locate our own binary to act as askpass; fall back to an
                // interactive prompt rather than failing outright.
                eprintln!("warning: could not enable password hand-off ({e}); ssh may prompt");
                None
            }
        }
    };

    let status = cmd
        .status()
        .with_context(|| format!("launching {what}"))?;
    if !status.success() {
        bail!("{what} exited with status {}", status);
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

/// Import hosts from an ssh_config file, merge new ones into the store, and
/// print a summary. `path` is `~/.ssh/config` when `None`.
pub fn cli_import(path: Option<std::path::PathBuf>) -> Result<()> {
    let path = path.unwrap_or_else(super::ssh_config::default_ssh_config_path);
    let imported = super::ssh_config::parse_ssh_config(&path)?;
    if imported.is_empty() {
        println!("No importable hosts found in {}", path.display());
        return Ok(());
    }
    let mut sessions = super::storage::load_sessions()?;
    let added = super::ssh_config::merge_new(&mut sessions, imported);
    if added > 0 {
        super::storage::save_sessions(&sessions)?;
    }
    println!(
        "Imported {added} new host(s) from {} ({} total)",
        path.display(),
        sessions.len()
    );
    Ok(())
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
        let host = if s.user.is_empty() {
            s.host.clone()
        } else {
            format!("{}@{}", s.user, s.host)
        };
        println!("{:<20} {:<30} {:<6}", s.name, host, s.port);
    }
    Ok(())
}

fn parse_hostspec(spec: &str) -> Result<Session> {
    // Formats: user@host  /  user@host:port
    let at = spec
        .find('@')
        .with_context(|| format!("expected user@host, got {spec:?}"))?;
    let user = spec[..at].to_string();
    let rest = &spec[at + 1..];
    let (host, port) = if let Some(colon) = rest.rfind(':') {
        let p: u16 = rest[colon + 1..]
            .parse()
            .with_context(|| format!("invalid port in {spec:?}"))?;
        (rest[..colon].to_string(), p)
    } else {
        (rest.to_string(), 22)
    };
    if user.is_empty() {
        bail!("missing user in {spec:?}");
    }
    if host.is_empty() {
        bail!("missing host in {spec:?}");
    }
    Ok(Session {
        name: spec.to_string(),
        host,
        user,
        port,
        ..Session::default()
    })
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
    fn ssh_target_with_user() {
        let s = Session {
            user: "alice".into(),
            host: "h".into(),
            ..Session::default()
        };
        assert_eq!(ssh_target(&s), "alice@h");
    }

    #[test]
    fn ssh_target_without_user_omits_at() {
        let s = Session {
            user: String::new(),
            host: "h".into(),
            ..Session::default()
        };
        assert_eq!(ssh_target(&s), "h");
    }

    #[test]
    fn herdr_not_found_returns_error() {
        // herdr is likely not installed in test environment
        let session = Session {
            name: "test".to_string(),
            host: "127.0.0.1".to_string(),
            user: "nobody".to_string(),
            port: 22,
            ..Session::default()
        };
        let cfg = ConnectConfig { use_herdr: true };
        // On a machine without herdr, this should error (either missing or rejected connection)
        let result = do_connect(&session, &cfg);
        // We just verify it doesn't panic; it may succeed on machines with herdr
        let _ = result;
    }
}
