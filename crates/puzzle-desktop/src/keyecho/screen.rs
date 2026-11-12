//! Numbered alternate-screen updates that expose every decoded key (§13.4).
//!
//! The final row reserves a show-command field like nvim. A press first
//! paints `^D`, then a second update scrolls or changes state and erases it.
//! The two writes are 2, 5 or 7 ms apart: D-33 must wait for the settle
//! window and suppress the short-lived first state without changing VT.
//! DECSTBM restricts line inserts/deletes to numbered rows, leaving status
//! outside the region; this also exercises tmux-like region damage.
//!
//! This program is deliberately a *witness*, not a second terminal core.
//! The user key arrives through the real PTY after the desktop adapter and
//! current VT keyboard modes have selected bytes. Decoder identifies that
//! byte sequence, then this module makes its interpretation visible in ink.
//! A wrong CSI-u modifier therefore appears in the status row, and an
//! unrecognized sequence remains visible in the RAW row. The harness can
//! assert both the transport write and the pixels; neither alone would
//! distinguish an input mistake from a stale rendered frame.
//!
//! The body is exactly 27 rows of stable numbered text. Half-page moves
//! insert or delete eleven lines inside DECSTBM 1;27, one-line moves use
//! one, and full-page moves use 27. The line number at the top changes
//! mechanically, so a visual scroll can be checked without recognizing
//! prose. The status rows remain outside the scrolling region and report
//! the decoded modifiers, counter, repeat count and release count.
//!
//! Show-command painting and erasure are separate synchronized groups.
//! Synchronization groups are used only after DECRQM confirmed mode 2026;
//! otherwise the same bytes are sent without wrappers. A two-millisecond
//! pause between groups spans the later 3.2–6.4-ms nvim measurements.
//! The terminal still stores both states, and
//! D-33 decides whether the first state reaches a display refresh.
//!
//! The child restores the previous screen and pops only its own kitty
//! stack entry on EOF. The harness usually kills it instead; closing the
//! PTY then has the same process-local effect without touching a user shell.

use super::decode::{KeyReport, Phase};
use std::{
    io::{self, Write},
    time::Duration,
};

/// One independent child-side screen and keyboard-mode state.
pub(super) struct Screen {
    /// Delay between the transient command mark and its erase.
    gap: Duration,
    /// Optional repeat cadence for a two-second held Ctrl+D witness.
    repeat_ms: Option<u64>,
    /// Requested kitty bits, pushed only after a positive query reply.
    flags: u8,
    /// DECRQM confirmation controls synchronized update wrappers.
    sync: bool,
    /// Prevents repeated push commands after an unrelated CSI-u reply.
    kitty_enabled: bool,
    /// Number displayed on the first scrollable line.
    first_line: usize,
    /// Ctrl+A/X edit this value without changing PTY input modes.
    counter: i32,
    /// Repeat reports are visible independently of command execution.
    repeats: usize,
    /// Keyup reports are visible independently of command execution.
    releases: usize,
}

impl Screen {
    /// Starts before any response can arrive from the terminal.
    #[cfg(test)]
    pub(super) fn new(flags: u8) -> Self {
        Self::with_gap(flags, 2)
    }

    /// Selects the measured editor erase gap for a pacing witness.
    #[cfg(test)]
    pub(super) fn with_gap(flags: u8, gap_ms: u64) -> Self {
        Self::with_timing(flags, gap_ms, None)
    }

    /// Adds the fixed repeat timing without changing single-key scenarios.
    pub(super) fn with_timing(flags: u8, gap_ms: u64, repeat_ms: Option<u64>) -> Self {
        Self {
            gap: Duration::from_millis(gap_ms),
            repeat_ms,
            flags,
            sync: false,
            kitty_enabled: false,
            first_line: 1,
            counter: 0,
            repeats: 0,
            releases: 0,
        }
    }

    /// Enters alt screen, queries supported modes, then paints numbered rows.
    pub(super) fn start<W: Write>(&mut self, out: &mut W) -> io::Result<()> {
        // Queries occur after 1049 so kitty flags are pushed on the same
        // per-screen stack that later keys observe (§6.8).
        out.write_all(
            b"\x1b[?1049h\x1b[?2026$p\x1b[?2027$p\x1b[?2031$p\x1b[?2048$p\x1b[?u\x1b[2J",
        )?;
        for row in 1..=27 {
            // Every row has a unique numeric marker so DL/IL displacement
            // is observable after a simulated Ctrl+D or Ctrl+U.
            self.line(out, row)?;
        }
        self.status(out, "startup")?;
        out.flush()
    }

    /// Remembers whether the terminal reported synchronized output support.
    pub(super) fn set_sync(&mut self, supported: bool) {
        self.sync = supported;
    }

    /// Pushes once after the terminal's kitty reply confirms support.
    pub(super) fn enable_kitty<W: Write>(&mut self, out: &mut W) -> io::Result<()> {
        if !self.kitty_enabled {
            write!(out, "\x1b[>{}u", self.flags)?;
            // A push belongs to the alternate screen entered at startup;
            // a second query reply must not grow the keyboard stack again.
            self.kitty_enabled = true;
            self.status(out, "kitty ready")?;
        }
        Ok(())
    }

    /// Renders a key and its effect; release never repeats a command.
    pub(super) fn key<W: Write>(&mut self, out: &mut W, key: KeyReport) -> io::Result<()> {
        if key.phase == Phase::Release {
            // Release reporting is independent of the command action.
            // A keyup must not scroll a second half page. Keep its decoded
            // modifiers visible while the separate release count advances.
            self.releases += 1;
            return self.status(out, &modifier_label(key.modifiers));
        }
        if key.phase == Phase::Press && key.letter == 'd' && self.repeat_ms.is_some() {
            return self.repeat_key(out, key);
        }
        self.key_once(out, key)
    }

    /// Emits a measured two-second key hold at the configured repeat rate.
    fn repeat_key<W: Write>(&mut self, out: &mut W, key: KeyReport) -> io::Result<()> {
        let interval = Duration::from_millis(self.repeat_ms.unwrap_or(33));
        let started = std::time::Instant::now();
        for index in 0..60 {
            if index > 0 {
                // Sleep to absolute deadlines so erase work in one repeat
                // does not accumulate cadence drift over the two seconds.
                let due = started + interval * index;
                std::thread::sleep(due.saturating_duration_since(std::time::Instant::now()));
            }
            self.key_once(
                out,
                KeyReport {
                    phase: if index == 0 {
                        Phase::Press
                    } else {
                        Phase::Repeat
                    },
                    ..key
                },
            )?;
            // The PTY must see each erase before the next mark. Otherwise a
            // buffered writer would make the simulated timing meaningless.
            out.flush()?;
        }
        Ok(())
    }

    /// Paints one mark, waits for the selected gap, then erases it.
    fn key_once<W: Write>(&mut self, out: &mut W, key: KeyReport) -> io::Result<()> {
        if key.phase == Phase::Repeat {
            self.repeats += 1;
        }
        self.begin(out)?;
        // This first complete update is intentionally visible in VT state.
        // K5's paced desktop renderer decides whether it is ever presented.
        write!(out, "\x1b[30;70H^{}", key.letter.to_ascii_uppercase())?;
        self.end(out)?;
        out.flush()?;
        // nvim sends the erase in a later synchronized update. The chosen
        // pause exposes all three measured timing regimes to the harness.
        std::thread::sleep(self.gap);
        self.begin(out)?;
        // The erase shares its group with the scroll and status update, so
        // a late snapshot sees the settled application state as one frame.
        self.perform(out, key.letter)?;
        out.write_all(b"\x1b[30;70H          ")?;
        self.status(out, &modifier_label(key.modifiers))?;
        self.end(out)
    }

    /// Makes undecodable input visible instead of silently accepting it.
    pub(super) fn raw<W: Write>(&mut self, out: &mut W, bytes: &[u8]) -> io::Result<()> {
        self.begin(out)?;
        write!(out, "\x1b[28;1H\x1b[2KRAW: {}", caret(bytes))?;
        self.end(out)
    }

    /// Restores the caller's keyboard stack and primary screen on EOF.
    pub(super) fn finish<W: Write>(&mut self, out: &mut W) -> io::Result<()> {
        if self.kitty_enabled {
            out.write_all(b"\x1b[<u")?;
        }
        out.write_all(b"\x1b[?1049l")
    }

    /// Restricts line movement to the numbered body, not the status rows.
    fn perform<W: Write>(&mut self, out: &mut W, letter: char) -> io::Result<()> {
        match letter {
            'd' => self.scroll(out, 11, true),
            'u' => self.scroll(out, 11, false),
            'e' => self.scroll(out, 1, true),
            'y' => self.scroll(out, 1, false),
            'f' => self.scroll(out, 27, true),
            'b' => self.scroll(out, 27, false),
            'a' => {
                self.counter += 1;
                Ok(())
            }
            'x' => {
                self.counter -= 1;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Uses DL/IL plus explicit new lines, matching editor scroll traffic.
    fn scroll<W: Write>(&mut self, out: &mut W, count: usize, down: bool) -> io::Result<()> {
        out.write_all(b"\x1b[1;27r\x1b[1;1H")?;
        // DL at row one moves later rows upward; IL makes space above them.
        // Redrawing only exposed rows follows common editor output patterns.
        write!(out, "\x1b[{}{}", count, if down { 'M' } else { 'L' })?;
        self.first_line = if down {
            // A down-scroll reveals later numbered lines at the bottom.
            self.first_line + count
        } else {
            // The first line never becomes zero or wraps before the file.
            self.first_line.saturating_sub(count).max(1)
        };
        let rows = if down { (28 - count)..=27 } else { 1..=count };
        for row in rows {
            self.line(out, row)?;
        }
        out.write_all(b"\x1b[r")
    }

    /// Reprints one row after IL/DL with a stable numeric identity.
    fn line<W: Write>(&self, out: &mut W, row: usize) -> io::Result<()> {
        write!(
            out,
            "\x1b[{row};1H\x1b[2Kline {:04}",
            self.first_line + row - 1
        )
    }

    /// Keeps mode and event counters outside the scrollable body.
    fn status<W: Write>(&self, out: &mut W, last: &str) -> io::Result<()> {
        // Erasing before each rewrite prevents a shorter status from
        // leaving stale modifier names or counter digits on the screen.
        write!(
            out,
            "\x1b[29;1H\x1b[2Kflags={} last={last} counter={} repeat={} release={}",
            self.flags, self.counter, self.repeats, self.releases
        )
    }

    /// Opens a synchronized group only after DECRQM reported support.
    fn begin<W: Write>(&self, out: &mut W) -> io::Result<()> {
        if self.sync {
            out.write_all(b"\x1b[?2026h")
        } else {
            Ok(())
        }
    }

    /// Closes the group so one complete application update can present.
    fn end<W: Write>(&self, out: &mut W) -> io::Result<()> {
        if self.sync {
            out.write_all(b"\x1b[?2026l")
        } else {
            Ok(())
        }
    }
}

/// Names only the modifier bits actually decoded from the key report.
fn modifier_label(bits: u8) -> String {
    let mut names = Vec::new();
    if bits & 4 != 0 {
        names.push("ctrl");
    }
    if bits & 1 != 0 {
        names.push("shift");
    }
    if bits & 2 != 0 {
        names.push("alt");
    }
    if bits & 8 != 0 {
        names.push("super");
    }
    if names.is_empty() {
        "plain".into()
    } else {
        names.join("+")
    }
}

/// Converts C0 and ESC into terminal-readable caret notation on RAW rows.
fn caret(bytes: &[u8]) -> String {
    let mut text = String::new();
    for &byte in bytes {
        match byte {
            0..=31 => {
                text.push('^');
                text.push(char::from(byte + 64));
            }
            127 => text.push_str("^?"),
            32..=126 => text.push(char::from(byte)),
            _ => text.push('?'),
        }
    }
    text
}
