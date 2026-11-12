//! Harness source revision for private native recordings (§13.4).
//! Geometry and OS-read boundaries are recorded by the embedding owner before PTY startup.

/// Detached source trees keep a useful explicit unknown revision.
pub(super) fn revision() -> String {
    // Git is invoked only for an explicitly requested diagnostic recording.
    std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".into())
}
