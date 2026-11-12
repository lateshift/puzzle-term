//! Builds and verifies Puzzle's bundled ncurses entry (design §6.12).
//!
//! `tic` is the system compiler approved for D-32. macOS stores entries
//! under hexadecimal prefixes, while Linux commonly uses first letters.
//! Both copies contain identical compiled bytes so TERMINFO works on either
//! layout without installing anything in the user's home directory.
//!
//! Verification recompiles in a private temporary directory. Comparing
//! fresh bytes with checked-in bytes catches an edited source that forgot
//! to regenerate the runtime entry. A missing tic reports SKIP in check;
//! the explicit build command still fails because it cannot do its job.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::SystemTime,
};

/// Generated entry names and both ncurses directory conventions.
const COPIES: [(&str, &str); 4] = [
    ("78/xterm-puzzle", "78/xterm-puzzle"),
    ("78/xterm-puzzle", "x/xterm-puzzle"),
    ("70/puzzle", "70/puzzle"),
    ("70/puzzle", "p/puzzle"),
];

/// Deletes a compiler workspace even after an early validation error.
struct Scratch(PathBuf);

impl Drop for Scratch {
    /// Temporary compiled entries must not pollute later comparisons.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Compiles the source and writes the checked-in native runtime copies.
pub(crate) fn run() -> Result<(), String> {
    let scratch = compile()?;
    let destination = Path::new("assets/terminfo/compiled");
    // Compile once, then duplicate the resulting bytes under the two
    // directory conventions. Recompiling each alias could vary metadata.
    for (source, target) in COPIES {
        let bytes = fs::read(scratch.0.join(source)).map_err(|error| error.to_string())?;
        let path = destination.join(target);
        // An installed terminfo database is deliberately avoided: runtime
        // children receive this repository-local directory through TERMINFO.
        fs::create_dir_all(path.parent().ok_or("entry has no parent")?)
            .map_err(|error| error.to_string())?;
        fs::write(path, bytes).map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Checks that checked-in binaries match a fresh compilation.
pub(crate) fn verify() -> Result<(), String> {
    // `check` remains useful before tic is installed, but the skipped
    // compiler comparison is visible rather than counted as a PASS.
    if !Command::new("tic")
        .arg("-V")
        .output()
        .is_ok_and(|out| out.status.success())
    {
        println!("SKIP terminfo: tic unavailable");
        return Ok(());
    }
    let scratch = compile()?;
    // Compare bytes, not infocmp text: the runtime passes binary entries
    // to ncurses, and this also detects a stale alias copy.
    for (source, target) in COPIES {
        let expected = fs::read(scratch.0.join(source)).map_err(|error| error.to_string())?;
        let actual = fs::read(Path::new("assets/terminfo/compiled").join(target))
            .map_err(|error| format!("{target}: {error}; run cargo xtask terminfo"))?;
        if actual != expected {
            return Err(format!("stale terminfo {target}; run cargo xtask terminfo"));
        }
    }
    Ok(())
}

/// Invokes tic into a unique directory and retains it until its guard drops.
fn compile() -> Result<Scratch, String> {
    // A process-specific path keeps concurrent `check` and `terminfo`
    // invocations from replacing one another's temporary compiler output.
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let scratch = Scratch(
        std::env::temp_dir().join(format!("puzzle-terminfo-{}-{nonce}", std::process::id())),
    );
    // Scratch owns cleanup even when tic reports a malformed capability.
    fs::create_dir_all(&scratch.0).map_err(|error| error.to_string())?;
    // No shell interpolation enters the compiler invocation: the entry
    // path is a fixed repository asset, and the output path is private.
    let output = Command::new("tic")
        .args(["-x", "-o"])
        .arg(&scratch.0)
        .arg("assets/terminfo/xterm-puzzle.terminfo")
        .output()
        .map_err(|error| format!("tic unavailable: {error}"))?;
    if !output.status.success() {
        // Preserve tic's diagnostic; a success status alone says nothing
        // about a source with capabilities that tic could not parse.
        return Err(format!(
            "tic failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(scratch)
}
