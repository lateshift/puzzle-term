//! Borrow ordinary VT spans; scan only graphics bodies byte by byte (M15.1).
//! Underscore search avoids a second scan of every ANSI escape-heavy corpus.
//! Partial sink consumption updates prefix state only for bytes actually parsed.
use super::{ApcFilter, ApcSink, Phase};

/// The normal path emits one borrowed span without building a scratch string.
impl ApcFilter {
    /// Passes through the next possible ESC _ introducer, or the entire slice.
    pub(super) fn search(&mut self, bytes: &[u8], sink: &mut impl ApcSink) -> (usize, bool) {
        // A candidate can straddle slices; the preceding ESC has already reached vte.
        let mut offset = 0;
        let candidate = loop {
            // Most slices contain no underscore at all. The specialized u8
            // contains implementation vectorizes that negative search before
            // the positional iterator needed for an actual candidate.
            if !bytes[offset..].contains(&b'_') {
                break None;
            }
            let Some(found) = bytes[offset..].iter().position(|byte| *byte == b'_') else {
                break None;
            };
            let at = offset + found;
            // An underscore in a filename or shell prompt is plain data;
            // only adjacency to ESC selects an APC introducer.
            if if at == 0 {
                self.previous_escape
            } else {
                bytes[at - 1] == 0x1b
            } {
                break Some(at + 1);
            }
            offset = at + 1;
        };
        // Include the introducer in the VT span. vte must enter its own APC
        // state even though this filter withholds the graphics body later.
        let offered = candidate.unwrap_or(bytes.len());
        // A consumer can stop on OSC or synchronized-output completion inside
        // this span. Never advance the filter past its accepted VT prefix.
        let used = sink.passthrough(&bytes[..offered]).min(offered);
        if used > 0 {
            self.previous_escape = bytes[used - 1] == 0x1b;
        }
        if used == offered && candidate.is_some() {
            // Only a completely consumed introducer makes G meaningful next.
            self.phase = Phase::Candidate;
        }
        (used, used == offered)
    }

    /// G selects kitty; another APC remains entirely vte's discarded string.
    pub(super) fn candidate(&mut self, bytes: &[u8], sink: &mut impl ApcSink) -> (usize, bool) {
        // The one-byte probe can itself meet a consumer's pending yield.
        if sink.passthrough(&bytes[..1]) == 0 {
            return (0, false);
        }
        if bytes[0] == b'G' {
            // Reserving body storage before G would penalize non-graphics APC.
            self.begin_body();
        } else {
            // Keep an ESC here: it can introduce a new APC across this boundary.
            self.phase = Phase::Search;
            self.previous_escape = bytes[0] == 0x1b;
        }
        (1, true)
    }

    /// Skip bounded body bytes, but send cancellation and ESC through vte itself.
    pub(super) fn body_span(&mut self, bytes: &[u8], sink: &mut impl ApcSink) -> (usize, bool) {
        // CAN and SUB cancel all VT strings; ESC either starts ST or abandons
        // this command for another escape. No ordinary payload byte escapes.
        let count = bytes
            .iter()
            .position(|b| matches!(b, 0x18 | 0x1a | 0x1b))
            .unwrap_or(bytes.len());
        if count > 0 {
            // Bounds apply while bytes arrive, never after buffering a whole
            // hostile transfer. This prefix has no VT-side semantic work.
            self.retain(&bytes[..count]);
            return (count, true);
        }
        if sink.passthrough(&bytes[..1]) == 0 {
            // Keep the cancellation byte untouched when the consumer yields.
            return (0, false);
        }
        self.phase = if bytes[0] == 0x1b {
            Phase::Terminator
        } else {
            Phase::Search
        };
        self.previous_escape = bytes[0] == 0x1b;
        (1, true)
    }

    /// Deliver only after ST has restored vte to ground, before any following text.
    pub(super) fn terminator(&mut self, bytes: &[u8], sink: &mut impl ApcSink) -> (usize, bool) {
        if bytes[0] != b'\\' {
            // ESC starts a new VT sequence: retain that ESC but discard the
            // unfinished graphics request. Search must inspect this next byte.
            self.phase = Phase::Search;
            return (0, true);
        }
        if sink.passthrough(&bytes[..1]) == 0 {
            // ST is incomplete until the consumer accepts its backslash.
            return (0, false);
        }
        self.phase = Phase::Search;
        self.previous_escape = false;
        // An oversized body has no callback, even when its suffix looks valid.
        (1, self.overflow || sink.graphics(&self.body))
    }
}
