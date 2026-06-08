fn main() {
    // Embed git commit hash into the binary
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok();
    if let Some(output) = output {
        if output.status.success() {
            let hash = String::from_utf8_lossy(&output.stdout)
                .trim()
                .chars()
                .take(7)
                .collect::<String>();
            println!("cargo:rustc-env=GIT_HASH={hash}");
        }
    }
    // Always set a fallback
    println!("cargo:rerun-if-changed=.git/HEAD");
} else {
    println!("cargo:rustc-env=GIT_HASH=unknown");
}
