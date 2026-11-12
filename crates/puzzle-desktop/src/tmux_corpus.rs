//! Records tmux's actual VT output for native parser benchmarks.
//!
//! A detached tmux session starts without an interactive shell prompt, and
//! its status bar is disabled before the recording client attaches. That
//! keeps hostnames, usernames and paths out of the fixture. The client PTY
//! is exactly 160×48 with TERM=xterm-256color; `seq 1 100000` supplies the
//! same neutral data in one pane or two split panes. Only the PTY's output
//! bytes are saved; control commands use a separate tmux client process.
//!
//! The D-31 capture uses a smaller 40×6 PTY because a 20-column pane puts
//! the VS16 symbol near the divider. tmux 3.x prints a provisional base,
//! backs up, and repaints the completed two-column grapheme. Replaying its
//! actual escape stream checks terminal state changes that a synthetic
//! `feed "✌️"` cannot reproduce, especially a stale base after backspace.
//!
//! Pane commands are literal, neutral strings and never start the login
//! shell. The capture terminates before server shutdown resets the client's
//! alternate screen; that teardown is unrelated to the live split and
//! would conceal the row under test. A short minimum byte count catches a
//! missing pane repaint without treating the generated data as a golden.
//!
//! tmux is deliberately not configured with the user's `~/.tmux.conf`:
//! aliases, plugins and status formats would change the parser workload
//! and could disclose private data. `-L` also isolates the capture from
//! any running personal tmux server. A unique socket name prevents two
//! concurrent probes from stealing one another's panes.
//!
//! The desktop PTY boundary creates the real terminal size. Setting only
//! `COLUMNS` and `LINES` would not make tmux produce the same control
//! stream, since tmux queries the PTY's winsize. The fixture contains
//! tmux's output to a client, not seq's raw stdout to a pane.
//!
//! This command writes into `target/` first. A person or agent reviews
//! bytes for names and paths before copying them into checked-in assets.
use crate::pty;
use portable_pty::CommandBuilder;
use puzzle_term::platform::PtySize;
use std::{io::Read, process::Command, thread, time::Duration};

/// Captures one of the requested single, horizontal or vertical layouts.
pub(crate) fn run(kind: Option<&str>) -> Result<(), String> {
    let kind = kind.ok_or("tmux-corpus needs single, hsplit, vsplit or grapheme")?;
    // Reject unknown names before creating a server: accidental input
    // cannot leave an isolated tmux process running without a matching
    // capture path or cleanup step.
    if !matches!(kind, "single" | "hsplit" | "vsplit" | "grapheme") {
        return Err(format!("unknown tmux corpus layout: {kind}"));
    }
    // A unique socket prevents the probe from changing the user's own tmux
    // server, even when that server is running during this command.
    let socket = format!("puzzle-corpus-{}", std::process::id());
    let session = "puzzle-corpus";
    // Detached creation permits status suppression before a client sees
    // even the first frame. A sleeping pane emits no shell prompt.
    let (columns, rows) = if kind == "grapheme" {
        // A 40-column client splits into two 19-column panes plus divider;
        // that makes the divider's expected physical column deterministic.
        ("40", "6")
    } else {
        ("160", "48")
    };
    control(
        &socket,
        &[
            "new-session",
            "-d",
            "-x",
            columns,
            "-y",
            rows,
            "-s",
            session,
            "sleep 30",
        ],
    )?;
    // Capture errors still need to release the socket. The normal path
    // kills the server only after the recording reader is attached.
    let result = record_layout(&socket, session, kind);
    // The successful path closes the server before joining its reader;
    // only an early error needs this separate cleanup attempt.
    if result.is_err() {
        let _ = control(&socket, &["kill-server"]);
    }
    result
}

/// Runs a tmux command against the isolated server with default config.
fn control(socket: &str, args: &[&str]) -> Result<(), String> {
    // The fixed config and TERM make bytes comparable across machines;
    // only the separate isolated server receives these commands.
    let status = Command::new("tmux")
        .args(["-f", "/dev/null", "-L", socket])
        .args(args)
        .env("TERM", "xterm-256color")
        .status()
        .map_err(|error| format!("start tmux control command: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("tmux {args:?} exited with {status}"))
    }
}

/// Creates an attached client with the same cell size as the tmux server.
fn client(socket: &str, session: &str, kind: &str) -> Result<pty::SpawnedPty, String> {
    let mut command = CommandBuilder::new("tmux");
    // The attaching client gets the same empty config as server controls;
    // a local default config must never alter keyboard or feature modes.
    for arg in [
        "-f",
        "/dev/null",
        "-L",
        socket,
        "attach-session",
        "-t",
        session,
    ] {
        command.arg(arg);
    }
    // The client PTY's real size, not environment COLUMNS/LINES, decides
    // how tmux emits cursor addressing and the split-pane divider.
    let (columns, rows) = if kind == "grapheme" {
        (40, 6)
    } else {
        (160, 48)
    };
    let size = PtySize {
        // Pixel dimensions accompany the cell dimensions because a PTY
        // can report both. tmux's layout uses the cell counts above.
        columns,
        rows,
        pixel_width: columns * 10,
        pixel_height: rows * 20,
    };
    pty::spawn(command, size)
}

/// Replaces sleeping panes with the same finite, privacy-neutral stream.
fn start_seq(socket: &str, kind: &str) -> Result<(), String> {
    // Respawn preserves pane geometry and begins output only after the
    // client is reading. The extra sleep keeps the pane alive at EOF.
    control(
        socket,
        &[
            "respawn-pane",
            "-k",
            "-t",
            "puzzle-corpus:0.0",
            "seq 1 100000; sleep 2",
        ],
    )?;
    if kind != "single" {
        // Both panes receive identical neutral text; differences in bytes
        // then come from tmux's split layout and redraw decisions.
        control(
            socket,
            &[
                "respawn-pane",
                "-k",
                "-t",
                "puzzle-corpus:0.1",
                "seq 1 100000; sleep 2",
            ],
        )?;
    }
    Ok(())
}

/// Attaches a recording PTY after the no-status layout is configured.
fn record_layout(socket: &str, session: &str, kind: &str) -> Result<(), String> {
    // This option must be set before attaching: tmux's default status
    // format can contain the host and current working directory.
    control(socket, &["set-option", "-g", "status", "off"])?;
    if kind == "grapheme" {
        return record_grapheme(socket, session);
    }
    if kind != "single" {
        let direction = if kind == "hsplit" { "-h" } else { "-v" };
        // The second pane starts asleep so it cannot race with initial
        // client paint or expose a user-specific login shell prompt.
        control(
            socket,
            &[
                "split-window",
                direction,
                "-t",
                "puzzle-corpus:0.0",
                "sleep 30",
            ],
        )?;
    }
    let spawned = client(socket, session, kind)?;
    let mut reader = spawned.reader;
    let capture = thread::spawn(move || {
        let mut bytes = Vec::new();
        // Closing the tmux server ends the client PTY. Unix PTY masters
        // may report EIO instead of EOF after the slave closes.
        let _ = reader.read_to_end(&mut bytes);
        bytes
    });
    // Let the client finish its initial blank-screen paint before starting
    // seq, so the capture includes its actual scrolling redraws.
    thread::sleep(Duration::from_millis(100));
    start_seq(socket, kind)?;
    thread::sleep(Duration::from_secs(2));
    // Killing the isolated server closes the slave; only then can the
    // blocking reader finish and return its complete byte stream.
    control(socket, &["kill-server"])?;
    let bytes = capture.join().map_err(|_| "tmux reader panicked")?;
    if bytes.len() < 64 * 1024 {
        return Err(format!(
            "tmux {kind} capture has only {} bytes",
            bytes.len()
        ));
    }
    let path = format!("target/corpus/tmux-{kind}.vt");
    std::fs::create_dir_all("target/corpus").map_err(|error| error.to_string())?;
    std::fs::write(&path, &bytes).map_err(|error| error.to_string())?;
    // The bytes are inspected for private data before being copied into
    // assets; generating target evidence does not silently bless fixtures.
    tracing::info!(kind, bytes = bytes.len(), %path, "tmux corpus captured");
    Ok(())
}

/// Captures tmux's VS16 backspace sequence and split divider at 40×6.
fn record_grapheme(socket: &str, session: &str) -> Result<(), String> {
    // Attach before pane repaint so the client VT stream includes tmux's
    // cursor-home, erase and redraw sequences in their original order.
    let spawned = client(socket, session, "grapheme")?;
    let mut reader = spawned.reader;
    let capture = thread::spawn(move || {
        let mut bytes = Vec::new();
        // The server close later ends the PTY. A Unix master may report
        // EIO after the final bytes, which is equivalent to EOF here.
        let _ = reader.read_to_end(&mut bytes);
        bytes
    });
    // Both panes start from a neutral literal, with no shell prompt or path.
    let pane = "printf 'vars ✌️\\n$ '; sleep 60";
    // Allow the first blank frame to arrive before the two pane commands;
    // otherwise the fixture could begin mid-redraw and depend on timing.
    thread::sleep(Duration::from_millis(100));
    control(
        socket,
        &["respawn-pane", "-k", "-t", "puzzle-corpus:0.0", pane],
    )?;
    control(
        socket,
        &["split-window", "-h", "-t", "puzzle-corpus:0.0", pane],
    )?;
    thread::sleep(Duration::from_millis(200));
    // Both panes must have painted before shutdown, or the regression
    // would test only the left side while still containing valid VT bytes.
    control(socket, &["kill-server"])?;
    let mut bytes = capture.join().map_err(|_| "tmux reader panicked")?;
    // Server shutdown clears the client's alternate screen. The fixture
    // represents the live split just before that unrelated teardown.
    let close = b"\x1b[?1l\x1b>";
    // This sequence begins tmux client's teardown. Truncate at its first
    // occurrence to retain the populated alternate screen for assertions.
    if let Some(end) = bytes.windows(close.len()).position(|part| part == close) {
        bytes.truncate(end);
    }
    if bytes.len() < 100 {
        // A tiny stream means attach or split failed; never bless it as a
        // fixture merely because the tmux control commands exited zero.
        return Err("tmux grapheme capture is incomplete".into());
    }
    std::fs::create_dir_all("target/corpus").map_err(|error| error.to_string())?;
    // This command emits reviewable target evidence only. The checked-in
    // fixture is copied separately after scanning for private strings.
    std::fs::write("target/corpus/tmux-grapheme.vt", &bytes).map_err(|error| error.to_string())?;
    tracing::info!(bytes = bytes.len(), "tmux grapheme fixture captured");
    Ok(())
}
