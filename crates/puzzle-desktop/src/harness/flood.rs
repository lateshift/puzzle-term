//! Headless M6 flood scenario using the production Session and GPU executor.
//!
//! A fake PTY repeats the checked-in 80×40 truecolor corpus as fast as the
//! fixed 64-buffer pool allows. The consumer handles key, resize and close events
//! before each four-millisecond parse turn, then submits at most one frame
//! per display interval. This exposes regressions in backlog limits and
//! event fairness without depending on an interactive shell or wall clock
//! animation content. The scenario counts real GPU submissions into a reused
//! offscreen attachment; live presentation remains a separate R15 review.
//!
//! The desktop shell has two independent queues: PTY bytes and OS events.
//! An exhausted buffer pool must block the reader while keyboard, resize and
//! close callbacks still reach the main thread. The fake PTY therefore
//! repeats the same retained stream until the pool itself blocks; no
//! sleep or output-rate guess regulates the producer. The parser sees at
//! most 16 KiB per slice from each 64 KiB allocation.
//!
//! The pool counts bytes when a read completes and subtracts each parsed
//! slice. The harness also samples a suffix retained by Session after its
//! parse budget. This checks the whole in-flight path rather than only the
//! channel's nominal slot capacity.
//!
//! The control thread injects a key, a resize and a close at distinct
//! times. Events are inspected before each bounded parse turn. Rendering
//! uses one retained GPU texture so a fresh texture allocation or readback
//! does not distort the frame interval; the actual window is reviewed with
//! DOOM-fire by the user after the mechanical gate passes.
//!
//! The key payload is ETX, the byte sent by Ctrl+C on a normal PTY. The
//! fake transport records that write rather than waiting for a display
//! submission, preserving the input-latency contract of P3. Resize calls
//! use the same Session API as window drag, so a parser monopolizing the
//! main loop delays the resize measurement even with no real OS window.
//! Closing intentionally discards any *remaining* fake output: that is
//! process teardown, distinct from dropping bytes in an active session.
//! The sender must unblock and join after the consumer drops, including
//! when it was waiting for one of the fixed empty buffers.
use crate::{
    font_data,
    pty_pool::{self, Consumer},
};
use puzzle_render::{FrameRenderer, Renderer};
use puzzle_term::{
    Session, SessionConfig,
    platform::{PtySize, Services, Transport, Waker},
};
use std::{
    fs,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    time::{Duration, Instant},
};
/// Bounded reader, control injection and one finite parser turn.
mod io;
use io::{handle_event, parse_turn, spawn_controls, spawn_producer};
/// Small report writer kept separate from the timed parser loop.
mod report;
use report::{write_metrics, write_storm_metrics};

/// Baseline modes share the current parser and renderer; only resize events differ.
#[derive(Clone, Copy)]
pub(super) enum Phase {
    /// Existing short flood acceptance with key, resize and close probes.
    Flood,
    /// Three seconds of output without size changes for throughput comparison.
    Steady,
    /// Three seconds with sixty alternating size events per second.
    Storm,
}

/// One device refresh at approximately 60 Hz; the budget only demands 30.
const REFRESH: Duration = Duration::from_micros(16_667);

/// Synthetic OS events have priority over output parsing on every turn.
enum Event {
    /// The reader queued output after the previous wake was consumed.
    Output,
    /// A user key must reach Transport before any frame work.
    Key(Instant),
    /// A drag resize must remain callable while the producer is blocked.
    Resize(Instant),
    /// Size event uses the same physical grid calculation as the live window.
    StormResize(Instant, [u32; 2]),
    /// Closing drops the receiver and releases a blocked producer.
    Close(Instant),
}

/// Waker stores only a dirty bit; the display turn coalesces requests.
struct FlagWaker(Arc<AtomicBool>);

impl Waker for FlagWaker {
    /// Many parsed chunks need only one next-refresh presentation.
    fn request_frame(&mut self) {
        self.0.store(true, Ordering::Release);
    }

    /// Blink timers cannot improve a continuous-output frame rate.
    fn wake_at(&mut self, _instant: Instant) {}
}

/// A writable fake PTY records the first key write after injection.
struct TimedTransport(Arc<AtomicBool>);

impl Transport for TimedTransport {
    /// The key's arrival is observable without a real child's scheduling.
    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        if bytes.contains(&3) {
            self.0.store(true, Ordering::Release);
        }
        Ok(())
    }

    /// Resize is driven by the same Session seam as a real PTY.
    fn resize(&mut self, _size: PtySize) -> Result<(), String> {
        Ok(())
    }

    /// Dropping the queue, rather than this handle, stops fake output.
    fn close(&mut self) -> Result<(), String> {
        Ok(())
    }
}

/// Counters serialized for independent acceptance criteria.
#[derive(Default)]
struct Metrics {
    /// GPU frame submissions while truecolor output continues.
    frames: usize,
    /// Longest gap between consecutive frame submissions in milliseconds.
    max_frame_gap_ms: f64,
    /// Key injection to fake PTY write in milliseconds.
    key_ms: Option<f64>,
    /// Resize injection to Session resize in milliseconds.
    resize_ms: Option<f64>,
    /// Close injection to event-loop stop in milliseconds.
    close_ms: Option<f64>,
    /// Largest channel plus Session backlog observed in bytes.
    max_pending: usize,
    /// Byte slices passed to the real VT parser.
    slices: usize,
    /// Number of bytes actually advanced through the VT parser.
    parsed_bytes: usize,
    /// Applied physical-size events, excluding the single flood latency probe.
    storm_resizes: usize,
    /// Every GPU submission interval, retained for the p99 baseline.
    frame_intervals_ms: Vec<f64>,
}

/// Borrowed channels and wake flags for one headless event-loop run.
struct LoopIo<'a> {
    /// Fixed PTY pool used by the live drain thread as well.
    output: &'a mut Consumer,
    /// Coalesced output and independent OS-like control events.
    events: &'a Receiver<Event>,
    /// True while one output wake awaits handling.
    queued: &'a AtomicBool,
    /// Confirms that the key reached the Transport seam.
    key_written: &'a AtomicBool,
    /// Coalesces Session's frame requests until the next refresh.
    frame_dirty: &'a AtomicBool,
}

/// One refresh gate and the timestamp of its previous GPU submission.
struct FrameTicker<'a> {
    /// Session damage coalesced until a display interval arrives.
    dirty: &'a AtomicBool,
    /// Previous submission time for the longest-gap check.
    last: Option<Instant>,
    /// Earliest instant when another submission is allowed.
    next: Instant,
    /// Independent cache owner; synthetic parsing does not lend a builder.
    drawing: super::drawing::Drawing,
}

impl FrameTicker<'_> {
    /// Submits at most one real GPU frame after the refresh deadline.
    fn submit(
        &mut self,
        renderer: &Renderer,
        executor: &mut FrameRenderer,
        view: &wgpu::TextureView,
        session: &mut Session,
        context: &mut puzzle_term::frame_builder::context::TextContext,
        metrics: &mut Metrics,
    ) -> Result<(), String> {
        let now = Instant::now();
        if now < self.next {
            return Ok(());
        }
        // Consuming a dirty bit means repeated parser wakes can produce
        // only one presentation at the next display interval.
        if self.dirty.swap(false, Ordering::AcqRel)
            && let Some(frame) = self.drawing.draw(session, now, context)?
        {
            executor
                .render_view(renderer, frame, view)
                .map_err(|error| error.to_string())?;
            if let Some(previous) = self.last {
                // A long blackout cannot hide inside an average fps.
                let interval = now.duration_since(previous).as_secs_f64() * 1_000.0;
                metrics.frame_intervals_ms.push(interval);
                metrics.max_frame_gap_ms = metrics.max_frame_gap_ms.max(interval);
            }
            self.last = Some(now);
            metrics.frames += 1;
        }
        // A slow turn does not trigger a burst of catch-up submissions.
        self.next = now + REFRESH;
        Ok(())
    }
}

/// Runs the synthetic process and writes a compact machine-readable report.
pub(super) fn run(renderer: &Renderer, output: &Path) -> Result<(), String> {
    let (elapsed, metrics) = phase(renderer, Phase::Flood)?;
    write_metrics(output, elapsed, &metrics)
}

/// Measures output alone and with sixty alternating resizes per second.
pub(super) fn run_storm(renderer: &Renderer, output: &Path) -> Result<(), String> {
    let (steady_time, steady) = phase(renderer, Phase::Steady)?;
    let (storm_time, storm) = phase(renderer, Phase::Storm)?;
    // The two phases use fresh sessions, PTY queues and GPU executors so
    // retained damage or parser state cannot bias the second measurement.
    write_storm_metrics(output, (steady_time, &steady), (storm_time, &storm))
}

/// Starts one fake PTY phase on the current main-thread parse architecture.
fn phase(renderer: &Renderer, mode: Phase) -> Result<(Duration, Metrics), String> {
    let corpus = fs::read("assets/fixtures/corpus/flood.vt")
        .map_err(|error| format!("read flood corpus: {error}"))?;
    // A failed fixture read cannot silently turn the test into an idle
    // session whose initial blank frame would satisfy a frame counter.
    // Complete font and GPU setup before the timed producer starts, just
    // as DOOM-fire starts only after an interactive terminal is open.
    let key_written = Arc::new(AtomicBool::new(false));
    let frame_dirty = Arc::new(AtomicBool::new(true));
    let (mut session, mut context) = flood_session(&key_written, &frame_dirty)?;
    // Real bundled glyphs and the retained atlas exercise the same
    // half-block ink path as the native terminal under DOOM-fire.
    let mut executor = FrameRenderer::new(renderer, wgpu::TextureFormat::Rgba8Unorm);
    // The storm alternates two surface extents. Reserve the larger target
    // so every scissor stays inside the attachment without a per-resize
    // texture allocation contaminating the baseline.
    let texture = renderer.offscreen_texture(1280, 960);
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    // A single attachment survives all refreshes. Recreating it would
    // make the benchmark about allocation rather than presentation.
    // The fake reader uses the same fixed pool as a live PTY, so this
    // scenario checks ownership and backpressure in the shipped path.
    let (producer_pool, mut consumer_pool) = pty_pool::pair();
    let (event_tx, event_rx) = mpsc::channel::<Event>();
    let queued = Arc::new(AtomicBool::new(false));
    // The wake flag bounds the OS-like event queue independently of the
    // byte queue: a full PTY must not hide an input callback behind wakes.
    let mut io = LoopIo {
        output: &mut consumer_pool,
        events: &event_rx,
        queued: &queued,
        key_written: &key_written,
        frame_dirty: &frame_dirty,
    };
    let began = Instant::now();
    // Start latency probes only after the Session and GPU are ready; a
    // cold shader compile must not masquerade as an output-flood stall.
    let producer = spawn_producer(corpus, producer_pool, event_tx.clone(), &queued);
    let controls = spawn_controls(event_tx, mode);
    let mut metrics = drive(
        renderer,
        &mut executor,
        &view,
        &mut session,
        &mut context,
        &mut io,
    )?;
    // The consumer ends on the explicit Close event, even if unread
    // truecolor chunks remain; waiting to empty an endless stream hangs.
    // Dropping the receiver releases a producer blocked by the bounded
    // channel; joining it proves close cannot strand the I/O thread.
    metrics.max_pending = metrics.max_pending.max(consumer_pool.peak_bytes());
    drop(consumer_pool);
    producer.join().map_err(|_| "flood producer panicked")?;
    controls.join().map_err(|_| "flood controls panicked")?;
    // The report is written only after both threads join, so its clean
    // shutdown line cannot pass while a blocked reader is still alive.
    Ok((began.elapsed(), metrics))
}

/// Constructs the same Session before either timing phase begins.
fn flood_session(
    key_written: &Arc<AtomicBool>,
    frame_dirty: &Arc<AtomicBool>,
) -> Result<(Session, puzzle_term::frame_builder::context::TextContext), String> {
    // The dirty bit starts set for the first visible background; later
    // Session damage requests redraw without enqueuing one event per cell.
    let services = Services::new(
        Box::new(TimedTransport(Arc::clone(key_written))),
        Box::new(FlagWaker(Arc::clone(frame_dirty))),
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

/// Gives OS events priority, then a bounded parser turn, then one frame.
fn drive(
    renderer: &Renderer,
    executor: &mut FrameRenderer,
    view: &wgpu::TextureView,
    session: &mut Session,
    context: &mut puzzle_term::frame_builder::context::TextContext,
    io: &mut LoopIo<'_>,
) -> Result<Metrics, String> {
    let mut metrics = Metrics::default();
    let mut ticker = FrameTicker {
        dirty: io.frame_dirty,
        last: None,
        next: Instant::now(),
        drawing: Default::default(),
    };
    let mut active = false;
    // `active` means a wake or expired parse budget left possible output;
    // it never forces polling after the bounded channel is empty (P2).
    loop {
        // Drain OS-like events before any parser or GPU work (§4.4).
        while let Ok(event) = io.events.try_recv() {
            active |= handle_event(event, session, io.key_written, &mut metrics)?;
        }
        // An input event may be generated while the reader blocks on a
        // full queue; it is handled before another four-millisecond slice.
        if metrics.close_ms.is_some() {
            break;
        }
        if active {
            // Clearing first lets a concurrent producer schedule the next
            // wake without racing a post-drain flag reset.
            io.queued.store(false, Ordering::Release);
            active = parse_turn(session, io.output, &mut metrics);
        }
        // A refresh gate gives parsing a bounded share while the fake
        // program continues filling the channel.
        ticker.submit(renderer, executor, view, session, context, &mut metrics)?;
        if !active && let Ok(event) = io.events.recv_timeout(Duration::from_millis(1)) {
            // Brief blocking yields CPU while output is momentarily idle,
            // but the next OS event wakes this loop without a frame timer.
            active |= handle_event(event, session, io.key_written, &mut metrics)?;
        }
    }
    Ok(metrics)
}
