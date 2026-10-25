fn main() {
    // Source identity for --version and the window title, like the C# build's InformationalVersion.
    let commit = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".into());
    let dirty = std::process::Command::new("git").args(["status", "--porcelain"]).output().ok().is_some_and(|o| !o.stdout.is_empty());
    println!("cargo:rustc-env=BILI_SOURCE={}{}", commit, if dirty { ".dirty" } else { "" });
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/index");
    tauri_build::build()
}
