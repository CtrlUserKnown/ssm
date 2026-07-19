use assert_cmd::Command;
use predicates::str as pstr;
use tempfile::tempdir;

#[test]
fn version_flag() {
    Command::cargo_bin("ssm").unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(pstr::contains("ssm"));
}

#[test]
fn help_flag() {
    Command::cargo_bin("ssm").unwrap()
        .arg("--help")
        .assert()
        .success();
}

#[test]
fn ssm_list_empty() {
    let tmp = tempdir().unwrap();
    Command::cargo_bin("ssm").unwrap()
        .env("DOTS_SSM_DIR", tmp.path())
        .arg("--list")
        .assert()
        .success()
        .stdout(pstr::contains("No saved sessions"));
}
