//! Feed a stored password to `ssh` (and therefore to `herdr`, which shells out
//! to system `ssh`) without a plaintext file or a long-lived environment var.
//!
//! OpenSSH ≥ 8.4 will call the program named by `$SSH_ASKPASS` to obtain a
//! password when `SSH_ASKPASS_REQUIRE=force` is set — even with a controlling
//! terminal present, which is exactly our situation (we run ssh in the
//! foreground). See the module wiring in [`crate::connect`].
//!
//! Rather than write the secret into a temp askpass script (the classic
//! world-readable-window vulnerability), ssm points `$SSH_ASKPASS` at *itself*
//! and serves the secret over a `0600` Unix-domain socket guarded by a one-time
//! nonce. `ssh` re-execs the helper (`ssm`, detected via [`is_responder`]),
//! which connects back, presents the nonce, and prints the secret.
//!
//! Two behaviours matter for correctness:
//! - **Serve once.** On an auth failure `ssh` retries and calls the helper
//!   again; we return an empty secret the second time so a wrong password fails
//!   fast instead of looping on the same value.
//! - **Password prompts only.** The helper answers only the password prompt; it
//!   refuses key-passphrase and host-key ("continue connecting?") prompts so we
//!   never auto-accept an unknown or changed host key.

use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};

const SOCK_ENV: &str = "SSM_ASKPASS_SOCK";
const NONCE_ENV: &str = "SSM_ASKPASS_NONCE";

/// True when this process was invoked by `ssh` as the askpass helper (i.e. our
/// server exported [`SOCK_ENV`] into the child's environment). Checked at the
/// very top of `main` before normal argument parsing.
pub fn is_responder() -> bool {
    std::env::var_os(SOCK_ENV).is_some()
}

/// Should the helper answer this prompt? Only true password prompts — never key
/// passphrases or the host-key confirmation.
fn is_password_prompt(prompt: &str) -> bool {
    let p = prompt.to_lowercase();
    p.contains("password") && !p.contains("passphrase")
}

/// The askpass helper entry point. Invoked as `ssm "<prompt>"` by ssh with
/// [`SOCK_ENV`]/[`NONCE_ENV`] in the environment. Prints the secret on stdout
/// (what ssh consumes) and exits; never returns.
pub fn respond() -> ! {
    // ssh passes the prompt text as the first CLI argument.
    let prompt = std::env::args().nth(1).unwrap_or_default();
    if !is_password_prompt(&prompt) {
        // Refuse passphrase / host-key prompts: exit non-zero, print nothing.
        std::process::exit(1);
    }

    match fetch_secret() {
        Ok(secret) if !secret.is_empty() => {
            // No trailing newline: ssh takes the first line as the password.
            print!("{secret}");
            let _ = std::io::stdout().flush();
            std::process::exit(0);
        }
        // Already served (retry) or error: nothing to give, fail fast.
        _ => std::process::exit(1),
    }
}

/// Connect to the server socket, present the nonce, and read back the secret.
fn fetch_secret() -> Result<String> {
    let sock = std::env::var(SOCK_ENV).context("missing askpass socket")?;
    let nonce = std::env::var(NONCE_ENV).context("missing askpass nonce")?;
    let mut stream = UnixStream::connect(&sock).context("connecting to askpass socket")?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(nonce.as_bytes()).context("sending nonce")?;
    stream.write_all(b"\n").ok();
    let mut buf = String::new();
    stream.read_to_string(&mut buf).context("reading secret")?;
    Ok(buf)
}

/// Choose a directory for the askpass socket whose full path fits within the
/// kernel's `sockaddr_un.sun_path` limit (104 bytes incl. NUL on macOS, 108 on
/// Linux). macOS's default `$TMPDIR` (`/var/folders/…`) is long enough that our
/// socket name overflows it, so we walk candidate directories shortest-fit-wins
/// and fall back to `/tmp`, which is always short and writable.
fn pick_socket_path(file: &str) -> PathBuf {
    // Stay comfortably under the smaller (macOS) limit, leaving room for the NUL.
    const MAX_LEN: usize = 100;

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(d) = std::env::var_os("XDG_RUNTIME_DIR") {
        candidates.push(PathBuf::from(d));
    }
    candidates.push(std::env::temp_dir());
    candidates.push(PathBuf::from("/tmp"));

    for dir in &candidates {
        let path = dir.join(file);
        if path.as_os_str().len() <= MAX_LEN {
            return path;
        }
    }
    // Nothing fit (extreme case); /tmp is the shortest option available.
    PathBuf::from("/tmp").join(file)
}

/// A running askpass server: owns the socket, the background accept thread, and
/// the environment values the child process needs. Dropping it shuts the thread
/// down and removes the socket file.
pub struct Server {
    sock_path: PathBuf,
    nonce: String,
    shutdown: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Server {
    /// Start serving `secret`. The socket lives in `$XDG_RUNTIME_DIR` (falling
    /// back to a temp dir, or `/tmp` when that path is too long — see
    /// [`pick_socket_path`]) with `0600` permissions.
    pub fn start(secret: String) -> Result<Server> {
        let nonce = random_hex(16);
        let sock_path = pick_socket_path(&format!(
            "ssm-askpass-{}-{}.sock",
            std::process::id(),
            nonce
        ));

        // Stale path from a crashed run would block bind(); clear it first.
        let _ = std::fs::remove_file(&sock_path);
        let listener = UnixListener::bind(&sock_path)
            .with_context(|| format!("binding askpass socket {}", sock_path.display()))?;
        std::fs::set_permissions(&sock_path, std::fs::Permissions::from_mode(0o600))
            .context("locking down askpass socket permissions")?;
        listener
            .set_nonblocking(true)
            .context("setting askpass socket non-blocking")?;

        let shutdown = Arc::new(AtomicBool::new(false));
        let handle = {
            let shutdown = Arc::clone(&shutdown);
            let nonce = nonce.clone();
            thread::Builder::new()
                .name("ssm-askpass".to_string())
                .spawn(move || accept_loop(listener, secret, nonce, shutdown))
                .context("spawning askpass thread")?
        };

        Ok(Server {
            sock_path,
            nonce,
            shutdown,
            handle: Some(handle),
        })
    }

    pub fn sock_path(&self) -> &std::path::Path {
        &self.sock_path
    }

    pub fn nonce(&self) -> &str {
        &self.nonce
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        let _ = std::fs::remove_file(&self.sock_path);
    }
}

fn accept_loop(listener: UnixListener, secret: String, nonce: String, shutdown: Arc<AtomicBool>) {
    let mut served = false;
    loop {
        if shutdown.load(Ordering::Relaxed) {
            return;
        }
        match listener.accept() {
            Ok((stream, _addr)) => {
                // Serve the real secret once; empty on any later (retry) call.
                let give = if served { "" } else { &secret };
                if handle_client(stream, &nonce, give) {
                    served = true;
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => return,
        }
    }
}

/// Validate the nonce and, if it matches, write `secret`. Returns whether a
/// valid (nonce-matched) request was served.
fn handle_client(mut stream: UnixStream, nonce: &str, secret: &str) -> bool {
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
    let mut buf = Vec::new();
    // Read the nonce line (client writes "<nonce>\n").
    let mut byte = [0u8; 1];
    while buf.len() < 128 {
        match stream.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                if byte[0] == b'\n' {
                    break;
                }
                buf.push(byte[0]);
            }
            Err(_) => return false,
        }
    }
    let got = String::from_utf8_lossy(&buf);
    if got != nonce {
        return false;
    }
    let _ = stream.write_all(secret.as_bytes());
    let _ = stream.flush();
    true
}

/// A hex string of `bytes` random bytes, read from `/dev/urandom` with a
/// time+pid fallback so the nonce is never predictable *and* never panics.
fn random_hex(bytes: usize) -> String {
    let mut raw = vec![0u8; bytes];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        if f.read_exact(&mut raw).is_ok() {
            return to_hex(&raw);
        }
    }
    // Fallback: not cryptographic, but the socket is already 0600-restricted.
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
        ^ (std::process::id() as u128);
    to_hex(&seed.to_le_bytes()[..bytes.min(16)])
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_password_prompts_answered() {
        assert!(is_password_prompt("alice@host's password: "));
        assert!(is_password_prompt("Password:"));
        assert!(!is_password_prompt("Enter passphrase for key /home/a/.ssh/id_ed25519:"));
        assert!(!is_password_prompt(
            "Are you sure you want to continue connecting (yes/no/[fingerprint])?"
        ));
    }

    fn ask(sock: &std::path::Path, nonce: &str) -> String {
        let mut s = UnixStream::connect(sock).unwrap();
        s.write_all(nonce.as_bytes()).unwrap();
        s.write_all(b"\n").unwrap();
        let mut buf = String::new();
        s.read_to_string(&mut buf).unwrap();
        buf
    }

    #[test]
    fn serves_secret_once_then_empty() {
        let srv = Server::start("hunter2".to_string()).unwrap();
        let sock = srv.sock_path().to_path_buf();
        let nonce = srv.nonce().to_string();

        assert_eq!(ask(&sock, &nonce), "hunter2");
        // Retry gets nothing so a wrong password fails fast instead of looping.
        assert_eq!(ask(&sock, &nonce), "");
    }

    #[test]
    fn wrong_nonce_gets_nothing() {
        let srv = Server::start("secret".to_string()).unwrap();
        let sock = srv.sock_path().to_path_buf();
        assert_eq!(ask(&sock, "not-the-nonce"), "");
        // The real nonce still works afterwards (bad attempt didn't consume it).
        assert_eq!(ask(&sock, srv.nonce()), "secret");
    }

    #[test]
    fn socket_is_owner_only() {
        let srv = Server::start("x".to_string()).unwrap();
        let mode = std::fs::metadata(srv.sock_path()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn drop_removes_socket() {
        let srv = Server::start("x".to_string()).unwrap();
        let path = srv.sock_path().to_path_buf();
        assert!(path.exists());
        drop(srv);
        assert!(!path.exists());
    }
}
