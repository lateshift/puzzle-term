//! Acceptance observers must compile in the same native feature configuration.
//! A workspace build may unify test-support or other features differently from
//! release probes. This explicit package lane catches ownership and lint defects
//! in the diagnostic path before any acceptance command can launch a window.

/// Compile every target of the native lock-metrics lane with warnings denied.
/// This runs before runtime witnesses: a broken probe graph must fail before
/// any dependent measurement could consume a leftover release executable.
pub(super) fn check() -> Result<(), String> {
    // Keep the package pair and feature identical to acceptance's native probes.
    // This strengthens the task gate; it does not enable diagnostics in hosts.
    super::quiet(
        "cargo",
        &[
            "clippy",
            "-p",
            "puzzle-embed",
            "-p",
            "puzzle-desktop",
            "--features",
            "lock-metrics",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    )
}
