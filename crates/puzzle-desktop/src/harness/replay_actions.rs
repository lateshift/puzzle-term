//! Deterministic replay of native JSON Lines event and parse boundaries.
//!
//! A PTY read may precede its parse slice, and the parser may advance while
//! the GPU presents an older frozen snapshot. The recording's `parse` and
//! `snapshot` entries retain those boundaries. Replaying every read at once
//! would erase the timing distinction at the heart of the nvim report.
//!
//! The adapter sees recorded RawInput, while a recording transport collects
//! actual encoded writes and VT replies. The final comparison names the
//! original entry for a changed byte sequence instead of treating any
//! terminal output as proof that the key was encoded correctly.
//! Pointer/wheel commands instead arrive after coalescing (§5.7, P12), without
//! a RawInput callback; their strict public wire codecs restore the same command.
//!
//! A recording contains both `pty-read` and `parse` because the drain may
//! collect bytes before the VT worker takes its next lease. Replayer queues
//! each read, then offers exactly the recorded prefix at each parse entry.
//! A parse entry includes `offered` bytes as well as the cumulative parsed
//! count: Session may retain a pending suffix after its time budget, so
//! offered and parsed are not necessarily equal. The replay checks both.
//!
//! A `snapshot` records a sealed Session publication before GPU work. A later
//! `frame` entry means the swapchain accepted that snapshot, even if the VT
//! worker parsed more bytes in between. The replayer therefore stores grid
//! text by frame number at snapshot time and materializes it at presentation
//! time. This is the minimal evidence needed to distinguish a stale draw
//! from a VT grid that itself failed to update. K3 adds pixel readback.
//!
//! PTY replies and input writes share one transport in the real Session.
//! Comparing their complete ordered write list catches an extra IME C0 send
//! as well as a wrong kitty encoding. It also avoids attributing a device
//! status reply to the user's preceding key merely because it arrived in
//! the same millisecond. The recorded entry index is retained for errors.
//!
//! `--input-only` deliberately makes a fresh child observe recorded host
//! callbacks. It is a different experiment from the deterministic VT
//! replay: the child may produce different output, but the adapter mapping
//! should stay stable. Both modes reject malformed JSON or hex fields with
//! a report before a byte reaches Session.
//!
//! Drawn replay additionally restores resize, DPR and focus callbacks.
//! Those do not change the PTY byte stream, but they change the cursor's
//! physical shape and the surface extent used by a frozen GPU frame.
//! A cold comparison must observe the same callback order and blink clock;
//! otherwise a hollow cursor after focus loss can appear to be stale ink.
//! The original recording remains private because its grid dumps may hold
//! the user's file, and no replay frame is staged into source control.

use crate::{keyboard::Adapter, recording};
use puzzle_term::{
    Origin, Session, SessionConfig,
    platform::{PtySize, Services, Transport, Waker},
};
use serde_json::Value as Json;
use std::{
    collections::{HashMap, VecDeque},
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// GPU readback for a private frame chronology, kept out of K2's CPU tests.
mod drawn;
use drawn::DrawProbe;
/// Command-line dispatch and recording-file traversal for this replay.
mod command;
#[cfg(test)]
use command::replay_file;
pub(super) use command::run;

/// The replay transport preserves each Session write boundary independently.
struct Writes {
    /// Shared list remains visible after Session takes transport ownership.
    actual: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl Transport for Writes {
    /// Appends exactly one encoder or VT reply call to the comparison list.
    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        // Keep separate write calls. Concatenating a correct key with an
        // accidental second IME write could hide the double-send defect.
        self.actual
            .lock()
            .map_err(|error| error.to_string())?
            .push(bytes.to_vec());
        Ok(())
    }
    /// The replay grid is fixed by the recording header for K2.
    fn resize(&mut self, _size: PtySize) -> Result<(), String> {
        Ok(())
    }
    /// The fake transport owns no OS handle to close.
    fn close(&mut self) -> Result<(), String> {
        Ok(())
    }
}

/// A headless replay advances frames at explicit recording entries.
struct NoWake;

impl Waker for NoWake {
    /// Frame boundaries come from the file, not a running event loop.
    fn request_frame(&mut self) {}
    /// Timer deadlines are not allowed to invent extra replay frames.
    fn wake_at(&mut self, _instant: Instant) {}
}

/// Accumulates reads until the recorded parser actually consumed them.
struct Replay {
    /// Current VT, input mode flags and typed command bus.
    session: Session,
    /// Same stateful adapter used by the live window.
    adapter: Adapter,
    /// OS bytes received but not yet parsed at this entry.
    pending: VecDeque<u8>,
    /// Number of bytes the replay has passed through VT.
    parsed: u64,
    /// Actual writes emitted by replies and input commands.
    writes: Arc<Mutex<Vec<Vec<u8>>>>,
    /// Expected writes retain their original JSON Lines indices.
    expected: Vec<(usize, Vec<u8>)>,
    /// Frozen grids are keyed by the frame that later presented them.
    snapshots: HashMap<u64, (u64, String)>,
    /// Last adapter result awaits the following command entry.
    command: Option<Json>,
    /// Optional GPU diagnostics keep every private output under target/.
    drawn: Option<DrawProbe>,
    /// Recorded millisecond offsets are relative to this monotonic origin.
    started: Instant,
}

impl Replay {
    /// Creates a neutral Session at the header's recorded grid size.
    fn new(header: &Json, with_drawn: bool) -> Result<Self, String> {
        // Reject malformed or extreme dimensions before allocating VT rows.
        // Recordings are external files, not trusted internal fixtures.
        let grid = header
            .get("grid")
            .and_then(Json::as_array)
            .ok_or("header lacks grid")?;
        let columns = grid
            .first()
            .and_then(Json::as_u64)
            .ok_or("header lacks columns")?;
        let rows = grid
            .get(1)
            .and_then(Json::as_u64)
            .ok_or("header lacks rows")?;
        let (columns, rows) = (
            usize::try_from(columns).map_err(|_| "columns overflow")?,
            usize::try_from(rows).map_err(|_| "rows overflow")?,
        );
        if columns == 0 || rows == 0 || columns > 1000 || rows > 1000 {
            return Err("recorded grid exceeds replay bounds".into());
        }
        let writes = Arc::new(Mutex::new(Vec::new()));
        // The transport is owned by Session, while acceptance compares its
        // side effects after every recorded event has been replayed.
        let transport = Writes {
            actual: Arc::clone(&writes),
        };
        let mut session = Session::new(
            SessionConfig {
                columns,
                rows,
                ..SessionConfig::default()
            },
            Services::new(Box::new(transport), Box::new(NoWake)),
        );
        let drawn = if with_drawn {
            let probe = DrawProbe::new(header)?;
            probe.install(&mut session);
            Some(probe)
        } else {
            None
        };
        Ok(Self {
            session,
            adapter: Adapter::default(),
            pending: VecDeque::new(),
            parsed: 0,
            writes,
            expected: Vec::new(),
            snapshots: HashMap::new(),
            command: None,
            drawn,
            started: Instant::now(),
        })
    }

    /// Feeds exactly the prefix the original parser consumed at this entry.
    fn parse_to(&mut self, total: u64, offered: usize, now: Instant) -> Result<(), String> {
        // Cumulative offsets must never move backwards. If they do, a
        // partial recording cannot be treated as a valid timeline.
        if total < self.parsed {
            return Err("parse offset moved backward".into());
        }
        let wanted = usize::try_from(total - self.parsed).map_err(|_| "parse span overflow")?;
        if offered > self.pending.len() {
            return Err("parse offset exceeds recorded PTY reads".into());
        }
        let bytes: Vec<u8> = self.pending.drain(..offered).collect();
        // `offered` is the actual VT call input. Its parsed result may
        // include an older pending suffix or leave a new one behind.
        let outcome = self.session.feed(&bytes, now);
        if outcome.parsed_bytes != wanted {
            return Err(format!(
                "replay parsed {} of {wanted} requested bytes",
                outcome.parsed_bytes
            ));
        }
        self.parsed = total;
        Ok(())
    }

    /// Runs one validated entry while retaining the original line number.
    fn entry(
        &mut self,
        number: usize,
        kind: &str,
        data: &Json,
        output: &Path,
        elapsed_ms: u64,
    ) -> Result<(), String> {
        let now = self.started + Duration::from_millis(elapsed_ms);
        // The file order is authoritative: `t` has only millisecond
        // resolution and can tie across a key and several PTY reads.
        match kind {
            "pty-read" => self.pending.extend(recorded_bytes(data)?),
            "pty-write" => self.expected.push((number, recorded_bytes(data)?)),
            "parse" => self.parse_to(
                recording::field_u64(data, "total")?,
                usize::try_from(recording::field_u64(data, "offered")?)
                    .map_err(|_| "offered bytes overflow")?,
                now,
            )?,
            "input" => {
                // Adapter state includes modifiers and IME preedit across
                // callbacks, exactly as it does in the live window.
                let raw = recording::raw_from_json(data)?;
                let command = self.adapter.apply(raw);
                self.command = Some(recording::command_json(command.as_ref())?);
                if let Some(command) = command {
                    // Session samples VT input modes at pump time, after
                    // whichever parse entries preceded this input entry.
                    self.session.submit(command, Origin::User);
                    self.session.pump(now);
                }
            }
            "command" => self.command_entry(number, data, now)?,
            "snapshot" => self.snapshot(number, data, now, output)?,
            "frame" => self.frame(number, data, output)?,
            // K2's CPU replay ignores presentation-only callbacks. Drawn
            // replay applies them because a focus cursor or backing extent
            // can change pixels without a single new PTY byte.
            "resize" | "dpr" | "focus" => self.geometry(kind, data)?,
            other => return Err(format!("entry {number}: unknown recording kind {other}")),
        }
        Ok(())
    }

    /// Applies host presentation state only in the GPU replay mode.
    fn geometry(&mut self, kind: &str, data: &Json) -> Result<(), String> {
        if self.drawn.is_none() {
            return Ok(());
        }
        match kind {
            "resize" => {
                let width = u32::try_from(recording::field_u64(data, "width")?)
                    .map_err(|_| "recorded width overflow")?;
                let height = u32::try_from(recording::field_u64(data, "height")?)
                    .map_err(|_| "recorded height overflow")?;
                self.session.set_surface_size([width, height]);
            }
            "dpr" => {
                let value = data
                    .get("value")
                    .and_then(Json::as_f64)
                    .ok_or("recorded DPR missing")? as f32;
                if let Some(probe) = self.drawn.as_mut() {
                    probe.set_dpr(&mut self.session, value)?;
                }
            }
            "focus" => {
                let focused = data
                    .get("focused")
                    .and_then(Json::as_bool)
                    .ok_or("recorded focus missing")?;
                self.session.set_focus(focused);
                if let Some(probe) = self.drawn.as_mut() {
                    probe.set_focus(focused);
                }
            }
            _ => return Err("unknown geometry event".into()),
        }
        Ok(())
    }

    /// Freezes grid state at the same parsed offset as the live UI copied.
    fn snapshot(
        &mut self,
        number: usize,
        data: &Json,
        now: Instant,
        output: &Path,
    ) -> Result<(), String> {
        // A snapshot may be older than the parser state at its later frame
        // presentation. This check uses the byte counter sealed with that publication.
        let frame = recording::field_u64(data, "number")?;
        let parsed = recording::field_u64(data, "parsed")?;
        if parsed != self.parsed {
            return Err(format!(
                "entry {number}: snapshot parsed {parsed}, replay {}",
                self.parsed
            ));
        }
        if let Some(probe) = self.drawn.as_mut() {
            probe.capture(&mut self.session, frame, now, output)?;
        } else {
            let mut drawing = super::drawing::Drawing::default();
            drawing.publish(&mut self.session, now)?;
        }
        // Text is copied now, before any following parse entry mutates VT.
        // K3 will also compare the pixels from this exact frozen frame.
        let view = self.session.buffer();
        let lines = view.line_ids().ok_or("snapshot lacks visible rows")?;
        self.snapshots
            .insert(frame, (parsed, view.text(lines, Default::default())));
        Ok(())
    }

    /// Materializes a presented grid from its earlier frozen snapshot.
    fn frame(&mut self, number: usize, data: &Json, output: &Path) -> Result<(), String> {
        // Successful presentation refers to an earlier snapshot number.
        // Do not rebuild the grid from the latest VT state at this point.
        let frame = recording::field_u64(data, "number")?;
        let parsed = recording::field_u64(data, "parsed")?;
        let (frozen, grid) = self
            .snapshots
            .remove(&frame)
            .ok_or("frame lacks snapshot")?;
        if parsed != frozen {
            return Err(format!(
                "entry {number}: frame offset differs from snapshot"
            ));
        }
        std::fs::write(
            // A numbered dump makes the user's visible-frame chronology
            // reviewable without including private data in source control.
            output.join(format!("replay-frame-{frame:03}.grid.txt")),
            grid,
        )
        .map_err(|error| error.to_string())
    }

    /// Compares each OS write against the encoder or VT reply that produced it.
    fn finish(self) -> Result<(), String> {
        // Reply writes may occur during a parse entry before its own `parse`
        // record is flushed. Comparing the full ordered streams avoids a
        // false mismatch caused only by that diagnostic-entry placement.
        let actual = self.writes.lock().map_err(|error| error.to_string())?;
        if actual.len() != self.expected.len() {
            return Err(format!(
                "PTY write count changed: recorded {}, new {}",
                self.expected.len(),
                actual.len()
            ));
        }
        for ((number, expected), actual) in self.expected.iter().zip(actual.iter()) {
            // Name the original `pty-write` line, not an inferred key event;
            // a query reply and a user key share this transport.
            if expected != actual {
                return Err(format!(
                    "entry {number}: recorded {}, new {}",
                    recording::hex(expected),
                    recording::hex(actual)
                ));
            }
        }
        Ok(())
    }
}

/// Decodes one hex field without accepting malformed external bytes.
fn recorded_bytes(data: &Json) -> Result<Vec<u8>, String> {
    // Hex conversion validates both nibbles and rejects a truncated tail.
    recording::unhex(recording::field_str(data, "hex")?)
}

/// Exit records are trusted only after strict validation in this child module.
mod exit;
/// Strict restoration of coalesced pointer commands without a key-adapter witness.
mod pointer;

#[cfg(test)]
/// Hand-authored event timelines prove byte matching and frame boundaries.
mod tests;
