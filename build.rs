//! Build metadata: short git hash baked in as BUILD_GIT_HASH (surfaced in
//! `/api/health` as `build`). Reads the git files directly instead of
//! spawning `git`: one malformed PATH entry on this PC makes
//! `Command::new("git")` fail even though the shell finds git. Degrades to
//! "unknown" when the source has no `.git` (source archive, zip export).

use std::path::{Path, PathBuf};

fn git_dir() -> Option<PathBuf> {
    let dot = Path::new(".git");
    if dot.is_dir() {
        return Some(dot.to_path_buf());
    }
    // Worktree / submodule: `.git` is a file pointing at the real dir.
    let text = std::fs::read_to_string(dot).ok()?;
    let path = text.trim().strip_prefix("gitdir: ")?;
    Some(PathBuf::from(path))
}

fn short_hash() -> Option<String> {
    let dir = git_dir()?;
    let head = std::fs::read_to_string(dir.join("HEAD")).ok()?;
    let head = head.trim();
    let full = if let Some(reference) = head.strip_prefix("ref: ") {
        std::fs::read_to_string(dir.join(reference)).ok()?
    } else {
        head.to_string()
    };
    let full = full.trim();
    if full.len() < 7 || !full.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(full.chars().take(7).collect())
}

fn main() {
    let hash = short_hash().unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=BUILD_GIT_HASH={hash}");
    // Re-run when the checked-out commit changes, so /api/health follows the
    // build it was compiled from.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/logs/HEAD");
}
