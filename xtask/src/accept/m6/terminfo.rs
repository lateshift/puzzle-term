//! D-32 acceptance of the compiled entry and real tmux negotiation.
//!
//! `infocmp` checks what ncurses reads, rather than accepting an uncompiled
//! source file. The PTY smoke then confirms TERM and TERMINFO reached a
//! child. tmux is the consumer that motivated Sync: its redraw test uses
//! the actual client output with native 1 KiB read boundaries.
//!
//! There are three independent risks in D-32. First, the source may be
//! edited without recompiling the binary that children actually read.
//! Second, the desktop launcher may set TERM but omit TERMINFO, causing
//! ncurses in the child to miss the entry. Third, both may be correct yet
//! tmux may decline to use synchronized output because it interprets the
//! capability differently. These checks cross each boundary separately.
//!
//! `infocmp -A` points at the bundled directory explicitly, so a user's
//! own terminfo database cannot make a missing checked-in entry pass.
//! Both hexadecimal and first-letter layouts are byte-compared by the
//! xtask verifier. `infocmp` then proves ncurses decodes those bytes.
//!
//! The PTY smoke uses the same spawn function as the live window. Its
//! fixed shell prints TERM and runs `tput` inside the child, which proves
//! the environment reaches a real process rather than only our Rust
//! structure. The tmux probe runs once for each criterion: its random
//! cmatrix repaint may differ between runs, but both runs retain the
//! Puzzle-versus-legacy control in the same process invocation.
//!
//! tmux and cmatrix are optional native test programs, not dependencies
//! of the shipped terminal. An absent tmux is a visible SKIP as the work
//! order requires. A present tmux with a broken capability is a failure.

use super::report;
use std::process::Command;

/// Reads the bundled binary entry and finds every declared D-32 name.
fn compiled_entry() -> Result<(), String> {
    // Recompile before reading the entry, so a stale source/binary pair
    // cannot be hidden by a valid older binary.
    crate::terminfo::verify()?;
    let output = crate::commands::capture(
        "infocmp",
        &["-x", "-A", "assets/terminfo/compiled", "xterm-puzzle"],
    )?;
    if !output.starts_with("#") || !output.contains("xterm-puzzle|puzzle|") {
        // `infocmp` can return an inherited system entry if the search
        // path is wrong; require our distinctive primary name and alias.
        return Err("infocmp did not read the bundled Puzzle entry".into());
    }
    for name in [
        "Sync", "RGB", "Tc", "Smulx", "Setulc", "Ss", "Se", "Cs", "Cr", "Ms", "Smol", "Rmol", "E3",
        "BE", "BD", "PS", "PE", "hs", "tsl", "fsl", "XT",
    ] {
        // A boolean ends with a comma; string values use `=`. Matching
        // those delimiters avoids treating a substring as a capability.
        if !output.contains(&format!("{name},")) && !output.contains(&format!("{name}=")) {
            return Err(format!("compiled terminfo lacks {name}"));
        }
    }
    Ok(())
}

/// Requires a child to see the entry and expand its Sync capability.
fn pty_child() -> Result<(), String> {
    // The smoke's buffer assertion is on parsed cells, so this does not
    // pass if a shell printed TERM but Session lost its output bytes.
    crate::commands::capture("cargo", &["run", "-p", "puzzle-desktop", "--", "pty-smoke"])?;
    Ok(())
}

/// Runs tmux if installed; absent optional native programs are visibly skipped.
fn tmux_probe(mode: &str) -> Result<(), String> {
    // A missing tmux or cmatrix is an environment limitation, not a product failure.
    // The status remains visible in the acceptance transcript.
    if Command::new("tmux").arg("-V").output().is_err()
        || Command::new("cmatrix").arg("-h").output().is_err()
    {
        println!("SKIP M6.4e tmux {mode}: tmux or cmatrix unavailable");
        return Ok(());
    }
    crate::commands::capture(
        "cargo",
        &["run", "-p", "puzzle-desktop", "--", "terminfo-probe", mode],
    )?;
    // The native probe itself compares both TERM variants and returns a
    // nonzero status for a false positive or an unprotected prompt frame.
    Ok(())
}

/// Replays checked-in PTY reads even on hosts without tmux or cmatrix.
fn replay_fixture() -> Result<(), String> {
    crate::commands::capture(
        "cargo",
        &[
            "run",
            "-p",
            "puzzle-desktop",
            "--",
            "terminfo-probe",
            "--fixture",
        ],
    )?;
    Ok(())
}

/// Selects the integration test so TN is checked through the VT parser.
fn xtgettcap_wire() -> Result<(), String> {
    let name = "dcs::termcap_query_uses_bundled_puzzle_capabilities";
    let output = crate::commands::capture(
        "cargo",
        &[
            "test",
            "-p",
            "puzzle-vt",
            "--test",
            "conformance",
            name,
            "--",
            "--exact",
        ],
    )?;
    if output.contains(&format!("test {name} ... ok")) {
        Ok(())
    } else {
        Err(format!("VT integration test {name} did not run"))
    }
}

/// Reports source, PTY and redraw contracts independently (R14).
pub(super) fn report_terminfo(passed: &mut bool) {
    // Each line corresponds to a distinct §6.12 promise; a failure in
    // one does not prevent the remaining evidence from being collected.
    report(
        "M6.4e compiled xterm-puzzle capabilities",
        compiled_entry(),
        passed,
    );
    report("M6.4e PTY TERM and tput Sync", pty_child(), passed);
    report(
        "M6.4e deterministic 1 KiB tmux cursor fixtures",
        replay_fixture(),
        passed,
    );
    report(
        "M6.4e tmux enables synchronized output",
        tmux_probe("--sync"),
        passed,
    );
    report(
        "M6.4e tmux cursor-flicker regression",
        tmux_probe("--cursor"),
        passed,
    );
    report(
        "M6.4e XTGETTCAP matches bundled capabilities and fallback TERM",
        xtgettcap_wire(),
        passed,
    );
}
