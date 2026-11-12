//! Native end-to-end colored, cursor-heavy and Unicode parse probes (§15.2).
//!
//! Timed input is an 8 MiB repetition of a deterministic xtask fixture.
//! `Terminal::advance` includes vte decoding, control execution, cell
//! writes and scrolling. Construction, warm-up and file output occur
//! outside each sample. Five samples are sorted and the median is written
//! for mechanical M5 acceptance; the pure library never reads a clock.
//!
//! SGR-heavy work changes cell rendition and tests SGR parameter decoding.
//! The cursor stream simulates a dashboard that rewrites old rows rather
//! than appending linear output. The Unicode mix exercises width and
//! cluster paths as a diagnostic without a native floor in §15.2.
//! All three use the same 80×24 terminal and bounded history, so an
//! improvement that only skips grid updates cannot satisfy these probes.
use puzzle_vt::Terminal;
use std::{hint::black_box, io, time::Instant};

/// Each sample has enough bytes to dominate timer granularity.
const CORPUS_BYTES: usize = 8 * 1024 * 1024;

/// Chooses only known local streams so a CLI argument cannot name a file.
fn paths(name: &str) -> io::Result<(&'static str, &'static str)> {
    match name {
        "sgr-heavy" => Ok((
            "assets/fixtures/corpus/sgr-heavy.vt",
            "target/corpus/sgr-heavy-mbps.txt",
        )),
        "cursor-heavy" => Ok((
            "assets/fixtures/corpus/cursor-heavy.vt",
            "target/corpus/cursor-heavy-mbps.txt",
        )),
        "unicode-mix" => Ok((
            "assets/fixtures/corpus/unicode-mix.vt",
            "target/corpus/unicode-mix-mbps.txt",
        )),
        "flood" => Ok((
            "assets/fixtures/corpus/flood.vt",
            "target/corpus/flood-mbps.txt",
        )),
        // These are captured from tmux 3.7 at 160×48 with a neutral seq
        // workload; unlike the synthetic streams, they include tmux's
        // actual region edits and pane redraw scheduling.
        "tmux-single" => Ok((
            "assets/fixtures/corpus/tmux-single.vt",
            "target/corpus/tmux-single-mbps.txt",
        )),
        "tmux-hsplit" => Ok((
            "assets/fixtures/corpus/tmux-hsplit.vt",
            "target/corpus/tmux-hsplit-mbps.txt",
        )),
        "tmux-vsplit" => Ok((
            "assets/fixtures/corpus/tmux-vsplit.vt",
            "target/corpus/tmux-vsplit-mbps.txt",
        )),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unknown parse corpus",
        )),
    }
}

/// Repeats whole source records without a partial final escape sequence.
fn corpus(source: &[u8]) -> io::Result<Vec<u8>> {
    // Reject an empty fixture before the repeat loop: external file
    // deletion or corruption must be reported rather than spin forever.
    if source.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "empty parse corpus",
        ));
    }
    let mut bytes = Vec::with_capacity(CORPUS_BYTES + source.len());
    while bytes.len() < CORPUS_BYTES {
        bytes.extend_from_slice(source);
    }
    Ok(bytes)
}

/// Measures five warmed parser-and-grid feeds and stores the median MB/s.
fn main() -> io::Result<()> {
    let name = std::env::args()
        .nth(1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "expected corpus name"))?;
    let (source_path, result_path) = paths(&name)?;
    let source = std::fs::read(source_path)?;
    let bytes = corpus(&source)?;
    // Tmux's recorded streams are measured at their actual 160-column
    // geometry; shrinking them would change margin and scroll behavior.
    let (columns, rows, history, bytes_cap) = if name.starts_with("tmux-") {
        // The captured client and pane sizes are 160×48. A full 10,000
        // retained rows exposes O(history) work in a top-margin SU.
        (160, 48, 10_000, 32 * 1024 * 1024)
    } else {
        let rows = if name == "flood" { 40 } else { 24 };
        (80, rows, 100, 80 * 16 * 100)
    };
    let mut terminal = Terminal::new(columns, rows, history, bytes_cap);
    // Warming with one complete source stream fills scrollback and parser
    // buffers before timing repeated dashboard or color operations.
    terminal.advance(&source);
    let mut rates = [0.0_f64; 5];
    for rate in &mut rates {
        // The byte count includes both escape sequences and printable
        // data, matching the PTY throughput seen by Terminal::advance.
        let started = Instant::now();
        terminal.advance(black_box(&bytes));
        *rate = bytes.len() as f64 / started.elapsed().as_secs_f64() / 1_000_000.0;
    }
    // Save chronological samples before sorting; acceptance still uses the same median.
    std::fs::create_dir_all("target/corpus")?;
    std::fs::write(
        result_path.replace("-mbps.txt", "-samples.txt"),
        rates
            .iter()
            .map(|rate| format!("{rate:.6}\n"))
            .collect::<String>(),
    )?;
    rates.sort_by(f64::total_cmp);
    // The artifact is written after the timed loop and removed by xtask
    // before the next invocation, preventing stale-budget evidence.
    std::fs::write(result_path, format!("{:.3}\n", rates[2]))?;
    Ok(())
}
