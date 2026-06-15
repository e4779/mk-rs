use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

fn main() {
    let hash = read_hash_file()
        .or_else(git_short)
        .unwrap_or_else(|| "unknown".into());

    println!("cargo:rustc-env=GIT_HASH={hash}");
    println!("cargo:rerun-if-changed=GIT_HASH");
    println!("cargo:rerun-if-changed=.git/HEAD");
}

fn read_hash_file() -> Option<String> {
    let path = Path::new(&env::var("CARGO_MANIFEST_DIR").ok()?).join("GIT_HASH");
    fs::read_to_string(path).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn git_short() -> Option<String> {
    Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output().ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
}
