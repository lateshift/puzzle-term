//! Repeated reset/setup/feed stages for any host-supplied recording (M16.8).
//! These clocks distinguish explicit document retirement from normal parsing.
//! Input files are local diagnostics; no program or benchmark is recognized.
//! Every chronological trial is retained, including initial storage growth.
//! The reset phase feeds RIS, which deletes primary history and clears both
//! visible grids. The setup phase then rebuilds the recorded source state.
//! A warm row pool should turn setup growth into reuse without retaining any
//! old text in the public buffer. The feed phase measures ordinary parsing
//! separately, so reset/setup time cannot be mistaken for scalar print cost.
use puzzle_term::{
    Session, SessionConfig,
    platform::{PtySize, Services, Transport, Waker},
};
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

/// Each phase owns its clock; formatted results are produced after the feed ends.
fn measure(setup: &[u8], bytes: &[u8], columns: usize, rows: usize) -> String {
    // Construction stays outside every reset, setup and feed phase clock.
    let mut session = Session::new(
        SessionConfig {
            columns,
            rows,
            parse_budget: Duration::from_secs(60),
            // Headless delivery consumes each offered production-sized slice.
            ..SessionConfig::default()
        },
        Services::new(Box::new(Sink), Box::new(Sink)),
    );
    let mut csv =
        String::from("sample,phase,bytes,ms,new_rows,history_bytes,allocated_history_bytes\n");
    for trial in 0..10 {
        // Repeat explicit reset, rather than timing an already-populated owner.
        for (phase, data) in [
            ("reset", b"\x1bc".as_slice()),
            ("setup", setup),
            ("feed", bytes),
        ] {
            let before = session.terminal().primary.ring.storage_growth().rows;
            // Growth deltas distinguish initial allocation from storage reuse.
            // Retain the same maximum owner slice used by the production pipeline.
            let start = Instant::now();
            for chunk in data.chunks(16_384) {
                black_box(session.feed(black_box(chunk), Instant::now()));
            }
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            let ring = &session.terminal().primary.ring;
            // Live history and inactive allocations have separate meanings:
            // reset makes history empty while retained storage remains bounded.
            // Diagnostics and CSV allocations occur after the phase clock stops.
            csv.push_str(&format!(
                "{trial},{phase},{},{ms:.6},{},{},{}\n",
                data.len(),
                ring.storage_growth().rows - before,
                ring.history_bytes(),
                ring.allocated_history_bytes()
            ));
        }
    }
    csv
}

/// Explicit paths keep diagnostic input and output away from shipped assets.
fn main() -> Result<(), String> {
    // All filesystem and input replication work precedes the phase clocks.
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [setup, corpus, columns, rows, output] = args.as_slice() else {
        // A failed argument shape cannot accidentally run with default geometry.
        return Err("expected SETUP CORPUS COLUMNS ROWS OUTPUT".into());
    };
    let columns = columns.parse().map_err(|_| "invalid columns")?;
    let rows = rows.parse().map_err(|_| "invalid rows")?;
    let setup = std::fs::read(setup).map_err(|e| e.to_string())?;
    // These bytes are host-supplied; the probe never decodes a workload name.
    let source = std::fs::read(corpus).map_err(|e| e.to_string())?;
    if source.is_empty() {
        // Ensure the replication loop can terminate on each accepted recording.
        return Err("empty source recording".into());
    }
    let mut bytes = Vec::new();
    // Match the caller's one-MiB repeated workload without recognizing its content.
    while bytes.len() < 1024 * 1024 {
        bytes.extend_from_slice(&source);
    }
    std::fs::write(output, measure(&setup, &bytes, columns, rows)).map_err(|e| e.to_string())
}
