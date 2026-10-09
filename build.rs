//! Build metadata: short git hash baked in as BUILD_GIT_HASH (surfaced in
//! `/api/health` as `build`). Degrades to "unknown" when git is absent
//! (e.g. source archives) — the build never fails on metadata.

fn main() {
    let hash = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=BUILD_GIT_HASH={hash}");
}
