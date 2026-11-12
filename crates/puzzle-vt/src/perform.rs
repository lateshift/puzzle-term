//! vte callbacks separated from the terminal's grid storage (§6.4).
//!
//! vte decodes bytes and handles split UTF-8 or escape sequences; these
//! callbacks own Puzzle's semantics. Sequence families have separate modules
//! so later work-order tasks can add controls without enlarging this adapter.
/// Initial CSI dispatch for the DECAWM print-path mode.
mod csi;
/// Bounded DECRQSS and XTGETTCAP request handlers.
pub(crate) mod dcs;
/// ESC designation and single-shift dispatch from xterm ctlseqs.
mod esc;
/// Bounded application strings for titles, palette, links and host policy.
mod osc;
/// SGR pen attributes and indexed or RGB color forms.
mod sgr;

use crate::terminal::Terminal;
use vte::{Params, Perform};

/// Borrowed interpreter created once per `Terminal::advance` chunk.
pub(crate) struct Performer<'a, 'b, const WATCH: bool> {
    /// Screen and cursor state modified by parser callbacks.
    pub(crate) term: &'a mut Terminal,
    /// A waiting snapshot may yield at a completed synchronized transaction.
    pub(crate) stop_at_sync_end: bool,
    /// Optional bounded capture; absent on the ordinary bulk parsing path.
    pub(crate) osc: Option<&'a mut crate::osc::Capture<'b>>,
}

impl<const WATCH: bool> Perform for Performer<'_, '_, WATCH> {
    /// Only a semantic transaction end can interrupt the explicitly bounded parser.
    fn terminated(&self) -> bool {
        self.term.reply_boundary_ready()
            || self.osc.as_ref().is_some_and(|capture| capture.ready)
            || self.stop_at_sync_end && self.term.modes.get(true, 2026) == Some(false)
    }

    /// Writes one decoded Unicode scalar through delayed-wrap rules.
    fn print(&mut self, ch: char) {
        self.term.print_char_watched::<WATCH>(ch);
    }

    /// Executes a C0 control while retaining parser state across chunks.
    fn execute(&mut self, byte: u8) {
        self.term.execute_c0(byte);
    }

    /// Dispatches the few CSI controls required by this task's print path.
    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        if !ignore {
            csi::dispatch::<WATCH>(self.term, params, intermediates, action);
        }
    }

    /// Dispatches ISO-2022 charset designation and shift functions.
    fn esc_dispatch(&mut self, intermediates: &[u8], ignore: bool, byte: u8) {
        if !ignore {
            esc::dispatch(self.term, intermediates, byte);
        }
    }

    /// Begins one bounded DCS request after vte recognizes its introducer.
    fn hook(&mut self, _params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        dcs::hook(self.term, intermediates, ignore, action);
    }

    /// Retains only recognized request bytes up to the local DCS cap.
    fn put(&mut self, byte: u8) {
        dcs::put(self.term, byte);
    }

    /// Answers a valid request or discards an incomplete one.
    fn unhook(&mut self) {
        dcs::unhook(self.term);
    }

    /// Dispatches OSC after vte bounded and separated its raw string.
    fn osc_dispatch(&mut self, params: &[&[u8]], bell_terminated: bool) {
        self.term.note_osc(params);
        osc::dispatch(self.term, params, bell_terminated);
        if let Some(capture) = &mut self.osc
            && let Some(number) = params
                .first()
                .and_then(|bytes| std::str::from_utf8(bytes).ok())
                .and_then(|text| text.parse::<u16>().ok())
            && !crate::osc::is_builtin(number)
        {
            // Core semantics always win; the handler receives only unclaimed commands.
            capture.capture(number);
        }
    }
}
