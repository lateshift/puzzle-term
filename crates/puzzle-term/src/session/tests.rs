//! Deterministic feed, reply, input and idle scheduling at the Session seam.
//!
//! The manual clock advances per parser slice, so a 64 KiB boundary is
//! tested without relying on the workstation's speed. The transport keeps
//! each write separately to detect a reply that was delayed past a slice.
#![allow(
    clippy::expect_used,
    reason = "test assertions identify session boundaries"
)]
use super::*;
use crate::frame_builder::{FrameBuilder, context::TextContext};
use puzzle_frame::RenderFrame;
/// Drawing belongs to the test host, separate from mutable source state.
mod drawing;
use crate::platform::{Clock, PtySize, Transport, Waker};
use drawing::TestHost;
use puzzle_bus::{
    hooks::{CommandKinds, EventKinds, Flow, HookCtx, Interceptor, Observer},
    wire::ToWire,
};
use puzzle_vt::input::{InputModes, Key, KeyEvent};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
/// Shortcut hook priority, FIFO flushing and prompt-preserving actions.
mod bindings;
/// Cursor and SGR-5 timer behavior with a host-owned text context.
mod blink;
/// Leader introspection and local shortcut query ownership.
mod builtin_help;
/// Cursor UI, finite status and explicit suggestion acceptance.
mod builtin_visuals;
/// Clipboard gesture and OSC 52 policy cases, kept out of this large fixture.
mod clipboard;
/// Native MIME work must complete through messages without blocking output.
mod clipboard_mime;
/// Selection copy formats and dynamic shortcut availability.
mod copy;
/// Declarative content lifetimes and pointer ownership use the production Session API.
mod decorations;
/// Drop lifecycle and geometry use owner messages and ordered replies.
mod dnd;
/// Provisional composition and ordered focus commands share public bus policies.
mod dom;
/// Input policies remain available throughout native glyph construction.
mod external_state;
/// Deferred platform handoffs and browser ownership of native text presses.
mod host;
/// Activity is a per-session output edge, not a frame or focus observation.
mod host_activity;
/// Encoded default colours drive replaceable native chrome without synchronous reads.
mod host_colors;
/// Native find interfaces receive incremental counts without querying another owner.
mod host_search;
/// Shell command labels remain bounded through parser, bus and host event delivery.
mod host_shell;
/// Change-only host facts replace native policy and animation queries.
mod host_state;
/// Visible image source ownership and image-free idle guarantees.
mod images;
/// D-50 messages preserve source ownership, policies and host result ordering.
mod message;
/// Model painters use injected time and the same typed navigation/input surface.
mod model_overlays;
/// Owner-side pointer modes and geometry remain current across queued host input.
mod motion;
/// Shared notification policy, asynchronous report routing and toast interaction.
mod notifications;
/// Native host UI exclusions preserve grid geometry and move only floating chrome.
mod occlusion;
/// Opt-in observations retain parser behavior and warm allocation guarantees.
mod operation_metrics;
/// Optional OSC protocols keep reply and lifecycle order at parser boundaries.
mod osc;
/// Handler-scoped receipts capture ordered text gaps without changing the unused path.
mod osc_anchors;
/// Work deferral preserves the original parser boundary and caller-turn credits.
mod osc_turns;
/// Extension lifetimes and backend-neutral logical placement geometry.
mod overlays;
/// Outside-drag timelines reuse shared scrolling without idle work.
mod pointer;
/// Caller-owned frame slots seal rows, preferences and overlay sources together.
mod publication;
/// Owner-side wake acknowledgement preserves burst timing during render work.
mod publication_wake;
/// Completion-backed reply buffers preserve parser order and exact sequence consumption.
mod reply_credit;
/// Bounded literal query progress and retained-result navigation.
mod search;
/// Text-view midpoint boundaries and hard-row padding preferences.
mod selection_pointer;
/// Xterm program requests and shared host Shift-capture settings.
mod shift_capture;
/// Completed synchronized transactions remain observable under continuous output.
mod sync_boundary;
/// Host appearance reports and OSC override expiry at the Session seam.
mod theme;
/// Input following uses produced bytes rather than the physical key alone.
mod viewport;
/// Cursor visibility across host changes and the finite blink cutoff.
mod visibility;

/// Shared test transport records every distinct write call.
struct RecordingTransport(Arc<Mutex<Vec<Vec<u8>>>>);

/// Shared write log retained after the session takes transport ownership.
type Writes = Arc<Mutex<Vec<Vec<u8>>>>;

impl Transport for RecordingTransport {
    /// A separate vector per write exposes reply ordering after each slice.
    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.0.lock().expect("writes lock").push(bytes.to_vec());
        Ok(())
    }
    /// The session never resizes during these parser-only cases.
    fn resize(&mut self, _size: PtySize) -> Result<(), String> {
        Ok(())
    }
    /// Closing a recorder does not affect its captured history.
    fn close(&mut self) -> Result<(), String> {
        Ok(())
    }
}

/// Counts requested frames instead of starting an event loop.
struct ManualWaker(Arc<AtomicUsize>);

impl Waker for ManualWaker {
    /// A request is visible even before the next frame is built.
    fn request_frame(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
    /// No blink deadline is needed for the feed and key tests.
    fn wake_at(&mut self, _instant: Instant) {}
}

/// Each elapsed-time query advances one deterministic millisecond.
struct ManualClock(AtomicUsize);

impl Clock for ManualClock {
    /// The first 64 KiB slice consumes the one-millisecond budget.
    fn elapsed_since(&self, _start: Instant) -> Duration {
        Duration::from_millis(self.0.fetch_add(1, Ordering::Relaxed) as u64 + 1)
    }
}

/// Constructs one session whose transport remains inspectable by the test.
fn fixture() -> (TestHost, Writes, Arc<AtomicUsize>) {
    let writes = Arc::new(Mutex::new(Vec::new()));
    let wakes = Arc::new(AtomicUsize::new(0));
    let mut services = Services::new(
        Box::new(RecordingTransport(writes.clone())),
        Box::new(ManualWaker(wakes.clone())),
    );
    services.clock = Box::new(ManualClock(AtomicUsize::new(0)));
    let config = SessionConfig {
        parse_budget: Duration::from_millis(1),
        ..SessionConfig::default()
    };
    (TestHost::new(config, services), writes, wakes)
}

/// A budget expiry retains the next slice and preserves reply order (§6.4).
#[test]
fn feed_budget_and_replies_are_slice_ordered() {
    let (mut session, writes, wakes) = fixture();
    let mut bytes = b"\x1b[5n".to_vec();
    bytes.resize(65_536, b' ');
    bytes.extend_from_slice(b"\x1b[6n");
    let first = session.feed(&bytes, Instant::now());
    assert_eq!(
        (first.parsed_bytes, first.pending_bytes, first.slices),
        (65_536, 4, 1)
    );
    assert_eq!(*writes.lock().expect("writes"), [b"\x1b[0n".to_vec()]);
    let second = session.feed(&[], Instant::now());
    assert_eq!((second.parsed_bytes, second.pending_bytes), (4, 0));
    assert_eq!(writes.lock().expect("writes").len(), 2);
    assert!(wakes.load(Ordering::Relaxed) >= 2);
}

/// A scheduled display turn resumes queued bytes without another PTY packet.
#[test]
fn pending_suffix_resumes_on_next_frame() {
    let (mut session, writes, _) = fixture();
    let mut bytes = b"\x1b[5n".to_vec();
    bytes.resize(65_536, b' ');
    bytes.extend_from_slice(b"\x1b[5n");
    let outcome = session.feed(&bytes, Instant::now());
    assert_eq!(outcome.pending_bytes, 4);
    assert!(session.publish(Instant::now()).is_some());
    assert!(session.pending.is_empty());
    assert_eq!(
        *writes.lock().expect("writes"),
        [b"\x1b[0n".to_vec(), b"\x1b[0n".to_vec()]
    );
}

/// Key encoding is synchronous; the session needs no frame to send bytes.
#[test]
fn key_reaches_transport_before_any_frame() {
    let (mut session, writes, _) = fixture();
    // Consume the initial blank frame so only input behavior remains.
    assert!(session.publish(Instant::now()).is_some());
    session.submit(Command::Key(KeyEvent::press(Key::Up)), Origin::User);
    session.pump(Instant::now());
    assert_eq!(*writes.lock().expect("writes"), [b"\x1b[A".to_vec()]);
    assert!(session.publish(Instant::now()).is_none());
}

/// An undamaged session has no recurring presentation request (P2).
#[test]
fn idle_session_requests_no_frames() {
    let (mut session, _, wakes) = fixture();
    assert!(session.publish(Instant::now()).is_some());
    assert!(session.publish(Instant::now()).is_none());
    assert_eq!(wakes.load(Ordering::Relaxed), 0);
}

/// OSC and C0 facts reach typed observers after the parser slice drains.
#[test]
fn vt_facts_are_forwarded_in_order() {
    /// A test observer records public names without reconstructing VT state.
    struct Collect(WritesNames);
    /// Shared fact names reveal the bus's FIFO delivery order.
    type WritesNames = Arc<Mutex<Vec<String>>>;
    /// Every matching fact is borrowed before the next one is delivered.
    impl Observer<Command, Event> for Collect {
        /// A title, bell and prompt should each retain its distinct kind.
        fn observe(&mut self, event: &Event, _ctx: &mut HookCtx<'_, Command, Event>) {
            self.0
                .lock()
                .expect("facts lock")
                .push(event.type_name().into());
        }
    }
    let (mut session, _, _) = fixture();
    let names = Arc::new(Mutex::new(Vec::new()));
    session
        .bus_mut()
        .observe(EventKinds::ALL, 0, "facts", Collect(names.clone()))
        .expect("observer");
    session.feed(b"\x1b]2;hello\x07\x07\x1b]133;A\x07", Instant::now());
    assert_eq!(
        *names.lock().expect("fact names"),
        [
            "terminal/titleChanged",
            "terminal/bell",
            "shell/promptStarted"
        ]
    );
}

/// A platform exit follows the final parsed text and publishes one fact.
#[test]
fn child_exit_drains_before_session_exited() {
    /// Captures the concrete exit payload while the bus drains its fact lane.
    struct CollectExit(Arc<Mutex<Vec<ExitInfo>>>);
    impl Observer<Command, Event> for CollectExit {
        /// Only the exit fact carries the final child status.
        fn observe(&mut self, event: &Event, _ctx: &mut HookCtx<'_, Command, Event>) {
            if let Event::SessionExited(info) = event {
                self.0.lock().expect("exit facts").push(info.clone());
            }
        }
    }
    let (mut session, _, _) = fixture();
    let seen = Arc::new(Mutex::new(Vec::new()));
    session
        .bus_mut()
        .observe(EventKinds::ALL, 0, "exit", CollectExit(seen.clone()))
        .expect("observer");
    session.feed(b"done", Instant::now());
    let info = ExitInfo {
        code: Some(3),
        signal: None,
        runtime_ms: 500,
    };
    session.submit(Command::ChildExited(info.clone()), Origin::Platform);
    session.pump(Instant::now());
    let view = session.buffer();
    assert!(
        view.text(view.line_ids().expect("lines"), Default::default())
            .contains("done")
    );
    assert_eq!(*seen.lock().expect("facts"), [info]);
    assert!(session.close_requested());
}

/// Held and abnormal exits consume a key without writing to the dead child.
#[test]
fn held_exit_resets_keyboard_and_key_requests_close_without_write() {
    let (mut session, writes, _) = fixture();
    session.on_exit = OnExit::Hold;
    session.feed(b"\x1b[?1h\x1b[?2004h\x1b[>1u", Instant::now());
    let info = ExitInfo {
        code: Some(3),
        signal: None,
        runtime_ms: 500,
    };
    session.submit(Command::ChildExited(info), Origin::Platform);
    session.pump(Instant::now());
    assert_eq!(InputModes::from_terminal(session.terminal()).kitty_flags, 0);
    assert!(!InputModes::from_terminal(session.terminal()).application_cursor);
    assert!(!InputModes::from_terminal(session.terminal()).bracketed_paste);
    assert_eq!(session.terminal().modes.get(true, 25), Some(false));
    let view = session.buffer();
    assert!(
        view.text(view.line_ids().expect("lines"), Default::default())
            .contains("Press any key to close.")
    );
    let before = writes.lock().expect("writes").len();
    session.submit(Command::Key(KeyEvent::press(Key::Char('x'))), Origin::User);
    session.pump(Instant::now());
    assert!(session.close_requested());
    assert_eq!(writes.lock().expect("writes").len(), before);
}

/// A held abnormal exit scrolls a final error upward instead of erasing it.
#[test]
fn held_exit_preserves_full_screen_final_error() {
    let (mut session, _, _) = fixture();
    let mut output = Vec::new();
    for _ in 0..23 {
        output.extend_from_slice(&[b'x'; 79]);
        output.extend_from_slice(b"\r\n");
    }
    output.extend_from_slice(b"fatal: config error");
    session.feed(&output, Instant::now());
    session.submit(
        Command::ChildExited(ExitInfo {
            code: Some(1),
            signal: None,
            runtime_ms: 100,
        }),
        Origin::Platform,
    );
    session.pump(Instant::now());
    let view = session.buffer();
    let visible = view.text(view.line_ids().expect("lines"), Default::default());
    assert!(visible.contains("fatal: config error"));
    assert!(visible.contains("[Process exited with code 1]"));
    assert!(visible.contains("Press any key to close."));
    assert_eq!(session.terminal().primary.ring.scrollback_len(), 2);
}

/// Held output keeps reverse video while dropping only input negotiation.
#[test]
fn held_exit_keeps_display_modes_and_handles_unknown_code() {
    let (mut session, _, _) = fixture();
    session.on_exit = OnExit::Hold;
    session.feed(b"\x1b[?5h\x1b[?1h\x1b[?1000h", Instant::now());
    session.submit(
        Command::ChildExited(ExitInfo {
            code: None,
            signal: None,
            runtime_ms: 100,
        }),
        Origin::Platform,
    );
    session.pump(Instant::now());
    assert_eq!(session.terminal().modes.get(true, 5), Some(true));
    assert_eq!(session.terminal().modes.get(true, 1), Some(false));
    assert_eq!(session.terminal().modes.get(true, 1000), Some(false));
    let view = session.buffer();
    let visible = view.text(view.line_ids().expect("lines"), Default::default());
    assert!(visible.contains("[Process exited]"));
    assert!(!visible.contains("code -1"));
}

/// A forged API origin cannot close a live PTY by naming an internal kind.
#[test]
fn api_origin_cannot_end_child_session() {
    let (mut session, _, _) = fixture();
    session.submit(
        Command::ChildExited(ExitInfo {
            code: Some(0),
            signal: None,
            runtime_ms: 1,
        }),
        Origin::Api,
    );
    session.pump(Instant::now());
    assert!(session.exit_info().is_none());
}

/// OSC 52 uses the policy lane with trusted Terminal origin (§5.1).
#[test]
fn osc_clipboard_request_passes_through_interceptors() {
    /// Captures the source before vetoing an application clipboard request.
    struct Deny(Arc<Mutex<Vec<Origin>>>);
    /// A terminal's OSC cannot write around a registered host policy.
    impl Interceptor<Command, Event> for Deny {
        /// Only the typed clipboard request is relevant to this guard.
        fn intercept(
            &mut self,
            command: &mut Command,
            ctx: &mut HookCtx<'_, Command, Event>,
        ) -> Flow {
            if matches!(command, Command::ClipboardWrite(_)) {
                self.0
                    .lock()
                    .expect("origins lock")
                    .push(ctx.origin().expect("source"));
                return Flow::Stop("clipboard blocked by test policy");
            }
            Flow::Continue
        }
    }
    let (mut session, _, _) = fixture();
    let origins = Arc::new(Mutex::new(Vec::new()));
    session
        .bus_mut()
        .intercept(CommandKinds::ALL, 0, "deny-osc52", Deny(origins.clone()))
        .expect("policy");
    session.feed(b"\x1b]52;c;aGVsbG8=\x07", Instant::now());
    assert_eq!(*origins.lock().expect("origins"), [Origin::Terminal]);
}

/// Mode 2026 defers a partially updated grid but releases it after 200 ms.
#[test]
fn synchronized_output_waits_then_times_out() {
    let (mut session, _, wakes) = fixture();
    let start = Instant::now();
    assert!(session.publish(start).is_some());
    session.feed(b"\x1b[?2026hHELLO", start);
    assert_eq!(wakes.load(Ordering::Relaxed), 0);
    assert!(
        session
            .publish(start + Duration::from_millis(100))
            .is_none()
    );
    assert!(
        session
            .publish(start + Duration::from_millis(201))
            .is_some()
    );
    session.feed(b"\x1b[?2026l", start + Duration::from_millis(202));
    assert!(
        session
            .publish(start + Duration::from_millis(202))
            .is_some()
    );
}

/// Scroll actions and prompt navigation replay neutral recorded application output.
mod scrolling;

/// Grouped links are tied to acknowledged pixels and final security policy.
mod builtin_links;
/// Tracked selection, extraction and assembly regressions.
mod selection;

/// OSC command durations and policy-protected hover copying.
mod builtin_blocks;

/// Completed-frame observation has no hidden or idle refresh cost.
mod builtin_hud;

/// Typical complete overlay set retains optimized budgets and free idle turns.
mod overlay_budget;

/// Warm image assembly and unchanged publication retain all allocations (M15.4).
mod image_idle;

/// Single-use clipboard capability scope and revocation (design §16).
mod clipboard_grant;

/// UAPI contents, coalesced host facts and session-end lifetime.
mod context;

/// Trusted optional-handler controls preserve source and Session ownership.
mod osc_control;

/// Registered sources publish immutable overlay state and validate user gestures.
mod osc_presentation;
