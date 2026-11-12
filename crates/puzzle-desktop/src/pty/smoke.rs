//! Headless PTY bytes through the real Session parser for M6.4 (§13.1).
//!
//! This checks the OS master/slave pair and process spawning, not a fake
//! transport. The shell command is fixed and contains no user data; the
//! process is never driven by a window or by GPU presentation.
//! The test deliberately inspects the parsed `BufferView`, because seeing
//! bytes in the master alone would miss a broken Session parser connection.
//! The shell exits itself, so no signal or interaction with the user's
//! login shell is needed. The timeout turns an OS PTY failure into one
//! reported acceptance line rather than an indefinitely hung check.
use super::spawn;
use portable_pty::CommandBuilder;
use puzzle_term::{
    Session, SessionConfig,
    platform::{PtySize, Services, Waker},
};
use std::{
    io::Read,
    sync::mpsc::{self, RecvTimeoutError},
    time::{Duration, Instant},
};

/// Test waker: smoke has no GPU and needs no presentation deadline.
struct NoFrame;

impl Waker for NoFrame {
    /// Output is inspected directly through the Session buffer.
    fn request_frame(&mut self) {}
    /// The fixed command exits before any blink timer matters.
    fn wake_at(&mut self, _instant: Instant) {}
}

/// Spawns a shell that reports its initial PTY size and prints `hello`.
pub(crate) fn run() -> Result<(), String> {
    crate::logging::init(None)?;
    // This stable lifecycle record lets acceptance distinguish the console
    // stream from the optional file without opening a graphical window.
    tracing::info!(event = "desktop.smoke_started", "PTY smoke started");
    let mut command = CommandBuilder::new("/bin/sh");
    command.arg("-c");
    // The fixed printf avoids prompts, profile scripts and machine-specific
    // output that would make a cross-platform test nondeterministic.
    command.arg(
        "stty size; printf 'TERM=%s ' \"$TERM\"; tput -T xterm-puzzle Sync; printf hello; exit",
    );
    let size = PtySize {
        columns: 80,
        rows: 24,
        pixel_width: 800,
        pixel_height: 480,
    };
    let spawned = spawn(command, size)?;
    // A real PTY may deliver the command's five bytes over multiple reads;
    // the channel lets the same Session reassemble them in arrival order.
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || read_once(spawned.reader, sender));
    let mut session = Session::new(
        SessionConfig {
            term_name: spawned.term_name,
            ..SessionConfig::default()
        },
        Services::new(Box::new(spawned.transport), Box::new(NoFrame)),
    );
    // Session owns the master writer while the reader runs separately;
    // its parser sees identical input to the interactive window.
    // The reader thread avoids blocking forever on a broken PTY. The
    // three-second bound is generous for a local fixed command and lets
    // acceptance report a useful failure instead of hanging.
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        // A short per-read wait keeps the deadline observable even if the
        // child never writes, without scheduling a GPU frame or polling VT.
        let bytes = match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(bytes) => bytes,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(error) => return Err(format!("PTY smoke output: {error}")),
        };
        session.feed(&bytes, Instant::now());
        // Search the public buffer view rather than the transport bytes:
        // passing proves PTY output reached VT cells, not merely a channel.
        let view = session.buffer();
        let Some(lines) = view.line_ids() else {
            continue;
        };
        let text = view.text(lines, Default::default());
        if text.contains("24 80") && text.contains("TERM=xterm-puzzle") && text.contains("hello") {
            return Ok(());
        }
    }
    Err("PTY smoke did not display initial size and hello within three seconds".into())
}

/// Reads one process output chunk without tying the test to an exit status.
fn read_once(mut reader: Box<dyn Read + Send>, sender: mpsc::Sender<Vec<u8>>) {
    let mut buffer = [0_u8; 4096];
    while let Ok(count) = reader.read(&mut buffer) {
        if count == 0 || sender.send(buffer[..count].to_vec()).is_err() {
            break;
        }
    }
}
