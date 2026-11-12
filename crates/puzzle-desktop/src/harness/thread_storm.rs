//! Threaded D-50 flood and resize storm with real offscreen submissions.
//!
//! T0 uses the former main-thread architecture. This probe uses the live
//! pool, Session owner and rotating snapshot handoff, with the same checked
//! truecolor corpus and 180 physical resize events over three seconds.
//! Separate steady and storm sessions make throughput comparable. The
//! control thread injects events independently of VT and GPU timing, so a
//! stalled event loop cannot make its own resize workload disappear.
//! Unlike a visual review, mechanical ink is not the criterion here: the
//! retained frame executor must submit at least thirty frames per second.
//!
//! The producer models a PTY reader, not a renderer. Its repeating source
//! fills a buffer supplied by the fixed pool and cannot allocate a fresh
//! vector for each read. Once all 64 buffers are outstanding, the producer
//! blocks until the parser returns one, just as a real PTY drain blocks.
//! This is why the peak-pending counter must be read from the pool itself.
//!
//! The corpus begins with cursor-home and paints an 80-by-40 truecolor
//! screen. Repeating it is deliberately more demanding than sparse shell
//! output: most rows change on every update, which defeats row caching.
//! The source loops within one buffer as well as across buffer boundaries;
//! the VT parser must preserve escapes regardless of where the read split.
//!
//! There are two separate phases, each with a new Session, parser and GPU
//! executor. The steady phase receives the same truecolor stream without
//! resize events. The storm phase adds 180 physical size events at 60 Hz.
//! This prevents a warm renderer from explaining a throughput difference
//! between the two phases, and makes their parser rates comparable.
//!
//! A control thread owns the event clock. It sends resize notifications
//! independently of the main-loop simulator. If the UI stalls, events
//! queue up and their count remains visible; a UI-owned event generator
//! would slow itself during a stall and falsely report a gentle workload.
//! The synthetic Ctrl+C is injected at the midpoint of that same clock.
//! Its timestamp precedes the UI control send. The transport records receipt,
//! so latency includes queueing and owner scheduling rather than encoding alone.
//!
//! The transport deliberately records PTY size calls separately from VT
//! grid changes. The grid must track the latest visible physical surface
//! before a frame, while SIGWINCH takes the latest grid every first-event 25-ms window.
//! An application that queries its PTY dimensions after the final event
//! must therefore see the same columns and rows as the visible grid.
//! Intermediate drags may reflow visible cells, but they must not send
//! a matching burst of OS resize signals to the child process.
//!
//! The renderer targets one offscreen texture. This exercises the real
//! retained GPU pipelines without requiring a window server or a human
//! drag gesture, while leaving visual quality to the user's review items.
//! A completed submission counts as a frame; an idle redraw request does
//! not. The simulator offers one display opportunity every 16,667 µs and
//! never intentionally presents twice in that interval.
//!
//! Frame interval p99 records the tail, since a mean FPS alone could hide
//! visible long pauses. The report also keeps key latency, parse bytes,
//! peak pool backlog, delivered resize count, applied grid count and PTY
//! resize count as separate fields. A single success flag would conceal
//! whether a regression comes from parsing, event handling or graphics.
//! The report uses decimal MB/s as in the older T0 harness, so before and
//! after columns do not differ merely because of a unit conversion.
//!
//! The fake PTY accepts ETX as the Ctrl+C byte. Real keyboard identity and
//! kitty encoding have their own tests; this probe asks only whether an
//! immediate input write can pass through while output is saturated.
//! The mock waker does nothing because the simulator already schedules
//! display opportunities. In the live window, damage wakes winit instead.
//!
//! The three-second deadline is finite. The stop flag makes a blocked
//! producer eventually return EOF after it obtains another pool buffer;
//! the parser drains all previously accepted bytes and exits in order.
//! Joining both workers ensures the report cannot be written while a
//! background thread still owns part of the state being measured.
//!
//! This headless timing is a mechanical regression gate, not proof that
//! macOS's live-resize modal loop behaves identically. T5 asks the user to
//! compare actual release windows before and after D-29 while dragging.
//! The live logger records frame gaps and main-thread stalls for that
//! separate observation instead of inferring them from this harness.
use crate::{font_data, pty_pool, session_thread, window::grid_for_surface};
use puzzle_render::{FrameRenderer, Renderer};
use puzzle_term::{
    Origin, Session, SessionConfig,
    frame::RenderFrame,
    messages::Command,
    platform::{PtySize, Services, Transport, Waker},
};
use std::{
    fs,
    io::Read,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
/// Frame handling stays separate from the scenario and control generator.
mod runtime;
use runtime::Runtime;
/// One 60 Hz display opportunity, shared with the T0 headless probe.
const REFRESH: Duration = Duration::from_micros(16_667);
/// The independent controller emits one tick for each of three seconds.
const RESIZES: usize = 180;

/// A fixed stream starts again at cursor-home after each complete corpus.
struct RepeatingReader {
    /// One complete screen update repeated without a per-frame allocation.
    corpus: Vec<u8>,
    /// Next byte in the retained corpus stream.
    offset: usize,
    /// Close tells a backpressured reader to return EOF after its next slot.
    stop: Arc<AtomicBool>,
}
impl Read for RepeatingReader {
    /// The pool owns the destination, so the producer allocates no chunks.
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        if self.stop.load(Ordering::Acquire) {
            return Ok(0);
        }
        for byte in output.iter_mut() {
            *byte = self.corpus[self.offset];
            self.offset = (self.offset + 1) % self.corpus.len();
        }
        Ok(output.len())
    }
}

/// A fake PTY records keys and actual coalesced resize calls.
struct ProbeTransport {
    /// Confirms that a key was written before another GPU submission.
    key: Arc<Mutex<Option<Instant>>>,
    /// Every OS-size notification, retained for storm-coalescing checks.
    sizes: Arc<Mutex<Vec<PtySize>>>,
}
impl Transport for ProbeTransport {
    /// ETX is Ctrl+C and must reach the PTY before a frame is built.
    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        if bytes.contains(&3) {
            *self.key.lock().map_err(|error| error.to_string())? = Some(Instant::now());
        }
        Ok(())
    }
    /// Size calls expose whether intermediate drags caused SIGWINCH storms.
    fn resize(&mut self, size: PtySize) -> Result<(), String> {
        self.sizes
            .lock()
            .map_err(|error| error.to_string())?
            .push(size);
        Ok(())
    }
    /// The finite probe stops its own reader after its control deadline.
    fn close(&mut self) -> Result<(), String> {
        Ok(())
    }
}

/// The loop attempts frames at 60 Hz and lets Session decide whether dirty.
struct NoWake;
impl Waker for NoWake {
    /// Session damage is polled only by this bounded measurement loop.
    fn request_frame(&mut self) {}
    /// Blink is not needed while every grid cell changes continuously.
    fn wake_at(&mut self, _instant: Instant) {}
}

/// Resize and key events travel separately from the saturated PTY pool.
enum Control {
    /// Actual backing pixels from an independent control timer.
    Resize {
        /// Backing pixels at this independent controller event.
        size: [u32; 2],
        /// Controller clock, unaffected by how long the GPU holds the consumer.
        at: Instant,
    },
    /// Key timestamp starts before the main loop can take its turn.
    Key(Instant),
    /// Ends a finite phase without requiring fake output to drain first.
    Close,
}

/// Measured totals for one independent phase.
#[derive(Default)]
struct Metrics {
    /// Bytes advanced through VT during the three-second interval.
    bytes: u64,
    /// Completed GPU submissions, excluding idle redraw requests.
    frames: usize,
    /// Intervals between submissions expose long visual stalls.
    frame_intervals: Vec<f64>,
    /// Time from synthetic key event to transport write.
    key_ms: f64,
    /// Peak unread bytes held by the fixed pool.
    pending: usize,
    /// Independent resize events delivered to the UI queue.
    received: usize,
    /// Grid reflows performed after event coalescing.
    applied: usize,
    /// PTY size notifications from the 25-ms first-event throttle.
    pty_calls: usize,
    /// Visible columns and rows after the final resize.
    grid: (usize, usize),
    /// All publication answers retain request wait, wall/CPU work and deadline outcome.
    publications: Vec<session_thread::PublicationTiming>,
    /// Last PTY columns and rows sent to the fake transport.
    pty: (usize, usize),
}

/// Runs both modes and writes a machine-readable after artifact.
pub(super) fn run(renderer: &Renderer, output: &Path) -> Result<(), String> {
    let steady = phase(renderer, false)?;
    let storm = phase(renderer, true)?;
    let report = format!(
        "steady_throughput_mb_s={:.3}\nsteady_fps={:.3}\nsteady_frame_p99_ms={:.3}\nsteady_key_ms={:.3}\nsteady_pending_bytes={}\nstorm_throughput_mb_s={:.3}\nstorm_fps={:.3}\nstorm_frame_p99_ms={:.3}\nstorm_key_ms={:.3}\nstorm_pending_bytes={}\nstorm_resize_events={}\nstorm_grid_applies={}\nstorm_pty_calls={}\nstorm_grid_cols={}\nstorm_grid_rows={}\nstorm_pty_cols={}\nstorm_pty_rows={}\n",
        steady.bytes as f64 / 3e6,
        steady.frames as f64 / 3.0,
        p99(&steady.frame_intervals),
        steady.key_ms,
        steady.pending,
        storm.bytes as f64 / 3e6,
        storm.frames as f64 / 3.0,
        p99(&storm.frame_intervals),
        storm.key_ms,
        storm.pending,
        storm.received,
        storm.applied,
        storm.pty_calls,
        storm.grid.0,
        storm.grid.1,
        storm.pty.0,
        storm.pty.1,
    );
    fs::write(output.join("after.txt"), report).map_err(|error| error.to_string())?;
    // This finite evidence retains every answer rather than selecting the fastest run.
    let publications = serde_json::json!({
        "steady": steady.publications.iter().map(|sample| sample.report()).collect::<Vec<_>>(),
        "storm": storm.publications.iter().map(|sample| sample.report()).collect::<Vec<_>>(),
    });
    fs::write(
        output.join("owner-publications.json"),
        publications.to_string(),
    )
    .map_err(|error| error.to_string())
}

/// Conservative 99th percentile of actual frame-submission intervals.
fn p99(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let index = (sorted.len() * 99).div_ceil(100).saturating_sub(1);
    sorted.get(index).copied().unwrap_or(0.0)
}

/// One control thread delivers ticks independently of rendering progress.
fn controls(sender: mpsc::Sender<Control>, storm: bool) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let began = Instant::now();
        for tick in 0..RESIZES {
            let due = began + Duration::from_secs_f64(tick as f64 / 60.0);
            if let Some(wait) = due.checked_duration_since(Instant::now()) {
                thread::sleep(wait);
            }
            if tick == 90 {
                let _ = sender.send(Control::Key(Instant::now()));
            }
            if storm {
                let size = if tick % 2 == 0 {
                    [1024, 800]
                } else {
                    [1280, 960]
                };
                if sender.send(Control::Resize { size, at: due }).is_err() {
                    return;
                }
            }
        }
        let _ = sender.send(Control::Close);
    })
}

/// Creates a fresh threaded session, GPU executor and bounded producer.
fn phase(renderer: &Renderer, storm: bool) -> Result<Metrics, String> {
    let corpus = fs::read("assets/fixtures/corpus/flood.vt").map_err(|error| error.to_string())?;
    let key = Arc::new(Mutex::new(None));
    let sizes = Arc::new(Mutex::new(Vec::new()));
    let (session, context) = configured_session(&key, &sizes)?;
    let (mut producer, consumer) = pty_pool::pair();
    let peak = consumer.peak_handle();
    let started = session_thread::start(session, consumer, session_thread::Config::default())?;
    let stop = Arc::new(AtomicBool::new(false));
    let producer_stop = Arc::clone(&stop);
    let drain = thread::spawn(move || {
        let mut reader = RepeatingReader {
            corpus,
            offset: 0,
            stop: producer_stop,
        };
        while producer.read_once(&mut reader).is_ok_and(|read| read) {}
    });
    let mut executor = FrameRenderer::new(renderer, wgpu::TextureFormat::Rgba8Unorm);
    let texture = renderer.offscreen_texture(1280, 960);
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let (control_tx, control_rx) = mpsc::channel();
    let control = controls(control_tx, storm);
    let mut runtime = Runtime {
        owner: started.handle,
        frames: started.frames,
        events: started.events,
        builder: puzzle_term::frame_builder::FrameBuilder::new(&context),
        regions: Default::default(),
        grid: (80, 40),
        key_sent: None,
        publications: Vec::new(),
        executor: &mut executor,
        renderer,
        view: &view,
        frame: RenderFrame::default(),
        context,
        sizes,
        key,
    };
    let mut metrics = runtime.drive(control_rx)?;
    metrics.pending = peak
        .load(Ordering::Acquire)
        .max(runtime.owner.pending_bytes());
    stop.store(true, Ordering::Release);
    drain.join().map_err(|_| "thread-storm producer panicked")?;
    started
        .worker
        .join()
        .map_err(|_| "thread-storm owner panicked")??;
    control
        .join()
        .map_err(|_| "thread-storm control panicked")?;
    Ok(metrics)
}

/// Supplies fonts and a recording PTY before timing the parser or GPU.
fn configured_session(
    key: &Arc<Mutex<Option<Instant>>>,
    sizes: &Arc<Mutex<Vec<PtySize>>>,
) -> Result<(Session, puzzle_term::frame_builder::context::TextContext), String> {
    let services = Services::new(
        Box::new(ProbeTransport {
            key: Arc::clone(key),
            sizes: Arc::clone(sizes),
        }),
        Box::new(NoWake),
    );
    let mut session = Session::new(
        SessionConfig {
            rows: 40,
            scrollback_lines: 0,
            ..SessionConfig::default()
        },
        services,
    );
    let context = puzzle_term::frame_builder::context::TextContext::new(
        font_data::bundled_font_set()?,
        14.0,
        1.0,
    )?;
    session.set_text_context(&context);
    session.set_surface_size([1024, 800]);
    Ok((session, context))
}
