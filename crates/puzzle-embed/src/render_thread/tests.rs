//! D-41 isolation uses a real Session snapshot, native adapter and PTY child.
//!
//! Lifecycle witnesses hold a driver behind an explicit gate while host changes
//! arrive. The worker must retain the latest extent, stop callbacks after removal,
//! and release the driver on its own thread without making input wait for it.
use super::*;
use crate::{
    keyboard::Adapter,
    session_thread::{self, Frames, Handle},
};
use puzzle_term::{
    FrameResult, Origin, Session, SessionConfig, SessionMessage,
    frame::RenderFrame,
    frame_builder::FrameBuilder,
    frame_builder::context::TextContext,
    platform::{PtySize, Services, Waker},
};
use std::{
    io::Read,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

/// These finite witnesses drive damage explicitly, so no OS event loop is needed.
struct Quiet;
impl Waker for Quiet {
    /// The test queues actor work rather than creating a winit window.
    fn request_frame(&mut self) {}
    /// A test's bounded input/read deadlines replace blink timers.
    fn wake_at(&mut self, _instant: Instant) {}
}

/// The GPU restoration witness uses no child and has no application replies.
struct NullTransport;
impl puzzle_term::platform::Transport for NullTransport {
    /// GPU fixtures contain printable text only.
    fn write(&mut self, _bytes: &[u8]) -> Result<(), String> {
        Ok(())
    }
    /// Offscreen captures retain their initial extent.
    fn resize(&mut self, _size: PtySize) -> Result<(), String> {
        Ok(())
    }
    /// Destruction releases only test-owned rendering resources.
    fn close(&mut self) -> Result<(), String> {
        Ok(())
    }
}

/// A real row builder holds its independent owner until the key reaches the child.
struct SlowFrame {
    /// Commands reach the Session owner independently of this driver's work.
    session: Handle,
    /// Only the rendering owner receives the three circulating snapshot slots.
    frames: Frames,
    /// Caches stay with this driver while the Session continues independently.
    builder: FrameBuilder,
    /// This context moves with the driver onto the one rendering thread.
    context: TextContext,
    /// Signals after a complete snapshot arrives, before expensive frame work.
    began: mpsc::Sender<()>,
    /// Signals actual frame construction rather than an elapsed fake callback.
    finished: mpsc::Sender<Result<(), String>>,
    /// Explicit overlap makes the functional witness independent of scheduling.
    release: mpsc::Receiver<()>,
}
impl Driver for SlowFrame {
    /// A host-controlled pause models expensive work around real capture and assembly.
    fn advance(&mut self, _updates: Pending, now: Instant) -> Option<Instant> {
        let deadline = now + crate::test_support::HANG_GUARD;
        let captured = self.session.request_frame(
            now,
            self.context.atlas_generation(),
            self.builder.style_epoch(),
            deadline,
        );
        if captured.is_ok()
            && let Some(mut reply) = self.frames.receive_until(deadline)
        {
            let _ = self.began.send(());
            if self
                .release
                .recv_timeout(crate::test_support::HANG_GUARD)
                .is_err()
            {
                return None;
            }
            let built = self.builder.build_published(
                &mut self.context,
                &mut reply.slot,
                &mut RenderFrame::default(),
            );
            self.frames.recycle(reply.slot);
            if built.is_err() {
                let _ = self
                    .session
                    .send(SessionMessage::FrameResult(FrameResult::BuildFailed));
            }
            let _ = self.finished.send(built);
        }
        None
    }
}

/// Keeps the child alive until its timed byte acknowledgement has been observed.
struct KeyChild {
    /// Ordered writes use the same native writer as the live harness.
    transport: crate::pty::LocalPty,
    /// READY and hexadecimal input replies come from the real slave reader.
    output: mpsc::Receiver<std::io::Result<Vec<u8>>>,
    /// Dropping this only after the witness avoids racing portable-pty reaping.
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

/// Starts a raw child and waits for READY before measuring input to its slave.
fn key_child() -> Result<KeyChild, String> {
    let mut command = portable_pty::CommandBuilder::new("/bin/sh");
    command.args([
        "-c",
        "stty raw -echo; printf READY; dd bs=1 count=1 2>/dev/null | od -An -tx1 | tr -d ' \\n'",
    ]);
    let mut child = crate::pty::spawn(
        command,
        PtySize {
            columns: 80,
            rows: 24,
            pixel_width: 800,
            pixel_height: 480,
        },
    )?;
    let (read_tx, read_rx) = mpsc::channel();
    thread::spawn(move || {
        let mut ready = [0; 5];
        let result = child.reader.read_exact(&mut ready).map(|()| ready);
        let _ = read_tx.send(result.map(|r| r.to_vec()));
        let mut bytes = [0; 2];
        let result = child.reader.read_exact(&mut bytes).map(|()| bytes.to_vec());
        let _ = read_tx.send(result);
    });
    assert_eq!(
        read_rx
            .recv_timeout(crate::test_support::HANG_GUARD)
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?,
        b"READY"
    );
    child.transport.start_writer()?;
    Ok(KeyChild {
        transport: child.transport,
        output: read_rx,
        child: child.child,
    })
}

/// Routes the recorded macOS Ctrl+D sequence through the native adapter.
fn send_control_d(handle: &Handle) -> Result<(), String> {
    let mut adapter = Adapter::new(puzzle_term::session::OptionAsAlt::None);
    for raw in crate::keyboard::sequences::for_chord("ctrl+d")?.0 {
        if let Some(command) = adapter.apply(raw) {
            handle.send(SessionMessage::Command(command, Origin::User))?;
        }
    }
    Ok(())
}

/// Native encoding reaches the real child while rendering remains occupied.
#[test]
fn long_frame_does_not_delay_real_pty_key() -> Result<(), String> {
    long_frame_key(None)
}

/// The original child-receipt budget is asserted only in optimized code.
#[cfg_attr(debug_assertions, ignore = "requires the release profile")]
#[test]
fn long_frame_key_budget() -> Result<(), String> {
    long_frame_key(Some(Duration::from_millis(50)))
}

/// A channel gate proves actual overlap without relying on a fixed sleep.
fn long_frame_key(budget: Option<Duration>) -> Result<(), String> {
    let KeyChild {
        transport,
        output: read_rx,
        child: _child,
    } = key_child()?;
    let context = TextContext::new(crate::font_data::bundled_font_set()?, 14.0, 1.0)?;
    let mut session = Session::new(
        SessionConfig::default(),
        Services::new(Box::new(transport), Box::new(Quiet)),
    );
    session.set_text_context(&context);
    session.feed(b"long frame", Instant::now());
    let (producer, consumer) = crate::pty_pool::pair();
    let started_owner =
        session_thread::start(session, consumer, session_thread::Config::default())?;
    let (began, started) = mpsc::channel();
    let (finished, done) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let mut actor = RenderThread::start()?;
    actor.add(Box::new(SlowFrame {
        session: started_owner.handle.clone(),
        frames: started_owner.frames,
        builder: FrameBuilder::new(&context),
        context,
        began,
        finished,
        release: gate,
    }))?;
    started
        .recv_timeout(crate::test_support::HANG_GUARD)
        .map_err(|e| e.to_string())?;
    let key_start = Instant::now();
    send_control_d(&started_owner.handle)?;
    let output = read_rx
        .recv_timeout(crate::test_support::HANG_GUARD)
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    assert!(
        String::from_utf8_lossy(&output).contains("04"),
        "child received wrong encoded key: {output:?}"
    );
    if let Some(limit) = budget {
        key_receipt_budget(key_start.elapsed(), limit);
    }
    assert!(
        done.try_recv().is_err(),
        "input did not overlap the long frame"
    );
    release.send(()).map_err(|e| e.to_string())?;
    done.recv_timeout(crate::test_support::HANG_GUARD)
        .map_err(|e| e.to_string())??;
    actor.shutdown();
    drop(producer);
    started_owner
        .worker
        .join()
        .map_err(|_| "Session owner panicked")??;
    Ok(())
}

/// Reports the optimized child receipt before comparing its unchanged limit.
fn key_receipt_budget(elapsed: Duration, limit: Duration) {
    crate::test_support::report(format_args!(
        "M17_METRIC held_frame_key_ms={:.6}",
        elapsed.as_secs_f64() * 1000.0
    ));
    assert!(elapsed <= limit, "key receipt took {elapsed:?}");
}

/// Only this driver may execute or release its simulated surface resources.
struct Observed {
    /// Synchronizes updates with a deliberately occupied rendering turn.
    began: mpsc::Sender<()>,
    /// Reports the one coalesced geometry and damage state actually consumed.
    seen: mpsc::Sender<Pending>,
    /// Exact destruction count catches orphan drivers or duplicate releases.
    drops: Arc<AtomicUsize>,
    /// A first turn holds coalescing until the host has posted its complete batch.
    release: Option<mpsc::Receiver<()>>,
}
impl Driver for Observed {
    /// The second invocation reports mailbox data after the long first turn.
    fn advance(&mut self, updates: Pending, _now: Instant) -> Option<Instant> {
        if let Some(release) = self.release.take() {
            let _ = self.began.send(());
            let _ = release.recv_timeout(crate::test_support::HANG_GUARD);
        } else {
            let _ = self.seen.send(updates);
        }
        None
    }
}
impl Drop for Observed {
    /// A surface's last release must occur on the process render worker.
    fn drop(&mut self) {
        assert_eq!(thread::current().name(), Some("puzzle-render"));
        self.drops.fetch_add(1, Ordering::Release);
    }
}

/// Many resize/output events retain exactly the final extent and one damage flag.
#[test]
fn resize_and_close_during_flood_release_on_worker() -> Result<(), String> {
    resize_close(None)
}

/// The former one-second host-update bound stays unchanged in optimized code.
#[cfg_attr(debug_assertions, ignore = "requires the release profile")]
#[test]
fn resize_close_budget() -> Result<(), String> {
    resize_close(Some(Duration::from_secs(1)))
}

/// Functional teardown and release timing share the same mailbox flood.
fn resize_close(budget: Option<Duration>) -> Result<(), String> {
    let (began, started) = mpsc::channel();
    let (seen, observed) = mpsc::channel();
    let drops = Arc::new(AtomicUsize::new(0));
    let (release, gate) = mpsc::channel();
    let mut actor = RenderThread::start()?;
    let (id, host) = actor.add(Box::new(Observed {
        began,
        seen,
        drops: Arc::clone(&drops),
        release: Some(gate),
    }))?;
    started
        .recv_timeout(crate::test_support::HANG_GUARD)
        .map_err(|e| e.to_string())?;
    let now = Instant::now();
    for width in 1..=10_000 {
        host.update(SurfaceUpdate::Geometry(surface::SurfaceInfo {
            extent: surface::Extent { width, height: 700 },
            scale: 2.0,
            display: 1,
            millihertz: 120_000,
        }));
        host.update(SurfaceUpdate::Frame);
    }
    release.send(()).map_err(|e| e.to_string())?;
    let updates = observed
        .recv_timeout(crate::test_support::HANG_GUARD)
        .map_err(|e| e.to_string())?;
    assert_eq!(updates.geometry.map(|i| i.extent.width), Some(10_000));
    assert!(updates.frame);
    // Removal is asynchronous and cannot wait for another costly frame.
    actor.remove(id);
    if let Some(limit) = budget {
        let elapsed = now.elapsed();
        crate::test_support::report(format_args!(
            "M17_METRIC resize_close_ms={:.6}",
            elapsed.as_secs_f64() * 1000.0
        ));
        assert!(elapsed < limit, "resize/close took {elapsed:?}");
    }
    actor.shutdown();
    assert_eq!(drops.load(Ordering::Acquire), 1);
    host.update(SurfaceUpdate::Frame);
    assert!(
        observed.try_recv().is_err(),
        "stale handle called a removed surface"
    );
    Ok(())
}

/// Retained unpark tokens must not lose an update between a driver scan and sleep.
#[test]
fn idle_wake_is_not_lost_and_shutdown_joins() -> Result<(), String> {
    let (began, started) = mpsc::channel();
    let (seen, observed) = mpsc::channel();
    let drops = Arc::new(AtomicUsize::new(0));
    let mut actor = RenderThread::start()?;
    let (_, host) = actor.add(Box::new(Observed {
        began,
        seen,
        drops: Arc::clone(&drops),
        release: None,
    }))?;
    observed
        .recv_timeout(crate::test_support::HANG_GUARD)
        .map_err(|e| e.to_string())?;
    for _ in 0..100 {
        host.update(SurfaceUpdate::Frame);
        assert!(
            observed
                .recv_timeout(crate::test_support::HANG_GUARD)
                .map_err(|e| e.to_string())?
                .frame
        );
    }
    assert!(started.try_recv().is_err());
    actor.shutdown();
    assert_eq!(drops.load(Ordering::Acquire), 1);
    Ok(())
}

// Distribution-specific tests stay beside their implementations.
include!("tests/optional_items.rs");
/// Actual GPU device replacement restores a CPU mirror on the render worker.
mod recovery;
/// Host callbacks share a coalesced mailbox without inheriting native source identity.
mod refresh;
/// Held child EOF and an occupied frame cannot make shutdown depend on sender drops.
mod shutdown;

/// A nested GPU executor can consume the thread's global park token.
struct NestedWait {
    /// The host waits until the render turn has begun before posting damage.
    began: mpsc::Sender<()>,
    /// Lets that turn finish only after its outer actor has received a wake.
    release: mpsc::Receiver<()>,
    /// Reports the next turn, which must not be stranded by the nested wait.
    seen: mpsc::Sender<Pending>,
    /// Only the first invocation simulates a future executor's parking.
    first: bool,
}
impl Driver for NestedWait {
    /// Consumes the same token as block_on without adding a GPU dependency.
    fn advance(&mut self, updates: Pending, _: Instant) -> Option<Instant> {
        if self.first {
            self.first = false;
            self.began
                .send(())
                .unwrap_or_else(|error| panic!("shutdown witness: {error:?}"));
            self.release
                .recv()
                .unwrap_or_else(|error| panic!("shutdown witness: {error:?}"));
            // A completed inner future may have used an outer actor's unpark.
            thread::park_timeout(Duration::ZERO);
        } else {
            self.seen
                .send(updates)
                .unwrap_or_else(|error| panic!("shutdown witness: {error:?}"));
        }
        None
    }
}

/// Damage arriving during a GPU wait must wake the outer actor independently.
#[test]
fn nested_executor_cannot_consume_actor_wake() -> Result<(), String> {
    let (began, started) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let (seen, observed) = mpsc::channel();
    let mut actor = RenderThread::start()?;
    let (_, host) = actor.add(Box::new(NestedWait {
        began,
        release: gate,
        seen,
        first: true,
    }))?;
    started
        .recv_timeout(crate::test_support::HANG_GUARD)
        .map_err(|e| e.to_string())?;
    host.update(SurfaceUpdate::Frame);
    release.send(()).map_err(|e| e.to_string())?;
    let next = observed
        .recv_timeout(crate::test_support::HANG_GUARD)
        .map_err(|e| e.to_string())?;
    assert!(next.frame);
    actor.shutdown();
    Ok(())
}
