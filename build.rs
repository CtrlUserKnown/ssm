use std::process::Command;

fn main() {
    // Version: env override (CI injects the tag) → git describe → Cargo version.
    let version = std::env::var("SSM_VERSION")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(git_describe)
        .unwrap_or_else(|| std::env::var("CARGO_PKG_VERSION").unwrap_or_default());
    let version = version.trim().trim_start_matches('v');
    println!("cargo:rustc-env=SSM_VERSION={version}");

    // Target triple (e.g. x86_64-unknown-linux-gnu) — matches the release asset
    // name, so the self-updater knows exactly which tarball to fetch.
    if let Ok(target) = std::env::var("TARGET") {
        println!("cargo:rustc-env=SSM_TARGET={target}");
    }

    if std::path::Path::new(".git/HEAD").exists() {
        println!("cargo:rerun-if-changed=.git/HEAD");
    }
    println!("cargo:rerun-if-env-changed=SSM_VERSION");
}

fn git_describe() -> Option<String> {
    let out = Command::new("git")
        .args(["describe", "--tags", "--always", "--match", "v*", "--dirty"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!s.is_empty()).then_some(s)
}
