//! Geometry-controlled parser stage measurements for host-supplied byte recordings.
//!
//! The caller captures each program's setup and output through a PTY at the
//! requested geometry. This probe contains no knowledge of particular corpora.
//! Setup, file I/O and CSV formatting stay outside the measured feed intervals.
//! Five chronological samples retain cold and warmed behavior; no sample is
//! discarded. Terminal and Session expose the cost of source reconciliation.
use puzzle_term::{
    Session, SessionConfig,
    platform::{PtySize, Services, Transport, Waker},
};
use puzzle_vt::Terminal;
use std::{
    hint::black_box,
    time::{Duration, Instant},
};

/// Headless services keep all parser semantics while excluding operating-system I/O.
struct Sink;
/// Ordered replies are accepted without adding a kernel write to this stage.
impl Transport for Sink {
    /// The source recording is output only; responses have no live recipient.
    fn write(&mut self, _: &[u8]) -> Result<(), String> {
        Ok(())
    }
    /// Geometry is supplied to the model directly rather than to a child.
    fn resize(&mut self, _: PtySize) -> Result<(), String> {
        Ok(())
    }
    /// No external transport resource survives a trial.
    fn close(&mut self) -> Result<(), String> {
        Ok(())
    }
}
/// Damage remains active, but display wakes cannot add work to a headless trial.
impl Waker for Sink {
    /// Feed reconciliation runs normally without a rendering consumer.
    fn request_frame(&mut self) {}
    /// Model timers are excluded from this byte-to-grid stage.
    fn wake_at(&mut self, _: Instant) {}
}

/// Runs both source owners at one width, with complete setup preceding timing.
fn measure(setup: &[u8], bytes: &[u8], columns: usize, rows: usize) -> String {
    // Both owners use the production history limit at the caller's exact size.
    let mut vt = Terminal::new(columns, rows, 10_000, 32 * 1024 * 1024);
    let mut session = Session::new(
        // Session includes scheduling/source reconciliation absent from pure VT.
        SessionConfig {
            columns,
            rows,
            // A synchronous headless trial must consume the entire offered slice.
            parse_budget: Duration::from_secs(60),
            ..SessionConfig::default()
        },
        Services::new(Box::new(Sink), Box::new(Sink)),
    );
    // Setup controls the screen and history exactly as in the recorded program.
    vt.advance(setup);
    session.feed(setup, Instant::now());
    let mut csv = String::from("owner,columns,rows,sample,bytes,line_feeds,ms,ns_per_line\n");
    let lines = bytes.iter().filter(|&&b| b == b'\n').count();
    // The first run remains observable even when CPU/cache state is colder.
    for trial in 0..5 {
        let start = Instant::now();
        for chunk in bytes.chunks(16_384) {
            // The desktop owner uses this same maximum parser slice.
            vt.advance(black_box(chunk));
        }
        let vt_ms = start.elapsed().as_secs_f64() * 1000.0;
        // A distinct clock prevents pure-VT work being charged to Session.
        let start = Instant::now();
        for chunk in bytes.chunks(16_384) {
            black_box(session.feed(black_box(chunk), Instant::now()));
        }
        let session_ms = start.elapsed().as_secs_f64() * 1000.0;
        // CSV construction follows both clocks, and keeps the first trial too.
        for (owner, ms) in [("vt", vt_ms), ("session", session_ms)] {
            // Report actual supplied bytes rather than the source file's size.
            csv.push_str(&format!(
                "{owner},{columns},{rows},{trial},{},{lines},{ms:.6},{:.6}\n",
                bytes.len(),
                ms * 1e6 / lines.max(1) as f64
            ));
        }
    }
    csv
}

/// Explicit paths prevent an experiment from overwriting a shipped fixture.
fn main() -> Result<(), String> {
    // Collect only these small command-line values before starting any clocks.
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [setup, corpus, columns, rows, output] = args.as_slice() else {
        // Refuse omitted paths: a missing recording cannot become a passing trial.
        return Err("expected SETUP CORPUS COLUMNS ROWS OUTPUT".into());
    };
    let columns = columns.parse().map_err(|_| "invalid columns")?;
    // Preserve column/row order used by Terminal::new and the capture driver.
    let rows = rows.parse().map_err(|_| "invalid rows")?;
    let setup = std::fs::read(setup).map_err(|e| e.to_string())?;
    // File reads cannot inflate the reported byte-to-grid throughput.
    let source = std::fs::read(corpus).map_err(|e| e.to_string())?;
    if source.is_empty() {
        // Otherwise the whole-record replication below would never progress.
        return Err("empty source recording".into());
    }
    let mut bytes = Vec::new();
    // Whole-record repetitions never cut UTF-8 or an escape sequence in half.
    while bytes.len() < 8 * 1024 * 1024 {
        bytes.extend_from_slice(&source);
    }
    std::fs::write(output, measure(&setup, &bytes, columns, rows)).map_err(|e| e.to_string())
}
