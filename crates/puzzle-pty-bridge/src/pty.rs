//! Blocking PTY workers and independent input/output byte bounds (§12.3).
//!
//! The output reader reserves credit before each OS read; an ACK wakes its
//! Condvar gate. The input writer consumes a one-MiB byte ring and reports
//! actual partial writes to the coordinator, which grants replacement credit.
//! Neither worker polls. The coordinator always reads socket controls, even
//! while the child is unable to read stdin or output awaits browser ACKs.
//! Input packet boundaries are immaterial to a PTY; coalescing small packets
//! into the ring prevents frame fragmentation from increasing queue metadata.
//! Output boundaries remain intact for optional recording and EOF ordering.
//! Closing both gates before child teardown releases every idle worker. A
//! blocked OS write ends when the original Child signals and reaps the shell.
use crate::child::ChildOwner;
use crate::{
    config::Config,
    flow::Flow,
    protocol::{PAYLOAD_CHUNK, Size},
    record::Recorder,
};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::{
    io::{Read, Write},
    sync::Arc,
    thread,
};
use tokio::sync::mpsc::{self, Receiver, Sender};

/// Reader outcomes are FIFO, so EOF cannot overtake the final output.
pub(crate) enum Output {
    /// One bounded read with its raw bytes, before encoding on the socket.
    Bytes(Vec<u8>),
    /// PTY EOF or Unix EIO; the socket then waits for the actual child status.
    Eof,
    /// A genuine I/O or recording failure terminates the session visibly.
    Failed(String),
}

/// A shell is owned by its authenticated socket, never by a detached global.
pub(crate) struct Pty {
    /// Resize remains on the session worker, serialized with incoming controls.
    pub master: Box<dyn MasterPty + Send>,
    /// Real status is retained until the output reader has delivered EOF.
    pub child: ChildOwner,
    /// FIFO bytes already charged against the one-MiB input allowance.
    /// This is byte capacity, not packet capacity: tiny frames cannot grow metadata.
    pub input: Arc<crate::input::Input>,
    /// Only actual OS writes restore client credit; errors wake the socket too.
    /// Capacity one prevents short writes from growing an unbounded completion list.
    pub written: Receiver<Result<u32, String>>,
    /// At most one queued output message, already charged to Flow.
    /// The reader blocks here while the async coordinator retains a prior chunk.
    pub output: Receiver<Output>,
    /// Socket closure wakes a reader sleeping on the acknowledgment gate.
    pub flow: Arc<Flow>,
}

/// Closing the flow gate precedes child teardown, releasing a paused reader.
impl Drop for Pty {
    /// ChildOwner then performs the platform-specific signal and reap.
    fn drop(&mut self) {
        self.flow.close();
        self.input.close();
    }
}

/// Opens the shell only after hello has selected the appearance (§14.7).
pub(crate) fn spawn(config: &Config, light: bool, record: Option<Recorder>) -> Result<Pty, String> {
    // Only Config supplies argv: a network packet cannot choose a program,
    // inject arguments or change the working directory of a spawned shell.
    let executable = config.shell.first().ok_or("empty shell command")?;
    let mut command = CommandBuilder::new(executable);
    command.args(&config.shell[1..]);
    environment(&mut command, light)?;
    // Hello has no size fields in v1. Use the conventional 80×24 until the
    // authenticated client sends its first validated physical-pixel resize.
    let pair = native_pty_system()
        .openpty(native_size(Size {
            cols: 80,
            rows: 24,
            width_px: 0,
            height_px: 0,
        }))
        .map_err(|error| format!("open bridge PTY: {error}"))?;
    // Clone/take the master endpoints before spawn. A failed descriptor
    // operation must not leave an already running login shell behind.
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| error.to_string())?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|error| error.to_string())?;
    // Register exit readiness before spawn so a fast child cannot exit unseen.
    let watch = crate::child::watch()?;
    let child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| error.to_string())?;
    // The two ledgers are independent: reading stdout consumes output credit,
    // while writing stdin returns input credit. Neither requires the other first.
    let flow = Arc::new(Flow::default());
    let input = Arc::new(crate::input::Input::new());
    // One completion is enough to wake the coordinator; the blocking writer
    // waits here until it can report accepted bytes without discarding a count.
    let (completed, written) = mpsc::channel(1);
    let (outgoing, output) = mpsc::channel(1);
    let pty = Pty {
        master: pair.master,
        child: ChildOwner::start(child, watch)?,
        input: input.clone(),
        written,
        output,
        flow: flow.clone(),
    };
    // Construct the cleanup guard before creating workers: a thread-start
    // failure must not leak a shell that has already inherited the slave.
    start_workers(reader, writer, flow, input, completed, outgoing, record)?;
    Ok(pty)
}

/// Starts independent blocking I/O after the socket owns the child guard.
fn start_workers(
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
    flow: Arc<Flow>,
    input: Arc<crate::input::Input>,
    completed: Sender<Result<u32, String>>,
    outgoing: Sender<Output>,
    record: Option<Recorder>,
) -> Result<(), String> {
    // Threads are native bridge details; the shared codec remains synchronous.
    // If either thread fails to start, spawn's guard hangs up the new child.
    thread::Builder::new()
        .name("bridge-input".into())
        .spawn(move || {
            // The scratch/ring retain byte order across partial OS writes.
            // A failed stdin endpoint ends the session visibly instead of
            // pretending the queued paste was delivered to the child.
            if let Err(error) = input.write(writer, &completed) {
                let _ = completed.blocking_send(Err(error));
            }
        })
        .map_err(|error| error.to_string())?;
    thread::Builder::new()
        .name("bridge-output".into())
        .spawn(move || {
            // Output and its EOF share one channel: child status cannot
            // bypass final bytes still waiting for a network sink slot.
            if let Err(error) = read_output(reader, &flow, &outgoing, record) {
                let _ = outgoing.blocking_send(Output::Failed(error));
            }
        })
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// Sets identity and the initial background hint before the first prompt.
fn environment(command: &mut CommandBuilder, light: bool) -> Result<(), String> {
    // Resolve bundled terminfo beside the workspace assets, not by installing
    // into HOME. Programs need Sync to avoid partial tmux updates (D-32).
    let terminfo =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/terminfo/compiled");
    let bundled =
        terminfo.join("78/xterm-puzzle").is_file() || terminfo.join("x/xterm-puzzle").is_file();
    // Both ncurses directory conventions ship; absence is a recoverable
    // compatibility fallback, not a reason to start with an unknown TERM.
    if bundled {
        command.env("TERMINFO", terminfo);
    } else {
        tracing::warn!(event = "bridge.terminfo_missing", "using xterm-256color");
    }
    command.env(
        "TERM",
        // Both ncurses directory conventions ship; absence is a recoverable
        // compatibility fallback, not a reason to start with an unknown TERM.
        if bundled {
            "xterm-puzzle"
        } else {
            "xterm-256color"
        },
    );
    // These hints describe the engine, not the local browser or shell. The
    // appearance must be fixed before programs first query their environment.
    command.env("COLORTERM", "truecolor");
    command.env("TERM_PROGRAM", "puzzle");
    command.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));
    command.env("COLORFGBG", if light { "0;15" } else { "15;0" });
    // HOME is inherited as a path, never interpolated into a shell command.
    let home = std::env::var_os("HOME").ok_or("HOME is required for bridge shell cwd")?;
    command.cwd(home);
    Ok(())
}

/// Reads exactly once per reservation, returning unused short-read credit.
fn read_output(
    mut reader: Box<dyn Read + Send>,
    flow: &Flow,
    outgoing: &Sender<Output>,
    mut record: Option<Recorder>,
) -> Result<(), String> {
    let mut buffer = vec![0; PAYLOAD_CHUNK];
    while flow.reserve()? {
        let count = match reader.read(&mut buffer) {
            Ok(count) => count,
            // On Unix an exhausted PTY commonly reports EIO instead of zero.
            Err(error) if cfg!(unix) && error.raw_os_error() == Some(5) => 0,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {
                flow.read_complete(0)?;
                continue;
            }
            Err(error) => return Err(format!("PTY read: {error}")),
        };
        flow.read_complete(count)?;
        if count == 0 {
            let _ = outgoing.blocking_send(Output::Eof);
            return Ok(());
        }
        // Record at the read boundary, before sending. Slow network flushes
        // must not distort the PTY timing captured in the optional .vtrec.
        if let Some(record) = &mut record {
            record.bytes(&buffer[..count])?;
        }
        // The channel blocks when full rather than dropping or growing.
        if outgoing
            .blocking_send(Output::Bytes(buffer[..count].to_vec()))
            .is_err()
        {
            return Ok(());
        }
    }
    Ok(())
}

/// Maps validated wire fields directly; there is no logical/device conversion.
/// portable-pty resize updates the kernel PTY and sends SIGWINCH on Unix.
/// The bridge has no fonts, so only the client can derive cells from pixels.
pub(crate) fn native_size(size: Size) -> PtySize {
    PtySize {
        cols: size.cols,
        rows: size.rows,
        pixel_width: size.width_px,
        pixel_height: size.height_px,
    }
}
