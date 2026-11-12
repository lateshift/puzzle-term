//! Real tmux output with Puzzle and legacy terminfo (§6.12, D-32).
//!
//! tmux decides whether to send mode 2026 from its outer terminal's
//! `Sync` capability. A single fixed cmatrix pane changes continuously;
//! an adjacent prompt leaves a known cursor position. The reader retains
//! the macOS PTY's 1 KiB read boundaries, because splitting an update
//! across reads is the condition that made the prompt cursor flicker.
//!
//! There are two separate processes in a tmux test. The detached server
//! prepares panes; an attached client talks to Puzzle through a PTY. Only
//! the attached client's TERM is the *outer* terminal identity. Setting
//! TERM on the server control commands would test a different interface.
//! The control client therefore uses a stable baseline identity, while
//! `spawn_with_term` chooses the identity of the attached client.
//!
//! The server uses `/dev/null` instead of the user's tmux configuration.
//! A user configuration might force `terminal-features :sync`, giving a
//! false positive even with xterm-256color. It can also add a status bar
//! containing local paths or hostnames. A private socket avoids attaching
//! to a user's live server, and the panes use only neutral commands.
//!
//! The prompt pane stays selected. cmatrix occupies the other pane, so
//! its redraws should leave the selected prompt cursor in place after
//! each complete tmux update. In the legacy case tmux does not mark the
//! update as atomic; a 1 KiB read can end with the cursor temporarily in
//! cmatrix. That is the reported flicker. With Sync, a frame requested
//! while mode 2026 is set is deferred until its matching reset.
//!
//! This replay deliberately models the older immediate-per-read frame
//! schedule. It tests the *protection* provided by tmux's Sync wrapping,
//! independently of D-33's late-snapshot scheduler. Thus a later frame
//! pacing improvement cannot accidentally conceal missing terminfo.
//! The probe is observational, not a visual golden: cmatrix's random
//! characters change on each run, while the atomicity condition does not.
//!
//! Counts start only after the `$ ` prompt and cursor have both appeared.
//! tmux's initial clear can expose the text before placing its cursor;
//! counting that first incomplete screen would report flicker unrelated
//! to a busy neighboring pane. The capture stops before killing tmux;
//! exit-time reset sequences are also outside the live-pane question.
//! The fixed 80×24 PTY gives tmux enough room for a horizontal split and
//! keeps the test cheap during `cargo xtask accept M6`.

use crate::pty;
use portable_pty::CommandBuilder;
use puzzle_term::{platform::PtySize, vt::Terminal};
use std::{
    io::Read,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

/// Counts actual synchronized updates and visible prompt-cursor defects.
#[derive(Default)]
struct ResultCounts {
    /// Number of 2026 begin sequences emitted by tmux.
    sync: usize,
    /// Frame candidates where the active prompt cursor is elsewhere.
    missing_cursor: usize,
    /// Candidates after tmux has painted the prompt at least once.
    frames: usize,
}

/// Runs both TERM variants without touching a user's tmux server.
pub(crate) fn run(check: Option<&str>) -> Result<(), String> {
    if check == Some("--fixture") {
        // Fixed read boundaries keep this gate independent of whichever
        // tmux and cmatrix versions are installed on the current machine.
        return check_fixtures();
    }
    if !program_available("tmux") || !program_available("cmatrix") {
        println!("SKIP terminfo live probe: tmux or cmatrix unavailable");
        return Ok(());
    }
    // The two TERM runs use identical pane commands and PTY geometry;
    // only the client's advertised capabilities may change negotiation.
    let puzzle = capture("xterm-puzzle")?;
    let legacy = capture("xterm-256color")?;
    // Print raw counts before evaluating either criterion. If a new tmux
    // version changes redraw behavior, the evidence remains visible.
    println!(
        "puzzle sync={} frames={} cursor_missing={}; legacy sync={} frames={} cursor_missing={}",
        puzzle.sync,
        puzzle.frames,
        puzzle.missing_cursor,
        legacy.sync,
        legacy.frames,
        legacy.missing_cursor
    );
    if check != Some("--cursor") && (puzzle.sync == 0 || legacy.sync != 0) {
        // An extension claim is meaningful only if the legacy control
        // cannot acquire the same wrapping from another source.
        return Err("tmux did not distinguish Puzzle's Sync capability".into());
    }
    if check != Some("--sync") && (puzzle.frames == 0 || legacy.frames == 0) {
        // A zero-frame comparison would pass the cursor assertion vacuously.
        return Err("tmux did not paint the prompt during the capture".into());
    }
    if check != Some("--sync") && (puzzle.missing_cursor != 0 || legacy.missing_cursor == 0) {
        // The negative control must exhibit the defect, or this capture
        // cannot establish that Sync removed it.
        return Err("tmux cursor-flicker comparison did not separate entries".into());
    }
    Ok(())
}

/// Recognizes installed optional tools without running a user's program.
fn program_available(name: &str) -> bool {
    Command::new(name)
        .arg(if name == "tmux" { "-V" } else { "-h" })
        .output()
        .is_ok()
}

/// Replays committed reads while preserving their original chunk boundaries.
fn check_fixtures() -> Result<(), String> {
    let puzzle = replay(&load_fixture("xterm-puzzle")?);
    let legacy = replay(&load_fixture("xterm-256color")?);
    println!(
        "fixture puzzle sync={} frames={} cursor_missing={}; legacy sync={} frames={} cursor_missing={}",
        puzzle.sync,
        puzzle.frames,
        puzzle.missing_cursor,
        legacy.sync,
        legacy.frames,
        legacy.missing_cursor
    );
    if puzzle.sync == 0 || legacy.sync != 0 || puzzle.frames == 0 || legacy.frames == 0 {
        return Err("fixture does not cover both terminfo negotiations".into());
    }
    if puzzle.missing_cursor != 0 || legacy.missing_cursor == 0 {
        return Err("fixture cursor position did not distinguish Sync".into());
    }
    Ok(())
}

/// Decodes u16 length prefixes, rejecting corruption instead of merging reads.
fn load_fixture(name: &str) -> Result<Vec<Vec<u8>>, String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../assets/fixtures/terminfo/{name}.reads"));
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let mut chunks = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        // A corrupted trailing length or oversized chunk is a fixture
        // failure, never a reason to silently merge adjacent PTY reads.
        let pair = bytes
            .get(offset..offset + 2)
            .ok_or("truncated read length")?;
        let length = usize::from(u16::from_le_bytes([pair[0], pair[1]]));
        if length == 0 || length > 1024 {
            return Err(format!("invalid {name} read length {length}"));
        }
        offset += 2;
        let chunk = bytes
            .get(offset..offset + length)
            .ok_or("truncated read bytes")?;
        chunks.push(chunk.to_vec());
        offset += length;
    }
    Ok(chunks)
}

/// Captures one isolated tmux client while cmatrix repaints the other pane.
fn capture(term_name: &'static str) -> Result<ResultCounts, String> {
    // Different socket names let both captures run in succession without
    // inheriting options or a previous client's terminal feature cache.
    let socket = format!("puzzle-terminfo-{}-{term_name}", std::process::id());
    let session = "puzzle-terminfo";
    control(
        &socket,
        &[
            "new-session",
            "-d",
            "-x",
            "80",
            "-y",
            "24",
            "-s",
            session,
            "printf '$ '; sleep 30",
        ],
    )?;
    // Even an early attach or cmatrix error must tear down the isolated
    // server, rather than leaving its sleeping child behind.
    let result = capture_started(&socket, session, term_name);
    if result.is_err() {
        let _ = control(&socket, &["kill-server"]);
    }
    result
}

/// Attaches the client only after the pane layout and status are fixed.
fn capture_started(
    socket: &str,
    session: &str,
    term_name: &'static str,
) -> Result<ResultCounts, String> {
    // Status is disabled before attachment: it would otherwise repaint
    // independently and change which cell owns the cursor.
    control(socket, &["set-option", "-g", "status", "off"])?;
    // tmux's `split-window -h` creates side-by-side panes. cmatrix is
    // deliberately in the unselected pane to stress cursor restoration.
    control(
        socket,
        &[
            "split-window",
            "-h",
            "-t",
            "puzzle-terminfo:0.0",
            "cmatrix -b",
        ],
    )?;
    control(socket, &["select-pane", "-t", "puzzle-terminfo:0.0"])?;
    // The attached client, rather than server controls, sees TERMINFO.
    let mut command = CommandBuilder::new("tmux");
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
    let size = PtySize {
        columns: 80,
        rows: 24,
        pixel_width: 800,
        pixel_height: 480,
    };
    let spawned = pty::spawn_with_term(command, size, Some(term_name))?;
    // The reader collects bytes; replay alone owns Session. Its read bound reproduces
    // portable-pty's macOS behavior at the boundary where frames arose.
    let stop = Arc::new(AtomicBool::new(false));
    let reader_stop = Arc::clone(&stop);
    let reader = thread::spawn(move || read_chunks(spawned.reader, &reader_stop));
    // Two seconds cover several cmatrix redraws and keep the acceptance
    // probe short enough for a routine check.
    thread::sleep(Duration::from_millis(2500));
    // Exclude tmux's shutdown repaint; the review concerns frames while
    // a live pane is updating, not the client's exit sequence.
    stop.store(true, Ordering::Release);
    // Kill releases the blocking PTY read. The atomic flag causes the
    // reader to discard bytes returned by this teardown.
    control(socket, &["kill-server"])?;
    let chunks = reader.join().map_err(|_| "tmux reader panicked")?;
    save_chunks(term_name, &chunks)?;
    Ok(replay(&chunks))
}

/// Saves read lengths before bytes so the same frame boundaries can replay.
fn save_chunks(term_name: &str, chunks: &[Vec<u8>]) -> Result<(), String> {
    // Evidence stays in ignored target/, not a committed fixture; although
    // these fixed commands are neutral, capture timing is nondeterministic.
    let directory = std::path::Path::new("target/terminfo");
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    for chunk in chunks {
        // The reader's fixed buffer bounds every length at 1,024 bytes.
        let length = u16::try_from(chunk.len()).map_err(|error| error.to_string())?;
        bytes.extend_from_slice(&length.to_le_bytes());
        // The length prefix makes a later replay distinguish two short
        // reads from one longer read with the same combined byte stream.
        bytes.extend_from_slice(chunk);
    }
    std::fs::write(directory.join(format!("{term_name}.reads")), bytes)
        .map_err(|error| error.to_string())
}

/// Uses a private socket and no user configuration or shell prompt.
fn control(socket: &str, args: &[&str]) -> Result<(), String> {
    // Use process arguments, never a shell: tmux targets and flags are
    // fixed strings, and the socket name only contains our pid and TERM.
    let status = Command::new("tmux")
        .args(["-f", "/dev/null", "-L", socket])
        .args(args)
        .env("TERM", "xterm-256color")
        .status()
        .map_err(|error| format!("start tmux: {error}"))?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| format!("tmux {args:?}: {status}"))
}

/// Stores exactly one vector per native read, including short reads.
fn read_chunks(mut reader: Box<dyn Read + Send>, stop: &AtomicBool) -> Vec<Vec<u8>> {
    // The array is the actual read buffer. Splitting a later large read
    // into synthetic pieces would not reproduce the OS scheduling point.
    let mut chunks = Vec::new();
    let mut buffer = [0_u8; 1024];
    while let Ok(size) = reader.read(&mut buffer) {
        if size == 0 {
            break;
        }
        if stop.load(Ordering::Acquire) {
            // The wake-up read may contain tmux's exit redraw. It is not
            // part of the live workload and must not enter cursor counts.
            break;
        }
        chunks.push(buffer[..size].to_vec());
    }
    chunks
}

/// Models a frame after each read unless mode 2026 defers presentation.
fn replay(chunks: &[Vec<u8>]) -> ResultCounts {
    // The pure VT model is sufficient for cursor and mode checks; no GPU
    // font or pixel state can influence tmux's negotiation bytes.
    let mut terminal = Terminal::new(80, 24, 0, 0);
    let mut result = ResultCounts::default();
    let bytes: Vec<u8> = chunks.iter().flatten().copied().collect();
    // Count begin markers on the joined byte stream, since a marker can
    // straddle a 1 KiB read even though each replay step remains separate.
    result.sync = bytes
        .windows(8)
        .filter(|part| *part == b"\x1b[?2026h")
        .count();
    let mut prompt_ready = false;
    for chunk in chunks {
        // `advance` retains incomplete CSI state across reads, matching
        // the live parser instead of assuming each read ends on a token.
        terminal.advance(chunk);
        if terminal.modes.get(true, 2026) == Some(true) {
            // The renderer withholds these intermediate states; examining
            // their cursor positions would invent frames the user never saw.
            continue;
        }
        // Prompt is the only `$` in the neutral fixed panes. Ignore the
        // initial clear before tmux has first placed it on the grid.
        let prompt = terminal
            .screen()
            .row(0)
            .and_then(|row| row.cells.iter().position(|cell| cell.ch == u32::from(b'$')));
        if let Some(column) = prompt {
            // The literal `$ ` occupies two cells. Until the cursor lands
            // immediately after it, startup layout is still incomplete.
            if terminal.cursor.row == 0 && terminal.cursor.column == column + 2 {
                prompt_ready = true;
            }
            if !prompt_ready {
                continue;
            }
            result.frames += 1;
            if terminal.cursor.row != 0
                || terminal.cursor.column != column + 2
                || terminal.modes.get(true, 25) != Some(true)
            {
                // A cursor elsewhere, or hidden by DECTCEM, is exactly
                // the transient state reported during the cmatrix redraw.
                result.missing_cursor += 1;
            }
        }
    }
    result
}
