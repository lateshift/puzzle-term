//! Real PTY exit ordering and held-screen policy checks (§4.7).
//!
//! A shell writes through an OS PTY, then its output is read to EOF before
//! Session receives ChildExited. This witnesses the platform boundary that
//! a unit test with a prefilled byte slice cannot prove. Fixed commands and
//! a temporary grid keep the test independent of the user's shell files.
use super::{LocalPty, SpawnedPty, spawn};
use portable_pty::CommandBuilder;
use puzzle_term::{
    Origin, Session, SessionConfig,
    messages::{Command, ExitInfo},
    platform::{PtySize, Services, Transport, Waker},
    session::OnExit,
    vt::input::{Key, KeyEvent},
};
use std::{
    io::Read,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, RecvTimeoutError},
    },
    time::{Duration, Instant},
};

/// Counts writes so an exited session cannot silently send a later key.
struct Tracked {
    /// Actual PTY writer and resize handle.
    inner: LocalPty,
    /// Shared counter outlives Session's ownership of the transport.
    writes: Arc<AtomicUsize>,
}

impl Transport for Tracked {
    /// Records a successful transport write, including VT replies.
    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        // Count only writes accepted by the PTY, so a failed write cannot
        // masquerade as the post-exit input regression being tested below.
        self.inner.write(bytes)?;
        self.writes.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    /// Keeps the real child grid in sync with Session when requested.
    fn resize(&mut self, size: PtySize) -> Result<(), String> {
        self.inner.resize(size)
    }
    /// Releases the PTY writer after a drained child exit.
    fn close(&mut self) -> Result<(), String> {
        self.inner.close()
    }
}

/// Headless checks inspect state directly and request no GPU frames.
struct NoFrame;

impl Waker for NoFrame {
    /// Session's grid is inspected without rendering.
    fn request_frame(&mut self) {
        // The exit policy changes the grid synchronously; this probe reads it
        // directly so presentation timing cannot hide an ordering defect.
    }
    /// Child exit is bounded by the test's read timeout.
    fn wake_at(&mut self, _instant: Instant) {
        // The child clock is bounded by drain's deadline, not by a frame tick.
    }
}

/// Runs one independent acceptance criterion over an actual local child.
pub(crate) fn run(case: Option<&str>) -> Result<(), String> {
    // A named case lets xtask report every exit criterion independently.
    crate::logging::init(None)?;
    let case = case.ok_or("exit-probe needs close, hold, abnormal or no-write")?;
    if !["close", "hold", "abnormal", "no-write"].contains(&case) {
        return Err("unknown exit-probe case".into());
    }
    // All cases use a real shell and wait for the final read before dispatch.
    let (mut session, writes, info) = completed_child(case)?;
    match case {
        // The ordinary policy closes after a successful drain even when the
        // child's status is nonzero, provided it ran past the abnormal window.
        "close" if !session.close_requested() || info.code != Some(3) => {
            Err("ordinary exit did not close with code 3".into())
        }
        // Hold paints an explicit status line; the key criterion reuses that
        // same state and then probes whether it can still write to the PTY.
        "hold" | "no-write"
            if session.close_requested()
                || !grid(&session)?.contains("[Process exited with code 3]") =>
        {
            Err("hold notice or close policy is wrong".into())
        }
        // A rapid failure must remain visible even with the Close setting.
        "abnormal"
            if session.close_requested()
                || !grid(&session)?.contains("[Process exited with code 1]") =>
        {
            Err("early abnormal exit was not held".into())
        }
        "no-write" => check_key_after_exit(&mut session, &writes),
        _ => Ok(()),
    }
}

/// Returns the final status after feeding every OS read into Session.
fn completed_child(case: &str) -> Result<(Session, Arc<AtomicUsize>, ExitInfo), String> {
    let mut spawned = spawn_case(case)?;
    let started = spawned.started;
    // A reader thread permits a bounded main-thread wait without depending
    // on whether this OS reports terminal closure as EOF or EIO.
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || read_chunks(spawned.reader, tx));
    // The counter survives the move of the transport into Session, allowing
    // the final key test to observe any attempted post-exit send.
    let writes = Arc::new(AtomicUsize::new(0));
    let mut session = session_for_case(case, spawned.term_name, spawned.transport, &writes);
    // This ordering is the contract: all output reaches the VT before the
    // status command, including bytes buffered by the kernel at child exit.
    drain(&rx, &mut session)?;
    let before = grid(&session)?;
    // `done` is the final output witness; checking it before ChildExited
    // catches a platform that sends the status as soon as wait() completes.
    if case != "abnormal" && !before.contains("done") {
        return Err("last child output was not parsed before exit".into());
    }
    // Waiting after drain also preserves the observed exit code on platforms
    // where a PTY read can finish just before the process is fully reaped.
    let status = spawned.child.wait().map_err(|error| error.to_string())?;
    let info = ExitInfo {
        // A signal and a numeric exit code are mutually exclusive on the
        // wire; do not invent a code for a signaled process.
        code: status
            .signal()
            .is_none()
            .then(|| i32::try_from(status.exit_code()).ok())
            .flatten(),
        signal: status.signal().map(str::to_owned),
        // Saturation keeps the wire's u64 duration valid even if Instant's
        // wider millisecond representation exceeds it.
        runtime_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
    };
    // Platform origin is required because an API caller must not forge the
    // lifecycle event of a child owned by this session.
    session.submit(Command::ChildExited(info.clone()), Origin::Platform);
    session.pump(Instant::now());
    // The session must retain the same status that was sent to its bus.
    if session.exit_info() != Some(&info) {
        return Err("SessionExited status changed".into());
    }
    Ok((session, writes, info))
}

/// Spawns a fixed command with the same TERM setup as the live window.
fn spawn_case(case: &str) -> Result<SpawnedPty, String> {
    // The half-second case is beyond the default 250 ms abnormal window.
    // The instant exit isolates the abnormal-policy branch from output drain.
    let command_text = if case == "abnormal" {
        "exit 1"
    } else {
        "printf done; sleep 0.5; exit 3"
    };
    // A fixed shell command avoids depending on the user's login shell,
    // rc files, current terminal size or interactive prompt behavior.
    let mut command = CommandBuilder::new("/bin/sh");
    command.arg("-c");
    command.arg(command_text);
    // The PTY still needs a positive grid; the exact dimensions do not affect
    // the exit decision, so they stay fixed across all four cases.
    let size = PtySize {
        columns: 80,
        rows: 24,
        pixel_width: 800,
        pixel_height: 480,
    };
    // Use the platform's actual spawn path so TERM and the PTY reader behave
    // as they do in the live desktop window.
    spawn(command, size)
}

/// Selects the policy before any child output or status reaches Session.
fn session_for_case(
    case: &str,
    term_name: &'static str,
    transport: LocalPty,
    writes: &Arc<AtomicUsize>,
) -> Session {
    let policy = if case == "hold" || case == "no-write" {
        OnExit::Hold
    } else {
        OnExit::Close
    };
    // Policy is supplied before the child exits; no configuration mutation
    // can accidentally turn a failed Close test into a passing Hold test.
    Session::new(
        SessionConfig {
            term_name,
            on_exit: policy,
            ..SessionConfig::default()
        },
        Services::new(
            Box::new(Tracked {
                inner: transport,
                writes: Arc::clone(writes),
            }),
            Box::new(NoFrame),
        ),
    )
}

/// Reads every PTY chunk until Unix EIO or EOF, with a finite test deadline.
fn drain(rx: &mpsc::Receiver<Vec<u8>>, session: &mut Session) -> Result<(), String> {
    // A hung reader is a test failure rather than an indefinitely stalled
    // acceptance run; the normal child finishes in about half a second.
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if Instant::now() >= deadline {
            return Err("PTY child did not end within three seconds".into());
        }
        // Short receives let the deadline be checked even while the child
        // has not yet produced another byte or closed its slave descriptor.
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(bytes) => {
                // Preserve each OS read's order; session parsing may happen
                // before EOF but the exit command always follows all reads.
                session.feed(&bytes, Instant::now());
            }
            // Sender destruction is the explicit EOF/EIO indication.
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
            // An empty interval says nothing about whether the child exited.
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

/// Converts the public VT view into one searchable status witness.
fn grid(session: &Session) -> Result<String, String> {
    // The public buffer view is what a harness dump uses, so this checks the
    // same visible cells that a held user session would display.
    let view = session.buffer();
    let lines = view.line_ids().ok_or("exit probe has no visible lines")?;
    Ok(view.text(lines, Default::default()))
}

/// Confirms the closing key emits an event without touching the dead PTY.
fn check_key_after_exit(session: &mut Session, writes: &AtomicUsize) -> Result<(), String> {
    // Snapshot after all VT replies from the child have completed; only this
    // synthetic key could increase the count after that point.
    let before = writes.load(Ordering::Relaxed);
    session.submit(Command::Key(KeyEvent::press(Key::Char('x'))), Origin::User);
    session.pump(Instant::now());
    // A held session turns its next key into a close request, regardless of
    // whether the previous application had selected a keyboard protocol.
    if !session.close_requested() || writes.load(Ordering::Relaxed) != before {
        return Err("post-exit key reached the PTY or did not request close".into());
    }
    Ok(())
}

/// Sends each OS read as one chunk and closes the channel at EOF/EIO.
fn read_chunks(mut reader: Box<dyn Read + Send>, tx: mpsc::Sender<Vec<u8>>) {
    // The reader is deliberately independent of Session; the production
    // drain thread has this same responsibility before posting ChildExited.
    let mut buffer = [0_u8; 4096];
    while let Ok(count) = reader.read(&mut buffer) {
        // An EOF ends the sequence. A disconnected receiver means the test
        // already failed and there is no reason to keep reading the child.
        if count == 0 || tx.send(buffer[..count].to_vec()).is_err() {
            break;
        }
    }
}
