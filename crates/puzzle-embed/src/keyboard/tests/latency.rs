//! Native adapter, owner-channel policy and actual queued PTY handoff latency.
//!
//! No window timing is simulated: this starts at the owned RawInput seam used
//! by winit and ends after LocalPty enqueues bytes. OS display/keyboard latency
//! and the child's later read are intentionally outside §15.2's endpoint.
#![allow(
    clippy::unwrap_used,
    reason = "a real PTY or measurement failure must fail the probe"
)]
use super::*;
use crate::{pty::LocalPty, session_thread};
use puzzle_term::{
    Origin, Session, SessionConfig,
    platform::{PtySize, Services, Transport, Waker},
};
use std::{
    io::Read,
    sync::{Arc, Mutex},
    time::Instant,
};

/// Timed calls retain their actual LocalPty transport instead of a memory sink.
struct TimedTransport {
    /// The same ordered writer queue used by the live window.
    inner: LocalPty,
    /// Timing metadata is test-only and never enters the core or native app.
    timing: Arc<Mutex<(Instant, Vec<f64>)>>,
}

/// Instrumentation surrounds the real enqueue, including its copy and send.
impl Transport for TimedTransport {
    /// Record only successfully handed-off bytes, after LocalPty returns.
    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        // A measured handoff must contain the expected legacy key, not an empty action.
        assert_eq!(bytes, b"\x1b[A");
        self.inner.write(bytes)?;
        let mut timing = self.timing.lock().unwrap();
        let elapsed = timing.0.elapsed().as_secs_f64() * 1000.0;
        timing.1.push(elapsed);
        Ok(())
    }
    /// Resizing does not participate in the key span.
    fn resize(&mut self, size: PtySize) -> Result<(), String> {
        self.inner.resize(size)
    }
    /// The test still owns and reaps its actual child.
    fn close(&mut self) -> Result<(), String> {
        self.inner.close()
    }
}

/// Headless input never needs a display frame or blink callback to reach the writer.
struct NoDisplay;
/// This probe deliberately never drives a frame, protecting immediate input.
impl Waker for NoDisplay {
    /// Requested damage remains independent of key-to-transport latency.
    fn request_frame(&mut self) {}
    /// The finite input probe does not service cursor blink timers.
    fn wake_at(&mut self, _: Instant) {}
}

/// The reported maximum includes every warmed press, rather than discarding outliers.
// This specified budget measures optimized code, not the workspace debug build.
#[cfg_attr(
    debug_assertions,
    ignore = "M7.7 latency budget requires the release profile"
)]
#[test]
fn key_to_queued_pty_budget() {
    let mut command = portable_pty::CommandBuilder::new("/bin/sh");
    command.arg("-c");
    // A finite reader exits after all 351 encoded three-byte keys, avoiding kill/startup races.
    command.arg("stty raw -echo; printf READY; dd bs=1 count=1053 of=/dev/null 2>/dev/null");
    let mut spawned = crate::pty::spawn(
        command,
        PtySize {
            columns: 80,
            rows: 24,
            pixel_width: 800,
            pixel_height: 480,
        },
    )
    .unwrap();
    // stty may flush pre-existing canonical input; wait until raw mode is established.
    let mut ready = [0; 5];
    spawned.reader.read_exact(&mut ready).unwrap();
    assert_eq!(&ready, b"READY");
    spawned.transport.start_writer().unwrap();
    let timing = Arc::new(Mutex::new((Instant::now(), Vec::with_capacity(351))));
    let services = Services::new(
        Box::new(TimedTransport {
            inner: spawned.transport,
            timing: timing.clone(),
        }),
        Box::new(NoDisplay),
    );
    let mut session = Session::new(SessionConfig::default(), services);
    let mut store = puzzle_term::settings::SettingsStore::default();
    store.set_binding_platform(puzzle_term::keybind::Platform::Mac, false);
    session.set_keybindings(store.keybindings());
    let (producer, consumer) = crate::pty_pool::pair();
    let started =
        session_thread::start(session, consumer, session_thread::Config::default()).unwrap();
    let mut adapter = Adapter::default();
    for _ in 0..351 {
        // Build the callback-shaped input outside the measured mapping/policy turn.
        let input = named("ArrowUp");
        timing.lock().unwrap().0 = Instant::now();
        let command = adapter.apply(input).unwrap();
        started
            .handle
            .send(puzzle_term::SessionMessage::Command(command, Origin::User))
            .unwrap();
        // A diagnostic round trip ends after the queued key, outside the measured span.
        started
            .handle
            .probe(Instant::now() + std::time::Duration::from_secs(1), |_| ())
            .unwrap();
    }
    let samples = timing.lock().unwrap().1.clone();
    assert_eq!(samples.len(), 351);
    report(&samples[50..]);
    // The writer drains all queued keys before the finite child exits normally.
    spawned.child.wait().unwrap();
    drop(producer);
    started.worker.join().unwrap().unwrap();
}

/// The artifact lets acceptance record numbers from a fresh release invocation.
fn report(samples: &[f64]) {
    let mut samples = samples.to_vec();
    samples.sort_by(f64::total_cmp);
    let median = samples[samples.len() / 2];
    let p99 = samples[(samples.len() - 1) * 99 / 100];
    let max = samples[samples.len() - 1];
    std::fs::write(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/m7-desktop-input.txt"),
        format!("median_ms={median:.6} p99_ms={p99:.6} max_ms={max:.6}\n"),
    )
    .unwrap();
    assert!(max <= 0.5, "native key maximum {max} ms > 0.5");
}

/// The saturated real-PTY cohort additionally retains paired owner clocks.
#[cfg(feature = "lock-metrics")]
mod flood;
