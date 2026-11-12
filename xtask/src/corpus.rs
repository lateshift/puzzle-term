//! Generates deterministic terminal corpora and measures native parse rates.
//!
//! The timed code lives in a test-only puzzle-vt example. `xtask` remains
//! standard-library-only, and acceptance reads one numeric result rather
//! than trusting a benchmark command's successful exit alone.
//! The old artifact is removed before each child process starts. Otherwise
//! a successful but broken benchmark executable could leave a stale number
//! that falsely passes the parser budget. Only the median is reported;
//! individual samples stay inside the example's timing loop.
//! M5.5 adds a second artifact for reflow. It measures a prepared grid's
//! resize alone, so parsing and fixture construction do not consume the
//! 20 ms budget. Both probes delete prior artifacts before launching a
//! release binary; this prevents a failed or skipped run from reporting
//! an earlier successful result as current evidence (R14).
//!
//! Source streams are checked in so CI and local runs feed identical
//! control bytes. The timed examples repeat whole source records to about
//! 8 MiB and time five complete `Terminal::advance` calls on a warm grid.
//! The numeric files in `target/corpus/` are disposable results, not
//! fixtures: deleting them before each child run is part of the evidence
//! chain. ASCII keeps its M5.2 example so earlier benchmark comparisons
//! remain meaningful; SGR, cursor, Unicode and recorded tmux share one
//! example. Tmux captures are made separately in the native PTY shell,
//! inspected for private data, then committed as fixed inputs.

/// Builds the four §15.2 source streams without using terminal state.
mod generate;

/// Returns the median native plain-ASCII throughput in decimal MB/s.
pub(crate) fn ascii_rate() -> Result<f64, String> {
    generate::write_all()?;
    // A leftover artifact from a prior build must not satisfy this run.
    let path = "target/corpus/plain-ascii-mbps.txt";
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    crate::commands::quiet(
        "cargo",
        &[
            "run",
            "--release",
            "-p",
            "puzzle-vt",
            "--example",
            "plain_ascii",
        ],
    )?;
    // The example writes only after timing and the child has exited here.
    std::fs::read_to_string(path)
        .map_err(|error| error.to_string())?
        .trim()
        .parse::<f64>()
        .map_err(|error| format!("invalid ASCII throughput: {error}"))
}

/// Measures a generated SGR, cursor or Unicode stream end to end.
pub(crate) fn mixed_rate(name: &str) -> Result<f64, String> {
    // Rebuild the checked-in synthetic input from its deterministic
    // generator before timing; a stale or edited local corpus cannot
    // silently alter the workload used by an acceptance floor.
    generate::write_all()?;
    let path = format!("target/corpus/{name}-mbps.txt");
    // Each criterion removes its previous artifact before spawning the
    // release example; a failed child cannot leave a stale passing rate.
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    crate::commands::quiet(
        "cargo",
        &[
            "run",
            "--release",
            "-p",
            "puzzle-vt",
            "--example",
            "parse_budget",
            "--",
            name,
        ],
    )?;
    // The result is a decimal MB/s median from the child. Parsing it
    // here makes an empty or nonnumeric artifact a failing criterion.
    std::fs::read_to_string(path)
        .map_err(|error| error.to_string())?
        .trim()
        .parse::<f64>()
        .map_err(|error| format!("invalid {name} throughput: {error}"))
}

/// Median MB/s for the SGR-heavy colored-output corpus.
pub(crate) fn sgr_rate() -> Result<f64, String> {
    mixed_rate("sgr-heavy")
}

/// Median MB/s for the cursor-heavy htop-like update corpus.
pub(crate) fn cursor_rate() -> Result<f64, String> {
    mixed_rate("cursor-heavy")
}

/// Diagnostic median MB/s for the mixed Unicode corpus.
pub(crate) fn unicode_rate() -> Result<f64, String> {
    mixed_rate("unicode-mix")
}

/// Diagnostic parse rate for continuous full-screen truecolor animation.
pub(crate) fn flood_rate() -> Result<f64, String> {
    mixed_rate("flood")
}

/// Regenerates synthetic sources and checks the three recorded tmux streams.
pub(crate) fn verify_sources() -> Result<(), String> {
    // All four represent distinct parser paths. Existence alone is weak:
    // a truncated source can make warm-up or repetition misleading.
    generate::write_all()?;
    for name in [
        "plain-ascii",
        "sgr-heavy",
        "cursor-heavy",
        "unicode-mix",
        "flood",
        "tmux-single",
        "tmux-hsplit",
        "tmux-vsplit",
    ] {
        let path = format!("assets/fixtures/corpus/{name}.vt");
        let bytes = std::fs::metadata(&path)
            .map_err(|error| error.to_string())?
            .len();
        if bytes < 64 * 1024 {
            return Err(format!("{path} has only {bytes} bytes"));
        }
    }
    Ok(())
}

/// Returns the five-trial median for 10,000×200 native reflow in milliseconds.
pub(crate) fn reflow_ms() -> Result<f64, String> {
    // The example reports only after all five timing trials complete.
    let path = "target/corpus/reflow-ms.txt";
    // Remove stale evidence before building or running the timed fixture.
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    crate::commands::quiet(
        "cargo",
        &[
            "run",
            "--release",
            "-p",
            "puzzle-vt",
            "--example",
            "reflow_budget",
        ],
    )?;
    // A numeric parse is required: mere file existence cannot satisfy
    // the native budget comparison in accept M5.
    std::fs::read_to_string(path)
        .map_err(|error| error.to_string())?
        .trim()
        .parse::<f64>()
        .map_err(|error| format!("invalid reflow timing: {error}"))
}

/// Regenerates synthetic fixtures and prints fresh native medians.
pub(crate) fn run() -> Result<(), String> {
    // The Unicode rate is diagnostic, while the first three correspond
    // to approved native throughput floors in design §15.2.
    let rate = ascii_rate()?;
    println!("plain ASCII parse: {rate:.3} MB/s");
    println!("SGR-heavy parse: {:.3} MB/s", sgr_rate()?);
    println!("cursor-heavy parse: {:.3} MB/s", cursor_rate()?);
    println!("Unicode mix parse: {:.3} MB/s", unicode_rate()?);
    println!("Flood truecolor parse: {:.3} MB/s", flood_rate()?);
    for (name, label) in [
        ("tmux-single", "tmux single"),
        ("tmux-hsplit", "tmux horizontal split"),
        ("tmux-vsplit", "tmux vertical split"),
    ] {
        // The recorded bytes stay immutable during a rate run; otherwise
        // another host's tmux timing could silently change this workload.
        println!("{label} parse: {:.3} MB/s", mixed_rate(name)?);
    }
    Ok(())
}
