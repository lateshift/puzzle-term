//! Local PTY ownership for the native M6.4 shell and smoke test (§13.1).
//!
//! The slave spawns a process with terminal identity variables. The master
//! yields one reader for an I/O thread and one writer for the Session.
//! The live host queues writes to one ordered worker: a child that stops
//! reading a large paste cannot hold the winit thread or reorder a later key.
//! The sole Session owner orders encoded keys with parser replies
//! before both enter this queue. The worker is the only OS writer, so a
//! partial PTY write cannot let a later reply overtake a paste. Closing
//! the transport drops the sender after already queued bytes; the worker
//! drains them or reports the first real write error. Recording taps the
//! successful OS write rather than a mere enqueue, preserving the meaning
//! of existing private PTY traces.
//! Queue instrumentation shares only counters with the owner, never a VT reference.
//! Resize calls stay behind the Transport seam so the Session and puzzle-vt
//! never import portable-pty. Window drag throttling belongs to the host.
//! The process environment advertises only modes puzzle-vt implements:
//! xterm-256color, truecolor and the product identity (§6.12). The dark
//! `COLORFGBG` hint matches the pre-theme M6 window; appearance switching
//! and paired themes follow in M6.6.
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize as NativeSize, native_pty_system};
use puzzle_term::platform::{PtySize, Transport};
use puzzle_term::theme::Appearance;
use std::io::{Read, Write};
use std::sync::mpsc::Sender;
/// Unix shell bootstrap construction stays outside the platform-neutral Session.
#[cfg(unix)]
mod integration;
/// Explicit shell, cwd and environment options preserve the login-shell startup path.
pub(crate) mod login;
pub use login::spawn_login_options;
/// Native shutdown can interrupt an idle descendant-held master without unsafe glue.
mod reader;
/// The blocking OS writer has explicit cancellation and join ownership.
mod writer;
use std::time::Instant;
pub use writer::WriterWorker;
/// Nested terminal identities must not leak into the child application's capability choice.
mod identity;
#[cfg(test)]
/// A real child proves that queued writes survive PTY backpressure in order.
mod tests;

/// Platform handles separated after a child starts on the PTY slave.
pub struct SpawnedPty {
    /// TERM actually advertised after checking for the bundled entry.
    pub term_name: &'static str,
    /// Session-owned writer and master resize handle.
    pub transport: LocalPty,
    /// Blocking output reader, moved to one dedicated I/O thread.
    pub reader: Box<dyn Read + Send>,
    /// Child lifetime and exit status retained by the native app.
    pub child: Box<dyn Child + Send + Sync>,
    /// Monotonic process start for the exit runtime reported after EOF.
    pub started: Instant,
}

/// Transport writes input and resizes the controlling terminal.
pub struct LocalPty {
    /// Master keeps the PTY open after the temporary spawn pair drops.
    master: Box<dyn MasterPty + Send>,
    /// Explicit teardown wakes the reader and bounds inherited-slave retention.
    read_cancel: reader::Cancel,
    /// Taking this once prevents two writers from reordering key bytes.
    writer: Option<Box<dyn Write + Send>>,
    /// Live-only ordered queue; the sole consumer writes bytes to the PTY.
    queued: Option<Sender<writer::Request>>,
    /// Optional diagnostic sink for exact successful writes and replies.
    recorder: Option<crate::recording::Recorder>,
    /// Opt-in observations end at successful enqueue, before any later OS write.
    metrics: Option<std::sync::Arc<crate::pipeline_metrics::Metrics>>,
}

impl LocalPty {
    /// Shares fixed diagnostic counters before Session takes this transport by value.
    pub fn set_metrics(&mut self, metrics: std::sync::Arc<crate::pipeline_metrics::Metrics>) {
        // The writer worker never reads these counters: enqueue completion is observed here.
        self.metrics = Some(metrics);
    }

    /// Adds a recording tap before Session takes transport ownership.
    pub fn set_recorder(&mut self, recorder: crate::recording::Recorder) {
        self.recorder = Some(recorder);
    }
}

impl Transport for LocalPty {
    /// Direct smoke writes complete inline; live writers return credit by message.
    fn deferred_replies(&self) -> bool {
        self.queued.is_some()
    }

    /// Binds the nonblocking owner mailbox before its first parser-generated reply.
    fn set_reply_completion(
        &mut self,
        complete: Box<dyn Fn(puzzle_term::platform::reply::ReplyCompletion) + Send + Sync>,
    ) {
        if let Some(writer) = &self.queued {
            let _ = writer.send(writer::Request::Completion(complete));
        }
    }

    /// Charges remain with Session until the worker completes the entire OS write.
    fn write_reply(
        &mut self,
        reply: puzzle_term::platform::reply::ReplyWrite,
    ) -> Result<puzzle_term::platform::reply::ReplyDelivery, String> {
        use puzzle_term::platform::reply::ReplyDelivery;
        if let Some(writer) = &self.queued {
            writer
                .send(writer::Request::Reply(reply))
                .map_err(|error| format!("queue PTY reply: {error}"))?;
            return Ok(ReplyDelivery::Pending);
        }
        // Direct headless transports have no asynchronous queue to account for.
        self.write(&reply.bytes)?;
        Ok(ReplyDelivery::Complete(reply.ticket))
    }

    /// Sends encoded user input without waiting for a frame.
    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        if let Some(writer) = &self.queued {
            // The sole Session owner gives keys and parser replies one producer
            // order; only this worker may perform blocking OS writes.
            // Copying the slice releases Session's borrow before the child
            // chooses to read it, so a sleeping program cannot stall UI work.
            writer
                .send(writer::Request::Bytes(bytes.to_vec()))
                .map_err(|error| format!("queue PTY write: {error}"))?;
            // The diagnostic end is this actual queue send, excluding OS writer scheduling.
            if let Some(metrics) = &self.metrics {
                metrics.control.queued();
            }
            return Ok(());
        }
        self.writer
            .as_mut()
            .ok_or("PTY writer was closed")?
            .write_all(bytes)
            .map_err(|error| format!("PTY write: {error}"))?;
        // Log only successful writes, with exact byte values. Input and
        // VT query replies may interleave, so the shared target lets the
        // reviewer correlate each write with preceding winit callbacks.
        tracing::debug!(target: "puzzle_desktop::input", event = "PtyWrite", byte_count = bytes.len());
        if let Some(recorder) = &self.recorder
            && let Err(error) = recorder.bytes("pty-write", bytes)
        {
            tracing::warn!(event = "desktop.record_failed", %error);
        }
        Ok(())
    }

    /// Applies the host's throttled grid and pixel extent to the OS PTY.
    fn resize(&mut self, size: PtySize) -> Result<(), String> {
        self.master
            .resize(native_size(size))
            .map_err(|error| format!("PTY resize: {error}"))
    }

    /// Dropping the sole writer signals EOF to a still-running child.
    fn close(&mut self) -> Result<(), String> {
        if let Some(queue) = self.queued.take() {
            // Close follows already queued input in the sole producer order.
            let _ = queue.send(writer::Request::Stop);
        }
        self.writer.take();
        self.read_cancel.stop();
        Ok(())
    }
}

/// Maps the neutral session's physical extent to portable-pty fields.
fn native_size(size: PtySize) -> NativeSize {
    // PTY cell counts and pixel dimensions are both conveyed: full-screen
    // programs use the former for layout and may query the latter for UI.
    NativeSize {
        rows: size.rows,
        cols: size.columns,
        pixel_width: size.pixel_width,
        pixel_height: size.pixel_height,
    }
}

/// Spawns the user's configured login shell with terminal identity values.
pub fn spawn_login(size: PtySize, appearance: Appearance) -> Result<SpawnedPty, String> {
    // Both native hosts use the same detected shell startup policy.
    spawn_login_options(size, appearance, None, None, &[])
}

/// Creates one PTY pair for either an interactive shell or a smoke child.
pub fn spawn(command: CommandBuilder, size: PtySize) -> Result<SpawnedPty, String> {
    spawn_with_term(command, size, None)
}

/// Overrides TERM only for conformance captures against the legacy entry.
pub fn spawn_with_term(
    command: CommandBuilder,
    size: PtySize,
    requested: Option<&'static str>,
) -> Result<SpawnedPty, String> {
    spawn_with_appearance(command, size, requested, Appearance::Dark, None)
}

/// Advertises the selected theme's background before the first shell prompt.
fn spawn_with_appearance(
    mut command: CommandBuilder,
    size: PtySize,
    requested: Option<&'static str>,
    appearance: Appearance,
    extra: Option<&std::path::Path>,
) -> Result<SpawnedPty, String> {
    // Standard terminal identity allows vim, less and color-aware programs
    // to negotiate the modes implemented by puzzle-vt (§6.12).
    let term_name = advertise_terminal(&mut command, requested, appearance);
    // Additional cold child policy follows identity cleanup, before the process exists.
    identity::apply_extra(&mut command, extra);
    let pair = native_pty_system()
        .openpty(native_size(size))
        .map_err(|error| format!("open PTY: {error}"))?;
    let started = Instant::now();
    let child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| format!("spawn PTY child: {error}"))?;
    let (reader, read_cancel) = reader::pair(pair.master.as_ref())?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|error| format!("take PTY writer: {error}"))?;
    // The slave drops here; only the child's inherited slave descriptors
    // remain, so EOF is observable when it exits.
    Ok(SpawnedPty {
        term_name,
        transport: LocalPty {
            master: pair.master,
            read_cancel,
            writer: Some(writer),
            queued: None,
            recorder: None,
            metrics: None,
        },
        reader,
        child,
        started,
    })
}

/// Establishes implemented capabilities and initial theme before any child startup output.
fn advertise_terminal(
    command: &mut CommandBuilder,
    requested: Option<&'static str>,
    appearance: Appearance,
) -> &'static str {
    // Preserve user settings and PATH, but discard the outer terminal's identity.
    // Both AppKit and the harness spawn through this one seam.
    identity::clean(command);
    let terminfo =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/terminfo/compiled");
    let term_name = if requested == Some("xterm-256color") {
        "xterm-256color"
    } else if terminfo.join("78/xterm-puzzle").is_file()
        || terminfo.join("x/xterm-puzzle").is_file()
    {
        // The child finds our entry without installing it in the user's
        // home database. ncurses also checks its normal fallback paths.
        command.env("TERMINFO", terminfo);
        "xterm-puzzle"
    } else {
        tracing::warn!(event = "desktop.terminfo_missing", "using xterm-256color");
        "xterm-256color"
    };
    command.env("TERM", term_name);
    command.env("COLORTERM", "truecolor");
    command.env("TERM_PROGRAM", "puzzle");
    command.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));
    command.env("COLORFGBG", appearance.colorfgbg());
    // Return the advertised identity so Session answers capability queries consistently.
    term_name
}
