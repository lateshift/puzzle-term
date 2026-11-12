//! Ordered live writes under a child that does not read for two seconds.
//!
//! A PTY's kernel input buffer is much smaller than a pasted file. This
//! test forces the writer into a blocking OS call while the Session keeps
//! processing input and damage on the caller thread. A plain in-memory
//! transport cannot reproduce that backpressure or detect byte loss.
//!
//! The user's 50-ms requirement is about handling and enqueueing a key on
//! the main thread. Delivering that key to a child that is not reading the
//! preceding paste within 50 ms would require overtaking paste bytes, which
//! would corrupt terminal input order. The child therefore sleeps while the
//! key and two VT replies are enqueued, then drains them all into a file.
//!
//! The synthetic frame turns deliberately come from Session damage while
//! the child is still asleep. They prove that the PTY write no longer holds
//! the thread that would take snapshots. GPU presentation is separately
//! covered by the existing flood and frame acceptance tests; this test
//! isolates the writer backpressure that used to stop those frames.
//!
//! The test output is generated text and is read back byte for byte before
//! deletion. It contains no shell prompt or user data and is never added
//! to the source tree.
#![allow(
    clippy::expect_used,
    reason = "test assertions name the stalled PTY operation"
)]
use super::*;
use puzzle_term::theme::Appearance;
use puzzle_term::{
    Origin, Session, SessionConfig,
    messages::Command,
    platform::{Services, Waker},
    vt::input::{Key, KeyEvent},
};
use std::{fs, io::Read, path::Path, sync::mpsc, thread, time::Duration};

/// The session still takes and presents damaged frames during writer stalls.
struct FrameWaker;

/// The child reads the selected background hint before its first prompt.
#[test]
fn spawned_colorfgbg_matches_initial_appearance() {
    for (appearance, expected) in [(Appearance::Dark, *b"15;0"), (Appearance::Light, *b"0;15")] {
        let mut command = CommandBuilder::new("/bin/sh");
        command.arg("-c");
        command.arg("printf %s \"$COLORFGBG\"");
        let spawned = spawn_with_appearance(
            command,
            PtySize {
                columns: 80,
                rows: 24,
                pixel_width: 800,
                pixel_height: 480,
            },
            None,
            appearance,
            None,
        )
        .expect("spawn shell");
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = spawned.reader;
            let mut bytes = [0; 4];
            let result = reader.read_exact(&mut bytes).map(|()| bytes);
            let _ = sender.send(result);
        });
        // A missing environment variable must fail in bounded time rather
        // than hanging the entire acceptance command on a PTY read.
        assert_eq!(
            receiver
                .recv_timeout(crate::test_support::HANG_GUARD)
                .expect("COLORFGBG timeout")
                .expect("COLORFGBG read"),
            expected
        );
    }
}

impl Waker for FrameWaker {
    /// The test drives refresh deadlines explicitly instead of waking winit.
    fn request_frame(&mut self) {}
    /// A 400-ms sample ends before any cursor-blink wake is relevant.
    fn wake_at(&mut self, _instant: Instant) {}
}

/// Starts a raw child and waits until it advertises the deliberate pause.
fn waiting_child(output: &Path) -> (LocalPty, Box<dyn Child + Send + Sync>) {
    // The shell is a real PTY peer, rather than a fake Write implementation.
    // It controls when stdin drains and writes the result to one temp file.
    let mut command = CommandBuilder::new("/bin/sh");
    command.arg("-c");
    // Raw mode prevents canonical input buffering and echo from hiding the
    // writer's order. READY proves the child reached its two-second pause.
    // `head` reads an exact byte count and then exits, avoiding an EOF
    // dependency on the PTY master that must stay alive for this test.
    // The count is 10 MB of paste, one key and two protocol replies.
    command.arg("stty raw -echo; printf READY; sleep 2; head -c 10000011 > \"$PUZZLE_WRITER_OUT\"");
    command.env("PUZZLE_WRITER_OUT", output);
    // The environment carries the path as data, not as shell-interpolated
    // command text. Temp directories can contain spaces on user systems.
    let spawned = spawn(
        command,
        PtySize {
            rows: 24,
            columns: 80,
            pixel_width: 800,
            pixel_height: 480,
        },
    )
    .expect("spawn");
    let SpawnedPty {
        transport,
        mut reader,
        child,
        ..
    } = spawned;
    // Blocking read is isolated so a broken child's missing READY signal
    // fails with a bounded test timeout instead of hanging cargo check.
    let (ready_tx, ready_rx) = mpsc::channel();
    thread::spawn(move || {
        let mut marker = [0; 5];
        let result = reader.read_exact(&mut marker).map(|_| marker);
        let _ = ready_tx.send(result);
    });
    assert_eq!(
        ready_rx
            .recv_timeout(crate::test_support::HANG_GUARD)
            .expect("READY timeout")
            .expect("READY read"),
        *b"READY"
    );
    // No paste is enqueued before raw mode is active and the sleep starts.
    // Otherwise canonical line discipline could transform or echo bytes.
    (transport, child)
}

/// Waits for the complete child capture and checks both content and order.
fn verify_child_output(output: &Path, paste: &[u8]) {
    // The child begins draining after its sleep. File length is a cheap
    // completion signal, while a deadline prevents a lost-write deadlock
    // from leaving the test process alive indefinitely.
    let deadline = Instant::now() + crate::test_support::HANG_GUARD;
    while fs::metadata(output).map_or(true, |meta| meta.len() < 10_000_011) {
        assert!(
            Instant::now() < deadline,
            "child never consumed queued bytes"
        );
        thread::sleep(Duration::from_millis(20));
    }
    let bytes = fs::read(output).expect("read child output");
    // Compare every byte. A prefix-only check could miss tail loss or
    // reply reordering after the PTY output buffer fills a second time.
    fs::remove_file(output).expect("remove synthetic output");
    assert_eq!(&bytes[..paste.len()], paste);
    assert_eq!(&bytes[paste.len()..], b"K\x1b[0n\x1b[1;1R");
}

/// Keeps budget evidence small and outside any private PTY recording.
fn write_evidence(key_ms: f64, frame_turns: u32) {
    let evidence = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pty-writer");
    fs::create_dir_all(&evidence).expect("evidence directory");
    fs::write(
        evidence.join("metrics.txt"),
        format!("key_enqueue_ms={key_ms:.3}\nframe_turns={frame_turns}\nbytes=10000011\n"),
    )
    .expect("write metrics");
}

/// A slow reader cannot block key enqueue or reverse parser replies.
#[test]
fn ten_megabyte_paste_keeps_input_order_and_frame_turns() {
    paste_order(None);
}

/// Optimized enqueue retains the original 50-ms backpressure limit.
#[cfg_attr(debug_assertions, ignore = "requires the release profile")]
#[test]
fn paste_enqueue_budget() {
    paste_order(Some(50.0));
}

/// Shared byte-for-byte witness; debug enforces behavior without scheduler timing.
fn paste_order(budget_ms: Option<f64>) {
    // A unique process and clock suffix prevents parallel test processes
    // from truncating one another's synthetic output. The path stays under
    // the OS temp directory and is removed after byte comparison.
    let name = format!(
        "puzzle-writer-{}-{}.bin",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    );
    let output = std::env::temp_dir().join(name);
    let (mut transport, child) = waiting_child(&output);
    // The reader-side READY happened before this conversion. Only the live
    // transport uses the queued worker; smoke tests may still use direct I/O.
    transport.start_writer().expect("writer thread");
    // The first OS write cannot finish while the child sleeps because the
    // paste dwarfs the kernel PTY buffer. Later writes must queue behind it.
    let paste = vec![b'P'; 10_000_000];
    transport.write(&paste).expect("paste enqueue");
    // Session owns the same transport used by the real window. Running the
    // key through its bus and encoder checks more than raw Sender latency.
    let services = Services::new(Box::new(transport), Box::new(FrameWaker));
    let mut session = Session::new(SessionConfig::default(), services);
    let key_start = Instant::now();
    // Start timing at key dispatch, after the large paste has been queued.
    // Measuring delivery to the sleeping child would contradict byte order.
    session.submit(Command::Key(KeyEvent::press(Key::Char('K'))), Origin::User);
    session.pump(Instant::now());
    let key_ms = key_start.elapsed().as_secs_f64() * 1000.0;
    // The 50-ms budget protects event-loop responsiveness during backpressure.
    if let Some(limit) = budget_ms {
        crate::test_support::report(format_args!("M17_METRIC paste_enqueue_ms={key_ms:.6}"));
        assert!(key_ms <= limit, "key enqueue took {key_ms:.3} ms");
    }
    // Parser replies share the ordered queue with user input, even though
    // the child has not started reading the earlier paste.
    session.feed(b"\x1b[5n\x1b[6n", Instant::now());
    let frame_turns = publish_while_writer_blocked(&mut session);
    verify_child_output(&output, &paste);
    assert!(frame_turns >= 25);
    // Persist only numeric evidence, never the child's captured bytes.
    write_evidence(key_ms, frame_turns);
    // Keep the child handle alive until the byte comparison finishes so
    // portable-pty cannot reap or close its slave side during the sample.
    let _ = child;
}

/// Real damaged publications stay responsive while the ordered OS writer is blocked.
fn publish_while_writer_blocked(session: &mut Session) -> u32 {
    // Damaged Session frames still complete at refresh-like intervals while
    // the worker is blocked on the child's unread input stream.
    let context = puzzle_term::frame_builder::context::TextContext::new(
        crate::font_data::bundled_font_set().expect("fonts"),
        14.0,
        1.0,
    )
    .expect("context");
    session.set_text_context(&context);
    let mut slot = puzzle_term::frame_snapshot::FrameSnapshot::default();
    let mut frame_turns = 0;
    while frame_turns < 25 {
        // New damage must produce a real Session frame on each turn, not
        // merely increment an unrelated timer while the transport stalls.
        session.feed(b"x", Instant::now());
        assert_eq!(
            session
                .publish_frame(Instant::now(), &mut slot, context.atlas_generation(), 0)
                .expect("publication"),
            puzzle_term::PublishOutcome::Published
        );
        thread::sleep(Duration::from_millis(16));
        frame_turns += 1;
    }
    frame_turns
}

/// Real interactive shell snippets use the same PTY and Session composition.
#[cfg(unix)]
mod shell_integration;

/// Real SIGWINCH redraws preserve a fresh multiline shell prompt through resize.
#[cfg(unix)]
mod resize_prompt;

/// Child identity must describe Puzzle even when launched inside another terminal.
mod identity;

// Optional process tests are removed as a complete unit in a standard distribution.
include!("tests/optional_items.rs");
