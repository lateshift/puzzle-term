//! App shutdown cancels held Session ownership before joining a rendering actor.
use super::*;
use puzzle_term::messages::{Event, ExitInfo};

/// Retains rendering's sender while optionally holding a frame in progress.
struct FinalScreen {
    /// Like the live driver, this endpoint survives until renderer destruction.
    _session: Handle,
    /// Snapshot receiver lifetime also follows the rendering owner.
    _frames: Frames,
    /// Establishes that shutdown overlaps a real driver invocation.
    began: mpsc::Sender<()>,
    /// The test releases expensive frame work independently of Session exit.
    release: Option<mpsc::Receiver<()>>,
}
impl Driver for FinalScreen {
    /// A held frame models synchronous glyph preparation without sleeping.
    fn advance(&mut self, _: Pending, _: Instant) -> Option<Instant> {
        let _ = self.began.send(());
        if let Some(release) = self.release.take() {
            release
                .recv()
                .unwrap_or_else(|error| panic!("shutdown witness: {error:?}"));
        }
        None
    }
}

/// Both native ownership loops remain joinable under the harness deadline.
struct HeldOwner {
    /// Session is live even though its real child has reached drained EOF.
    started: session_thread::Started,
    /// The test joins the real PTY reader after final output and process reaping.
    drain: thread::JoinHandle<Result<(), String>>,
}

/// Flood output must drain before the live owner enters its final-screen wait.
fn held_owner() -> Result<HeldOwner, String> {
    let mut command = portable_pty::CommandBuilder::new("/bin/sh");
    // Raw output avoids PTY CR/LF rewriting so the byte count proves complete drainage.
    command.args(["-c", "stty raw -echo; i=0; while [ \"$i\" -lt 10000 ]; do printf 'output under flood\\r\\n'; i=$((i+1)); done"]);
    let child = crate::pty::spawn(
        command,
        PtySize {
            columns: 80,
            rows: 24,
            pixel_width: 800,
            pixel_height: 480,
        },
    )?;
    let session = Session::new(
        SessionConfig {
            on_exit: puzzle_term::session::OnExit::Hold,
            ..SessionConfig::default()
        },
        Services::new(Box::new(child.transport), Box::new(Quiet)),
    );
    let (mut producer, consumer) = crate::pty_pool::pair();
    let (exit, exits) = mpsc::channel();
    let (notify, wakes) = mpsc::channel();
    let started = session_thread::start(
        session,
        consumer,
        session_thread::Config {
            exit: Some(exits),
            notify: Some(Arc::new(move || {
                let _ = notify.send(());
            })),
            recorder: None,
            decoder: None,
        },
    )?;
    let drain = thread::spawn(move || {
        let mut reader = child.reader;
        // As in the native reader, macOS EIO terminates the final PTY read.
        while matches!(producer.read_once(reader.as_mut()), Ok(true)) {}
        let mut process = child.child;
        let status = process.wait().map_err(|error| error.to_string())?;
        assert_eq!(status.exit_code(), 0);
        exit.send(ExitInfo {
            code: Some(0),
            signal: None,
            runtime_ms: child.started.elapsed().as_millis() as u64,
        })
        .map_err(|error| error.to_string())?;
        // EOF follows every accepted output payload on the data channel.
        drop(producer);
        Ok(())
    });
    wait_held(&started, &wakes)?;
    Ok(HeldOwner { started, drain })
}

/// Observe held EOF through the host event surface, never by borrowing Session.
fn wait_held(started: &session_thread::Started, wakes: &mpsc::Receiver<()>) -> Result<(), String> {
    let deadline = Instant::now() + crate::test_support::HANG_GUARD;
    loop {
        started.events.begin_drain();
        if std::iter::from_fn(|| started.events.try_recv().ok())
            .any(|event| matches!(event, puzzle_term::HostEvent::Fact(Event::SessionExited(_))))
        {
            break;
        }
        wakes
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|error| error.to_string())?;
    }
    assert_eq!(started.handle.parsed_total(), 200_000);
    assert!(
        !started.worker.is_finished(),
        "child EOF must retain the final screen"
    );
    Ok(())
}

/// Both workers must join while the host still owns its sending capability.
fn shutdown_held(mid_frame: bool) -> Result<(), String> {
    let HeldOwner { started, drain } = held_owner()?;
    let mut actor = RenderThread::start()?;
    let (began, active) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    actor.add(Box::new(FinalScreen {
        _session: started.handle.clone(),
        _frames: started.frames,
        began,
        release: mid_frame.then_some(gate),
    }))?;
    active
        .recv_timeout(crate::test_support::HANG_GUARD)
        .map_err(|error| error.to_string())?;
    let retained_host = started.handle;
    // This is the app's order: cancellation, render join, native owner release,
    // then Session join. Neither join depends on dropping retained_host first.
    retained_host.shutdown();
    let (joined, completion) = mpsc::channel();
    let joiner = thread::spawn(move || {
        actor.shutdown();
        let result = started
            .worker
            .join()
            .unwrap_or_else(|error| panic!("shutdown witness: {error:?}"));
        drain
            .join()
            .unwrap_or_else(|error| panic!("drain witness: {error:?}"))
            .unwrap_or_else(|error| panic!("drain witness: {error:?}"));
        joined
            .send(result)
            .unwrap_or_else(|error| panic!("shutdown witness: {error:?}"));
    });
    if mid_frame {
        release
            .send(())
            .unwrap_or_else(|error| panic!("shutdown witness: {error:?}"));
    }
    completion
        .recv_timeout(crate::test_support::HANG_GUARD)
        .map_err(|error| error.to_string())??;
    drop(retained_host);
    joiner
        .join()
        .unwrap_or_else(|error| panic!("shutdown witness: {error:?}"));
    Ok(())
}

/// A held final screen cannot strand ordinary app shutdown after output EOF.
#[test]
fn held_child_shutdown_joins_all_threads() -> Result<(), String> {
    shutdown_held(false)
}

/// Synchronous render work may finish late, but ownership cancellation wakes its owner.
#[test]
fn held_child_shutdown_during_frame_joins_all_threads() -> Result<(), String> {
    shutdown_held(true)
}
