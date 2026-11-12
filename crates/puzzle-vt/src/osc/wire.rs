//! Retain raw OSC bytes that vte deliberately strips from its semantic parameters.
//! This observer never dispatches a command: vte remains the parser authority.
//! Its last candidate is used only when vte yields a registered unknown OSC.
//! Saturating lengths and a fixed cap bound hostile strings across arbitrary reads.
use super::OscRequest;

/// Where the byte observer is within the most recent OSC candidate.
#[derive(Default, PartialEq)]
enum Phase {
    /// Look for ESC, without retaining ordinary output.
    #[default]
    Outside,
    /// ESC can introduce OSC or terminate an existing OSC.
    Escape,
    /// Retain bytes until BEL, cancellation or ESC.
    Body,
    /// vte dispatches at ESC; wait for the actual backslash before invoking a handler.
    Terminator,
    /// A complete BEL/ST sequence; retained until vte's yield is processed.
    Complete,
}

/// Optional-protocol raw observation has no allocation until an OSC is encountered.
#[derive(Default)]
pub(crate) struct Wire {
    /// Last candidate's raw body includes number and separators.
    body: Vec<u8>,
    /// Exact raw length even after storage reaches its existing 64-KiB bound.
    length: usize,
    /// Stream position persists across feed boundaries.
    phase: Phase,
    /// A registered semantic dispatch is waiting for the second ST byte.
    pub(crate) pending: bool,
    /// True only for the one-byte BEL terminator.
    bell: bool,
}

/// Observe consumed bytes separately from vte's semantic dispatch callbacks.
impl Wire {
    /// Observes only the prefix actually consumed by vte; retained suffixes wait.
    pub(crate) fn scan(&mut self, bytes: &[u8]) {
        // The caller scans no unconsumed suffix: a yielded extension runs first.
        for &byte in bytes {
            match self.phase {
                Phase::Body => self.body_byte(byte),
                // ESC alone is not ST; a split backslash completes the candidate.
                Phase::Terminator if byte == b'\\' => self.phase = Phase::Complete,
                Phase::Escape | Phase::Terminator if byte == b']' => {
                    // Reuse the previous allocation, even for a newly introduced OSC.
                    self.body.clear();
                    // Length includes the command number and separating semicolon.
                    self.length = 0;
                    self.phase = Phase::Body;
                    self.bell = false;
                }
                // An ordinary byte or a non-OSC escape abandons the candidate.
                _ => {
                    self.phase = if byte == 0x1b {
                        Phase::Escape
                    } else {
                        Phase::Outside
                    }
                }
            }
        }
    }

    /// Keeps controls instead of silently turning a malformed key into a valid one.
    fn body_byte(&mut self, byte: u8) {
        match byte {
            // BEL terminates without contributing to the retained body.
            0x07 => {
                self.phase = Phase::Complete;
                self.bell = true;
            }
            0x1b => self.phase = Phase::Terminator,
            // CAN/SUB cancel the sequence, even though vte dispatches its partial fields.
            0x18 | 0x1a => self.phase = Phase::Outside,
            _ => {
                // Count discarded bytes too: a protocol may have a smaller wire cap.
                self.length = self.length.saturating_add(1);
                // The raw observer obeys the same storage bound as the VT parser.
                if self.body.len() < crate::osc::MAX_BUFFER_BYTES {
                    self.body.push(byte);
                }
            }
        }
    }

    /// A registered dispatch is ready only after BEL or the complete two-byte ST.
    pub(crate) fn finish(&mut self, request: &mut OscRequest) -> bool {
        // Delay a vte dispatch at ESC until the following byte proves it was ST.
        self.pending = self.phase == Phase::Terminator;
        if self.phase != Phase::Complete {
            return false;
        }
        // Raw numeric identity must agree: ignored controls cannot fabricate registration.
        let split = self.body.iter().position(|b| *b == b';');
        let number = split.map_or(self.body.as_slice(), |at| &self.body[..at]);
        if std::str::from_utf8(number)
            .ok()
            .and_then(|s| s.parse::<u16>().ok())
            != Some(request.number)
        {
            return false;
        }
        // Keep the original selector too. Numeric dispatch may recognize aliases,
        // but a handler authenticates original bytes, never a formatted number.
        request.selector.clear();
        request.selector.extend_from_slice(number);
        // Strip only the numeric prefix; preserve every payload byte for validation.
        request.payload.clear();
        if let Some(at) = split {
            request.payload.extend_from_slice(&self.body[at + 1..]);
        }
        // ESC ] and BEL add three bytes; ESC ] and ESC-backslash add four.
        request.sequence_bytes = self.length.saturating_add(if self.bell { 3 } else { 4 });
        // Reaching the cap is truncation even when the terminator arrives next.
        request.truncated = self.length >= crate::osc::MAX_BUFFER_BYTES;
        request.bell_terminated = self.bell;
        true
    }
}
