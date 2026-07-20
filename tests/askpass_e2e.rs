//! End-to-end check of the SSH_ASKPASS hand-off: run the real `ssm` binary the
//! way `ssh` would (as the askpass helper, with the socket/nonce in the env)
//! and confirm it serves the password for a password prompt, refuses others,
//! and only serves once.

use assert_cmd::Command;
use ssm::askpass::Server;

#[test]
fn responds_to_password_prompt() {
    let srv = Server::start("s3cr3t".to_string()).unwrap();
    Command::cargo_bin("ssm")
        .unwrap()
        .env("SSM_ASKPASS_SOCK", srv.sock_path())
        .env("SSM_ASKPASS_NONCE", srv.nonce())
        .arg("alice@host's password: ")
        .assert()
        .success()
        .stdout("s3cr3t");
}

#[test]
fn refuses_host_key_prompt() {
    let srv = Server::start("s3cr3t".to_string()).unwrap();
    Command::cargo_bin("ssm")
        .unwrap()
        .env("SSM_ASKPASS_SOCK", srv.sock_path())
        .env("SSM_ASKPASS_NONCE", srv.nonce())
        .arg("Are you sure you want to continue connecting (yes/no/[fingerprint])? ")
        .assert()
        .failure()
        .stdout("");
}

#[test]
fn refuses_key_passphrase_prompt() {
    let srv = Server::start("s3cr3t".to_string()).unwrap();
    Command::cargo_bin("ssm")
        .unwrap()
        .env("SSM_ASKPASS_SOCK", srv.sock_path())
        .env("SSM_ASKPASS_NONCE", srv.nonce())
        .arg("Enter passphrase for key '/home/a/.ssh/id_ed25519': ")
        .assert()
        .failure();
}

#[test]
fn serves_password_only_once() {
    let srv = Server::start("once-only".to_string()).unwrap();
    let mut first = Command::cargo_bin("ssm").unwrap();
    first
        .env("SSM_ASKPASS_SOCK", srv.sock_path())
        .env("SSM_ASKPASS_NONCE", srv.nonce())
        .arg("password: ")
        .assert()
        .success()
        .stdout("once-only");

    // Second invocation (an ssh retry) gets nothing so a wrong password fails
    // fast instead of looping on the same value.
    Command::cargo_bin("ssm")
        .unwrap()
        .env("SSM_ASKPASS_SOCK", srv.sock_path())
        .env("SSM_ASKPASS_NONCE", srv.nonce())
        .arg("password: ")
        .assert()
        .failure();
}
