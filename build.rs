// LogCrab - GPL-3.0-or-later
// Build script to embed version info at compile time

use std::process::Command;

fn main() {
    // Get git hash
    let git_hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                String::from_utf8(output.stdout).ok()
            } else {
                None
            }
        })
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Check if working directory is dirty
    let is_dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .map(|output| !output.stdout.is_empty())
        .unwrap_or(false);

    let git_hash = if is_dirty {
        format!("{git_hash}-dirty")
    } else {
        git_hash
    };

    println!("cargo:rustc-env=GIT_HASH={git_hash}");

    // Rerun if git HEAD changes
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
}
