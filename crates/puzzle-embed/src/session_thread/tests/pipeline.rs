//! D-50 priority controls, exact drained bytes and ordered real-PTY replies.
//! Priority controls and sealed publications cross channels to one Session owner.
//! Publication clocks record queue residence and owner work separately.
use super::*;
use crate::{pty, pty_pool};
use portable_pty::CommandBuilder;
use std::sync::{Arc, Barrier};

/// Starts an output-only owner with a writer transaction receiver.
fn output_owner(config: SessionConfig) -> Result<(Started, pty_pool::Producer), String> {
    let (writes, _) = mpsc::channel();
    let session = Session::new(
        config,
        Services::new(Box::new(Writer(writes)), Box::new(Quiet)),
    );
    let (producer, consumer) = pty_pool::pair();
    Ok((start(session, consumer, Config::default())?, producer))
}

/// One hundred thousand control/data wake races retain every byte and completion.
#[test]
fn hundred_thousand_control_handoffs_finish() -> Result<(), String> {
    control_handoffs(None)
}

/// The original one-second limit measures optimized execution only.
#[cfg_attr(debug_assertions, ignore = "requires the release profile")]
#[test]
fn control_handoff_budget() -> Result<(), String> {
    control_handoffs(Some(Duration::from_secs(1)))
}

/// Functional and release witnesses send the same 100,000 controls and 8 MiB.
fn control_handoffs(budget: Option<Duration>) -> Result<(), String> {
    let (started, mut producer) = output_owner(SessionConfig::default())?;
    let barrier = Arc::new(Barrier::new(2));
    let producer_barrier = barrier.clone();
    let (controls_done, wait_controls) = mpsc::channel();
    let source = thread::spawn(move || -> Result<(), String> {
        let mut input = Cursor::new(vec![b'x'; 8 * 1024 * 1024]);
        producer_barrier.wait();
        while producer.read_once(&mut input)? {}
        // Finite EOF follows the final queued control, keeping the same race cohort live.
        wait_controls.recv().map_err(|error| error.to_string())?;
        Ok(())
    });
    let (finished_tx, finished_rx) = mpsc::channel();
    let handle = started.handle;
    let watcher = thread::spawn(move || {
        let drained = source.join().is_ok_and(|result| result.is_ok());
        let parsed = started.worker.join().is_ok_and(|result| result.is_ok());
        let _ = finished_tx.send(drained && parsed);
    });
    barrier.wait();
    let began = Instant::now();
    let deadline = began + crate::test_support::HANG_GUARD;
    for _ in 0..100_000 {
        // Repeated host turns cross the priority inbox, never borrow live VT.
        handle.send(SessionMessage::Command(
            Command::Focus(true),
            Origin::Platform,
        ))?;
    }
    controls_done.send(()).map_err(|error| error.to_string())?;
    assert!(
        finished_rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|error| error.to_string())?,
        "owner panicked during control handoff"
    );
    if let Some(limit) = budget {
        let elapsed = began.elapsed();
        crate::test_support::report(format_args!(
            "M17_METRIC control_handoffs_ms={:.6}",
            elapsed.as_secs_f64() * 1000.0
        ));
        assert!(elapsed <= limit, "100,000 handoffs took {elapsed:?}");
    }
    watcher.join().map_err(|_| "control watcher panicked")?;
    assert_eq!(handle.parsed_total(), 8 * 1024 * 1024);
    Ok(())
}

/// A real slave reads DA then CPR and prints their exact ordered hex bytes.
#[test]
fn real_pty_da_cpr_reply_order() -> Result<(), String> {
    let mut command = CommandBuilder::new("/bin/sh");
    command.arg("-c");
    command.arg("stty -echo -icanon min 1 time 0; printf '\\033[c\\033[6n'; dd bs=1 count=15 2>/dev/null | od -An -tx1 | tr -d ' \\n'; printf '\\n'");
    let mut spawned = pty::spawn(
        command,
        PtySize {
            columns: 80,
            rows: 24,
            pixel_width: 800,
            pixel_height: 480,
        },
    )?;
    spawned.transport.start_writer()?;
    let (mut producer, consumer) = pty_pool::pair();
    let session = Session::new(
        SessionConfig::default(),
        Services::new(Box::new(spawned.transport), Box::new(Quiet)),
    );
    let started = start(session, consumer, Config::default())?;
    let (observed_tx, drained_rx) = mpsc::channel();
    let drain = thread::spawn(move || {
        let mut reader = spawned.reader;
        while producer.read_once(reader.as_mut()).is_ok_and(|read| read) {}
        // The finite Session remains queryable until its final reply is observed.
        let _ = drained_rx.recv();
    });
    let deadline = Instant::now() + crate::test_support::HANG_GUARD;
    let mut observed = false;
    while Instant::now() < deadline && !started.worker.is_finished() {
        observed = started.handle.probe(deadline, |session| {
            let view = session.buffer();
            view.line_ids().is_some_and(|lines| {
                view.text(lines, Default::default())
                    .contains("1b5b3f36323b3232631b5b313b3152")
            })
        })?;
        if observed {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    observed_tx.send(()).map_err(|error| error.to_string())?;
    drain.join().map_err(|_| "PTY drain panicked")?;
    started.worker.join().map_err(|_| "PTY owner panicked")??;
    assert!(
        observed,
        "real PTY did not display ordered DA and CPR reply hex"
    );
    Ok(())
}

/// Posts a diagnostic observation during feed, to be applied before the next slice.
struct SliceWake {
    /// Sender installation is test-only and cannot borrow the owned Session.
    handle: mpsc::Receiver<Handle>,
    /// Each observation returns the actually consumed byte offset.
    observed: mpsc::Sender<u64>,
    /// Retains the installed sending endpoint across consecutive slices.
    installed: Option<Handle>,
}
impl Waker for SliceWake {
    /// A feed wake queues a priority control before more available data can parse.
    fn request_frame(&mut self) {
        if self.installed.is_none() {
            self.installed = self.handle.try_recv().ok();
        }
        if let Some(handle) = &self.installed {
            let observed = self.observed.clone();
            handle
                .post(Control::Probe(Box::new(move |session| {
                    let _ = observed.send(session.parsed_bytes_total());
                })))
                .unwrap_or_else(|error| panic!("{error}"));
        }
    }
    /// The finite three-slice fixture has no display or animation timer.
    fn wake_at(&mut self, _: Instant) {}
}

/// Available output cannot bypass the priority check between any two 16-KiB slices.
#[test]
fn available_slices_apply_controls_at_each_boundary() -> Result<(), String> {
    let (install, handle) = mpsc::channel();
    let (observed, received) = mpsc::channel();
    let (writes, _) = mpsc::channel();
    let session = Session::new(
        SessionConfig::default(),
        Services::new(
            Box::new(Writer(writes)),
            Box::new(SliceWake {
                handle,
                observed,
                installed: None,
            }),
        ),
    );
    let (mut producer, consumer) = pty_pool::pair();
    let started = start(session, consumer, Config::default())?;
    install
        .send(started.handle.clone())
        .map_err(|error| error.to_string())?;
    // Keep EOF out until all three feed-posted controls have been serviced.
    assert!(producer.read_once(&mut Cursor::new(vec![b'A'; 3 * pty_pool::SLICE_SIZE]))?);
    for slice in 1..=3 {
        assert_eq!(
            received
                .recv_timeout(crate::test_support::HANG_GUARD)
                .map_err(|error| error.to_string())?,
            (slice * pty_pool::SLICE_SIZE) as u64
        );
    }
    drop(producer);
    started.worker.join().map_err(|_| "owner panicked")??;
    assert_eq!(
        started.handle.parsed_total(),
        (3 * pty_pool::SLICE_SIZE) as u64
    );
    Ok(())
}
