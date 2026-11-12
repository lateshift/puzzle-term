//! Bounded fake PTY producer and fair control-event handling (§4.4).
//!
//! A blocked producer retains bytes until the consumer resumes. Control
//! events use a separate queue so key, resize and close cannot wait behind
//! the truecolor stream. Session parses only one finite turn at a time.
use super::{Event, Metrics, Phase};
use crate::pty_pool::{Consumer, Producer};
use puzzle_term::{Origin, Session, messages::Command};
use std::{
    io::{self, Read},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
    thread,
    time::{Duration, Instant},
};

/// Cycles through the truecolor corpus without allocating a new chunk.
struct RepeatingReader {
    /// The complete frame includes cursor-home before each repetition.
    corpus: Vec<u8>,
    /// Next corpus byte to copy into a pool-owned read buffer.
    offset: usize,
}

impl Read for RepeatingReader {
    /// Fills a pool buffer even when the corpus boundary falls inside it.
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        // An OS PTY has no message boundaries. Crossing the corpus frame
        // boundary in one read reproduces that byte-stream contract, so
        // parser correctness cannot depend on one read equaling one frame.
        for byte in output.iter_mut() {
            *byte = self.corpus[self.offset];
            self.offset += 1;
            if self.offset == self.corpus.len() {
                self.offset = 0;
            }
        }
        Ok(output.len())
    }
}

/// Keeps the fake PTY full while respecting the fixed 4 MiB pool.
pub(super) fn spawn_producer(
    corpus: Vec<u8>,
    mut output: Producer,
    events: Sender<Event>,
    queued: &Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    let queued = Arc::clone(queued);
    thread::spawn(move || {
        let mut reader = RepeatingReader { corpus, offset: 0 };
        loop {
            // This is the production drain operation. The fake reader's
            // speed is intentionally unbounded so only returned buffers
            // regulate how far it can run ahead of the parser.
            // A closed consumer releases a producer waiting for an empty
            // vector; a full pool never drops a live terminal's bytes.
            if !output.read_once(&mut reader).is_ok_and(|read| read) {
                return;
            }
            if !queued.swap(true, Ordering::AcqRel) && events.send(Event::Output).is_err() {
                return;
            }
        }
    })
}

/// Injects the three latency probes independently of the busy reader.
pub(super) fn spawn_controls(events: Sender<Event>, mode: Phase) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        if !matches!(mode, Phase::Flood) {
            return three_second_controls(events, mode);
        }
        // Inject controls from a different thread than the blocked reader;
        // otherwise a full PTY queue would delay the *test* key itself.
        thread::sleep(Duration::from_millis(200));
        let _ = events.send(Event::Key(Instant::now()));
        // The resize arrives while the flood remains active, not after
        // Ctrl+C has had a chance to terminate the synthetic producer.
        thread::sleep(Duration::from_millis(300));
        let _ = events.send(Event::Resize(Instant::now()));
        // Closing at 800 ms leaves enough full-screen frames to estimate
        // the sustained interval while bounding the test's wall time.
        thread::sleep(Duration::from_millis(300));
        let _ = events.send(Event::Close(Instant::now()));
    })
}

/// Injects 180 size events at sixty hertz during the storm baseline.
fn three_second_controls(events: Sender<Event>, mode: Phase) {
    let began = Instant::now();
    // Absolute deadlines avoid accumulating `sleep` overshoot across 180
    // events; otherwise a busy machine could deliver substantially fewer
    // than sixty resizes per second and make the storm look easier.
    for tick in 0..180 {
        let target = began + Duration::from_secs_f64(tick as f64 / 60.0);
        if let Some(delay) = target.checked_duration_since(Instant::now()) {
            thread::sleep(delay);
        }
        if tick == 90 {
            // Probe input halfway through sustained resizing, when parser
            // and GPU work have reached their steady backlog rather than
            // measuring only a quiet startup or teardown instant.
            let _ = events.send(Event::Key(Instant::now()));
        }
        if matches!(mode, Phase::Storm) {
            // Alternating physical extents change both columns and rows;
            // identical sizes would exercise only surface configuration.
            let size = if tick % 2 == 0 {
                [1024, 800]
            } else {
                [1280, 960]
            };
            let _ = events.send(Event::StormResize(Instant::now(), size));
        }
    }
    let _ = events.send(Event::Close(Instant::now()));
}

/// Parses at most one four-millisecond turn and reports retained backlog.
pub(super) fn parse_turn(
    session: &mut Session,
    output: &mut Consumer,
    metrics: &mut Metrics,
) -> bool {
    // The stopwatch bounds a *whole* consumer turn, not a single chunk:
    // fast chunks must not let an endless producer monopolize input.
    let started = Instant::now();
    loop {
        // `try_recv` cannot wait for a producer: an idle parse turn must
        // give control back to the OS event loop immediately.
        let Some(bytes) = output.next_slice() else {
            return false;
        };
        let count = bytes.len();
        let outcome = session.feed(bytes, Instant::now());
        if output.consume(count).is_err() {
            return false;
        }
        // Count bytes the parser actually consumed. Channel receipt alone
        // would overstate throughput when Session retains a pending suffix.
        // Session checks its own time budget after each parser slice;
        // this outer limit caps total work over several channel reads.
        metrics.slices += outcome.slices;
        metrics.parsed_bytes += outcome.parsed_bytes;
        // Session can retain a suffix after its own parse budget. Count
        // that suffix alongside unread pool bytes, not only channel slots.
        metrics.max_pending = metrics
            .max_pending
            .max(output.pending_bytes() + outcome.pending_bytes);
        // Session may retain an unparsed suffix after a slow slice.
        if started.elapsed() >= Duration::from_millis(4) {
            // Return with the channel intact. The next loop iteration
            // handles queued input before consuming more output.
            return true;
        }
    }
}

/// Handles a control event before parsing, returning whether output woke us.
pub(super) fn handle_event(
    event: Event,
    session: &mut Session,
    key_written: &AtomicBool,
    metrics: &mut Metrics,
) -> Result<bool, String> {
    match event {
        Event::Output => return Ok(true),
        Event::Key(sent) => {
            // Submit and pump before considering a GPU frame: this is the
            // same immediate transport path as live winit key events.
            session.submit(Command::Text("\u{3}".into()), Origin::User);
            session.pump(Instant::now());
            if key_written.load(Ordering::Acquire) {
                // A timestamp without an actual transport write would let
                // an inert input encoder pass the latency criterion.
                metrics.key_ms = Some(sent.elapsed().as_secs_f64() * 1_000.0);
            }
        }
        Event::Resize(sent) => {
            // Updating geometry during flood detects a main-loop stall;
            // the fake PTY resize itself is intentionally cheap.
            session.resize(80, 40, [1024, 800])?;
            metrics.resize_ms = Some(sent.elapsed().as_secs_f64() * 1_000.0);
        }
        Event::StormResize(sent, size) => {
            // The event carries physical pixels, matching winit's
            // `Window::inner_size`; CSS or logical pixels would produce a
            // misleading grid at DPR two during the desktop comparison.
            // Use the live window's physical-pixel grid calculation and the
            // same Session resize path, including reflow and PTY resize.
            let cell = session.cell_metrics().ok_or("storm fonts missing")?;
            let physical = crate::render_thread::surface::Extent {
                width: size[0],
                height: size[1],
            };
            let grid = crate::window::grid_for_surface(physical, cell, 16);
            session.resize(usize::from(grid.columns), usize::from(grid.rows), size)?;
            metrics.resize_ms = Some(sent.elapsed().as_secs_f64() * 1_000.0);
            metrics.storm_resizes += 1;
        }
        Event::Close(sent) => metrics.close_ms = Some(sent.elapsed().as_secs_f64() * 1_000.0),
    }
    Ok(false)
}
