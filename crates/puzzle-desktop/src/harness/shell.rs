//! Live PTY command and keyboard evidence without an OS window.
//!
//! The encoder-only `key` command submits through Session. The planned
//! platform-shaped `input` command will cross the desktop RawInput adapter.
//! The transport records writes before forwarding them to the PTY, because
//! a grid dump alone cannot expose a wrong or duplicated control sequence.
//!
//! Nvim is a useful protocol witness because it opts into the kitty keyboard
//! extension immediately after entering the alternate screen. A terminal
//! that encodes Ctrl+D using primary-screen state will send a legacy 0x04
//! even though the editor requested CSI u on its alternate screen.
//! The visual symptom can be misleading: the editor may scroll for the
//! correct CSI u press and then display a literal `^D` for a second C0 byte.
//! Capturing write boundaries therefore matters more than capturing only
//! the concatenated input stream. A second transport write remains visible
//! even when both writes occur before the next render frame.
//!
//! A recorded platform sequence will exercise the order of key, modifier
//! and IME callbacks after K2 captures it. Inventing that order here would
//! defeat the purpose of diagnosing the live-only bug.
//!
//! Nvim's output is captured before Session parses it. This lets the trace
//! report the screen selected when each negotiation sequence arrived, even
//! if a later full-screen redraw erases every visible sign of that sequence.
//! The Session itself remains the authority for input mode at key time:
//! trace bytes can arrive ahead of the bounded parser during a burst, so the
//! trace alone cannot prove what flags the encoder actually sampled.
//!
//! The child runs with a fixed 100×30 grid and the repository as its cwd.
//! That cwd lets the existing nvim regression open `README.md`; an empty
//! buffer has no half page to scroll and would make a bad encoder appear
//! harmless. General shell commands use the same PTY and working directory.
//!
//! The reader thread and bounded channel keep the scenario representative
//! of the live native transport without opening a winit window. The child
//! handle belongs only to this scenario and is killed at drop; it can never
//! terminate the user's separate live Puzzle window or editor session.
//! Raw output is capped at one MiB because a faulty child could write
//! indefinitely; the captured startup negotiation is far below that cap.
//! Input writes are separately retained without this cap so the regression
//! can say exactly how many PTY writes a single key generated.
use super::Scenario;
use crate::{
    keyboard::{Adapter, raw::RawInput, sequences},
    pty,
};
use portable_pty::{Child, CommandBuilder};
use puzzle_term::{
    Origin, Session, SessionConfig,
    platform::{PtySize, Services},
};
use std::{
    fs,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver},
    },
    time::{Duration, Instant},
};

/// Encoder-only key chords, separate from recorded platform input.
mod chord;
pub(super) use chord::key;
/// Exact PTY write comparison for one recorded host chord.
mod expect;
mod io;
/// Replays keyecho's two real PTY groups against the shared settle rule.
mod pacing;
pub(super) use pacing::{expect_paced_no_marker, expect_paced_repeat_no_marker};
/// Failed witnesses retain all captured bytes before a retry starts another child.
mod failure;
/// Mirrors a real PTY session into incremental GPU scenario assertions.
mod render;
mod trace;
/// Bounded quiet-period wait for arbitrary live commands.
mod wait;
pub(super) use expect::expect_pty_key;
use io::{NoFrame, RecordedPty, read_output};
pub(super) use render::render;
use trace::{escaped, mode_trace};
pub(super) use wait::wait_idle;

/// Scenario state also represents a documented skip when nvim is absent.
pub(super) enum ShellRun {
    /// Running child and its captured protocol evidence.
    Active(Box<ShellProbe>),
    /// A missing optional application does not fail unrelated acceptance.
    Skipped,
}

/// PTY, parser and recorded writes owned by one headless child.
///
/// Raw output and Session can advance at different rates when the parser's
/// eight-millisecond feed budget expires. The key test records both the
/// wire trace and the Session's active flags before input is encoded.
pub(super) struct ShellProbe {
    /// The session observes modes before encoding input.
    session: Session,
    /// Reader cannot block the scenario's command interpreter.
    output: Receiver<Vec<u8>>,
    /// Raw output preserves negotiation bytes for later annotation.
    raw: Vec<u8>,
    /// Each transport write is retained as a separate byte sequence.
    writes: Arc<Mutex<Vec<Vec<u8>>>>,
    /// Only this spawned child is terminated when the scenario finishes.
    child: Box<dyn Child + Send + Sync>,
    /// Number of writes before the most recent key press.
    ///
    /// Startup DA and kitty-query replies may already have been written.
    key_start: usize,
    /// First later reply write, excluded from the key sequence evidence.
    ///
    /// Nvim may query device attributes during the following redraw.
    key_end: usize,
    /// Kitty flags sampled immediately before the key was encoded.
    key_flags: u8,
    /// Raw output offset just before the most recent input chord.
    key_output_start: usize,
    /// Stateful host adapter shared by input-only replay and future chords.
    adapter: Adapter,
}

/// Starts the scenario's arbitrary shell command on a bounded PTY grid.
///
/// The optional nvim witness uses `--clean` in the scenario itself to avoid
/// user plugins; a missing executable is recorded as a skip, not a pass.
pub(super) fn open(state: &mut Scenario, line: &str) -> Result<(), String> {
    render::reset(state);
    let command = super::preview::quoted(line, "shell")?;
    if command.is_empty() {
        return Err("shell command is empty".into());
    }
    if command.contains("nvim --clean")
        && std::process::Command::new("nvim")
            .arg("--version")
            .output()
            .is_err()
    {
        let note = "SKIP nvim Ctrl+D regression: nvim is not installed\n".to_owned();
        fs::write(state.output.join("nvim-skip.txt"), &note).map_err(|error| error.to_string())?;
        state.shell = Some(ShellRun::Skipped);
        return Ok(());
    }
    // A prior skipped run must not hide a newly installed nvim executable.
    let _ = fs::remove_file(state.output.join("nvim-skip.txt"));
    let child_command = shell_command(command)?;
    // Thirty rows give Ctrl+D a visible half-page movement. Matching PTY
    // rows and Session rows keeps its cursor reports in the same space.
    let size = PtySize {
        columns: 100,
        rows: 30,
        pixel_width: 1000,
        pixel_height: 600,
    };
    // The real local PTY supplies tty detection; a pipe would make nvim
    // choose a different input and output protocol.
    let spawned = pty::spawn(child_command, size)?;
    // Sixteen chunks bound in-flight output while the scenario prepares
    // the parser, paralleling the native app's backpressure discipline.
    let (sender, output) = mpsc::sync_channel(16);
    std::thread::spawn(move || read_output(spawned.reader, sender));
    // Session owns the transport, so the shared log is the only readout
    // of exact writes after a key command has crossed that ownership seam.
    let writes = Arc::new(Mutex::new(Vec::new()));
    let transport = RecordedPty {
        inner: spawned.transport,
        writes: Arc::clone(&writes),
    };
    // The VT grid must agree with the OS PTY at startup; a mismatch can
    // change nvim's page height and invalidate the scroll assertion.
    let config = SessionConfig {
        term_name: spawned.term_name,
        columns: 100,
        rows: 30,
        ..SessionConfig::default()
    };
    let session = Session::new(
        config,
        Services::new(Box::new(transport), Box::new(NoFrame)),
    );
    let mut probe = ShellProbe {
        session,
        output,
        raw: Vec::new(),
        writes,
        child: spawned.child,
        key_start: 0,
        key_end: 0,
        key_flags: 0,
        key_output_start: 0,
        adapter: Adapter::default(),
    };
    if command.contains("nvim --clean") {
        probe.wait_for_kitty();
    }
    // One final short drain lets the editor finish its first file redraw;
    // otherwise an empty startup buffer could be mistaken for real text.
    probe.collect(Duration::from_millis(100));
    state.shell = Some(ShellRun::Active(Box::new(probe)));
    Ok(())
}

/// Starts an arbitrary command line with the repository as its cwd.
fn shell_command(line: &str) -> Result<CommandBuilder, String> {
    // POSIX sh parses the quoted scenario argument as a command line. The
    // harness deliberately accepts different programs and arguments here.
    let mut command = CommandBuilder::new("/bin/sh");
    command.arg("-c");
    // `<bin>` names this exact harness executable. A release acceptance
    // run must start its release keyecho child, not a stale debug binary.
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let quoted = format!(
        "'{}'",
        executable.display().to_string().replace('\'', "'\\''")
    );
    command.arg(line.replace("<bin>", &quoted));
    // The PTY library may choose a home directory when no cwd is set;
    // the regression must open this repository's real progress file.
    command.cwd(std::env::current_dir().map_err(|error| error.to_string())?);
    Ok(command)
}

/// Sends a recorded macOS callback sequence through the live RawInput seam.
pub(super) fn input(state: &mut Scenario, line: &str) -> Result<(), String> {
    let chord = line.strip_prefix("input ").ok_or("input needs a chord")?;
    let (events, source) = sequences::for_chord(chord)?;
    let Some(ShellRun::Active(probe)) = state.shell.as_mut() else {
        return Err("input needs an active shell".into());
    };
    // A fence around all four callbacks captures both press and release.
    // The mode is read before any callback can change Session state.
    let writes = probe.writes.lock().map_err(|error| error.to_string())?;
    probe.key_start = writes.len();
    drop(writes);
    let terminal = probe.session.terminal();
    probe.key_flags = terminal.kitty_keys.flags(terminal.active);
    probe.key_output_start = probe.raw.len();
    tracing::debug!(chord, ?source, "headless macOS input sequence");
    for event in events {
        probe.apply_raw(event);
    }
    probe.key_end = probe
        .writes
        .lock()
        .map_err(|error| error.to_string())?
        .len();
    Ok(())
}

/// Feeds one recorded platform callback through the exact live adapter.
pub(super) fn raw_input(state: &mut Scenario, input: RawInput) -> Result<(), String> {
    let Some(ShellRun::Active(probe)) = state.shell.as_mut() else {
        return Err("input-only replay needs an active shell".into());
    };
    probe.apply_raw(input);
    Ok(())
}

impl ShellProbe {
    /// Applies one host callback and pumps a resulting command immediately.
    fn apply_raw(&mut self, input: RawInput) {
        if let Some(command) = self.adapter.apply(input) {
            // Session samples the currently parsed kitty and DECCKM modes;
            // replaying raw PTY bytes directly would skip that policy layer.
            self.session.submit(command, Origin::User);
            self.session.pump(Instant::now());
        }
    }
}

/// Lets nvim redraw twice after the key, as the scenario requests.
///
/// Each turn drains output before asking Session for a frame. The frame
/// call also resumes any parser suffix that exceeded the previous feed's
/// time budget, just as the fair native event loop does under output load.
pub(super) fn frames(state: &mut Scenario, count: Option<&str>) -> Result<(), String> {
    let count: usize = count
        .ok_or("frames needs a count")?
        .parse()
        .map_err(|_| "invalid frames count")?;
    if count > 32 {
        return Err("frames count exceeds 32".into());
    }
    let Some(ShellRun::Active(probe)) = state.shell.as_mut() else {
        return Ok(());
    };
    // Two turns are enough for a key response and a following screen paint;
    // the scenario intentionally does not wait on visual presentation.
    for _ in 0..count {
        probe.collect(Duration::from_millis(100));
        let mut drawing = super::drawing::Drawing::default();
        drawing.publish(&mut probe.session, Instant::now())?;
    }
    Ok(())
}

/// Writes the VT text, negotiated mode trace and exact key writes.
///
/// A reviewer can compare the mode trace and the key write without relying
/// on a screenshot's ambiguous glyph rendering. The grid text supplies an
/// independent behavioral check: after Ctrl+D, later progress lines should
/// occupy the top of the nvim buffer and no literal `^D` should appear.
pub(super) fn dump(state: &mut Scenario, label: Option<&str>) -> Result<(), String> {
    let label = label.ok_or("dump needs a label")?;
    let Some(shell) = state.shell.as_mut() else {
        return Err("shell was not started".into());
    };
    let ShellRun::Active(probe) = shell else {
        return Ok(());
    };
    probe.collect(Duration::from_millis(20));
    // Inspect parsed VT cells, rather than searching nvim's raw output:
    // old screen contents remain in the raw stream after a redraw.
    let view = probe.session.buffer();
    let lines = view.line_ids().ok_or("nvim has no visible lines")?;
    let grid = view.text(lines, Default::default());
    fs::write(state.output.join(format!("{label}.grid.txt")), grid)
        .map_err(|error| error.to_string())?;
    // The trace replays CSI sequences to annotate screen transitions.
    fs::write(state.output.join("modes.txt"), mode_trace(&probe.raw))
        .map_err(|error| error.to_string())?;
    // Preserve individual transport-write rows; concatenation would hide
    // whether an IME commit generated a second write after the key press.
    let writes = probe.writes.lock().map_err(|error| error.to_string())?;
    let mut keys = format!("flags={}\n", probe.key_flags);
    for bytes in &writes[probe.key_start..probe.key_end] {
        keys.push_str(&format!("{}\n", escaped(bytes)));
    }
    fs::write(state.output.join("key-bytes.txt"), keys).map_err(|error| error.to_string())
}

impl ShellProbe {
    /// Waits for nvim's kitty push before an encoder regression sends a key.
    fn wait_for_kitty(&mut self) {
        // A general shell command may never negotiate this mode; callers
        // opt in only for nvim, where an early key would validly be legacy.
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            self.collect(Duration::from_millis(40));
            let terminal = self.session.terminal();
            if terminal.kitty_keys.flags(terminal.active) != 0 {
                break;
            }
        }
    }
    /// Drains a bounded read interval while preserving raw PTY output.
    ///
    /// One timed receive waits for startup or redraw. The following
    /// nonblocking drain handles a burst without adding a fixed sleep per
    /// chunk, so short nvim redraws do not take seconds in acceptance.
    fn collect(&mut self, delay: Duration) {
        if let Ok(bytes) = self.output.recv_timeout(delay) {
            self.ingest(&bytes);
        }
        while let Ok(bytes) = self.output.try_recv() {
            self.ingest(&bytes);
        }
    }
    /// Feeds identical bytes to the real parser and the evidence buffer.
    ///
    /// The raw cap bounds diagnostic memory only. Every byte still reaches
    /// Session, because truncating protocol input could change kitty flags.
    fn ingest(&mut self, bytes: &[u8]) {
        if self.raw.len() + bytes.len() <= 1_048_576 {
            self.raw.extend_from_slice(bytes);
        }
        self.session.feed(bytes, Instant::now());
    }
}

impl Drop for ShellProbe {
    /// Stops only the child this scenario spawned.
    ///
    /// The PTY reader exits when the sole master writer and child close;
    /// no global process-name lookup or user window is involved.
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}
