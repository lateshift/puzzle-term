//! Headless nvim keyboard-protocol acceptance through the real PTY.
use std::{fs, path::Path};
/// Runs nvim after its kitty negotiation and inspects the actual PTY write.
/// An absent optional nvim leaves an explicit skip artifact. On machines
/// with nvim, the exact hex line is required: flag 3 activates kitty's
/// disambiguated Ctrl+D form and there must be no second transport write.
pub(super) fn ctrl_d() -> Result<(), String> {
    // A fresh run prevents an old key-bytes artifact from passing after
    // changes to the live adapter or input encoder.
    crate::commands::capture(
        "cargo",
        &[
            "run",
            "-p",
            "puzzle-desktop",
            "--",
            "harness",
            "assets/scenarios/nvim-ctrl-d.pzs",
        ],
    )?;
    let root = Path::new("target/harness/nvim-ctrl-d");
    // The scenario writes this only when executable lookup fails; the
    // installed path deletes any stale skip file before spawning nvim.
    if root.join("nvim-skip.txt").exists() {
        return Ok(());
    }
    let bytes =
        fs::read_to_string(root.join("key-bytes.txt")).map_err(|error| error.to_string())?;
    // These bytes are ESC [ 100 ; 5 u. A legacy 04 or a second line from
    // IME Commit fails even if the editor happens to show no visible error.
    if bytes.trim() != "flags=3\n1b 5b 31 30 30 3b 35 75" {
        return Err(format!("nvim Ctrl+D PTY writes differ: {bytes:?}"));
    }
    let grid = fs::read_to_string(root.join("after-ctrl-d.grid.txt"))
        .map_err(|error| error.to_string())?;
    // Progress milestones are near the top of the real file; they move
    // into view after a half-page scroll on the fixed thirty-row grid.
    if grid.contains("^D") || !grid.contains("M1 Bus and logging") {
        return Err("nvim did not scroll half a page without a literal ^D".into());
    }
    Ok(())
}
