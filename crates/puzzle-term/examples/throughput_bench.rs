//! Stage-isolated native VT and Session throughput over a host-supplied corpus.
//!
//! Setup and file reads stay outside five timed trials. The same byte stream
//! and 282×63 geometry run through Terminal and Session, with mode 2027 enabled
//! and disabled, and with the actual 1-KiB PTY granularity beside larger slices.
//! This separates parser/grid work from per-feed Session reconciliation without
//! introducing a renderer, an OS transport or a corpus-specific fast path.
use puzzle_term::{
    Session, SessionConfig,
    platform::{PtySize, Services, Transport, Waker},
};
use puzzle_vt::Terminal;
use std::{
    hint::black_box,
    io::Write,
    time::{Duration, Instant},
};

/// An unobserved transport discards only replies, never application output.
struct Sink;
/// Satisfies Session services without putting OS writes inside its clock.
impl Transport for Sink {
    /// Replies are irrelevant to this isolated byte-to-grid measurement.
    fn write(&mut self, _: &[u8]) -> Result<(), String> {
        Ok(())
    }
    /// No OS geometry exists in a headless parser trial.
    fn resize(&mut self, _: PtySize) -> Result<(), String> {
        Ok(())
    }
    /// There is no child process to close.
    fn close(&mut self) -> Result<(), String> {
        Ok(())
    }
}
/// Keeps damage reconciliation active while excluding display scheduling.
impl Waker for Sink {
    /// Damage is recorded normally; no display consumes it during parsing.
    fn request_frame(&mut self) {}
    /// No cursor or output timer is run inside a throughput trial.
    fn wake_at(&mut self, _: Instant) {}
}

/// Five chronological trials include every timed repetition, not selected minima.
fn measure(bytes: &[u8], csv: &mut String) {
    // Alternate the configured width policy without changing any corpus bytes.
    for unicode in [true, false] {
        // Include macOS read granularity and both product parse-slice bounds.
        for slice in [1024, 16384, 65536] {
            // Both owners use default product history bounds and matched geometry.
            let mut vt = Terminal::new(282, 63, 10_000, 32 * 1024 * 1024);
            vt.set_grapheme_width_default(unicode);
            let mut session = Session::new(
                SessionConfig {
                    columns: 282,
                    rows: 63,
                    grapheme_width_unicode: unicode,
                    // A headless owner must consume every offered chunk in its trial.
                    parse_budget: Duration::from_secs(60),
                    ..SessionConfig::default()
                },
                Services::new(Box::new(Sink), Box::new(Sink)),
            );
            // Native graphics opt-in must preserve the same text-only throughput.
            session.enable_images(320 * 1024 * 1024);
            // Alternate-screen output matches vtebench's setup and avoids history drift.
            vt.advance(b"\x1b[?1049h");
            session.feed(b"\x1b[?1049h", Instant::now());
            // Owners retain their warmed row allocations across repetitions.
            for trial in 0..5 {
                // Start immediately before the byte-to-grid loop, excluding setup.
                let started = Instant::now();
                for chunk in bytes.chunks(slice) {
                    // Keep the compiler from treating the supplied corpus as constant.
                    vt.advance(black_box(chunk));
                }
                // Capture VT wall time before Session runs on the same thread.
                let vt_ms = started.elapsed().as_secs_f64() * 1000.0;
                let started = Instant::now();
                for chunk in bytes.chunks(slice) {
                    black_box(session.feed(black_box(chunk), Instant::now()));
                }
                // Session time also includes normal feed and damage bookkeeping.
                let session_ms = started.elapsed().as_secs_f64() * 1000.0;
                // The first trial is retained too: this report exposes cold cluster growth.
                // CSV construction is deliberately outside both parser clocks.
                for (owner, ms) in [("vt", vt_ms), ("session", session_ms)] {
                    // Byte count and MiB units make the two owners directly comparable.
                    csv.push_str(&format!(
                        "{owner},{unicode},{slice},{trial},{},{ms:.6},{:.6}\n",
                        bytes.len(),
                        bytes.len() as f64 / (ms / 1000.0) / 1048576.0
                    ));
                }
            }
        }
    }
}

/// Repeats complete source bytes; it never invents a partial terminal sequence.
fn main() -> Result<(), String> {
    // Both paths are explicit so this probe cannot overwrite a shipped fixture.
    let path = std::env::args().nth(1).ok_or("expected corpus path")?;
    let output = std::env::args().nth(2).ok_or("expected CSV path")?;
    // Source I/O is setup, not terminal throughput.
    let source = std::fs::read(path).map_err(|e| e.to_string())?;
    if source.is_empty() {
        return Err("empty corpus".into());
    }
    // Repeat whole sequences to amortize the timer without cutting UTF-8.
    let mut bytes = Vec::new();
    while bytes.len() < 8 * 1024 * 1024 {
        bytes.extend_from_slice(&source);
    }
    // Formatting and result I/O occur only after each timed parser loop.
    let mut csv = String::from("owner,unicode,slice,sample,bytes,ms,mib_per_second\n");
    measure(&bytes, &mut csv);
    std::fs::write(&output, &csv).map_err(|e| e.to_string())?;
    std::io::stdout()
        .write_all(format!("throughput samples=60 path={output}\n").as_bytes())
        .map_err(|e| e.to_string())
}
