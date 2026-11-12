//! Raw OSC validation before vte discards C0 controls from string parameters.
//! Clipboard bodies are not copied. OSC 66, 99 and 3008 retain their exact bounded payloads.
//! Outside OSC a bulk delimiter check avoids per-byte work;
//! only string bodies carry raw lengths and invalid-control state across reads.
//! vte remains the authority on whether a candidate is an actual OSC command.
//! The prefix gate allocates only for families that need the original bytes.
//! Cancellation is distinct from a bad optional field: no cancelled start commits.
//! Reaching the cap is remembered even after storage stops growing.
/// Observer phases need no storage or allocations.
#[derive(Clone, Copy, Default, PartialEq)]
enum Phase {
    /// Bulk spans are passed to vte without walking individual output bytes.
    #[default]
    Outside,
    /// A split ESC may introduce an OSC on the next parser turn.
    Escape,
    /// Count original string bytes, including controls discarded by vte.
    Body,
}
/// A semantic OSC callback sees the raw validity for its current bounded string.
#[derive(Clone, Default)]
pub(super) struct Validator {
    /// Introducer/body state persists across arbitrary PTY read boundaries.
    phase: Phase,
    /// Five-byte prefix recognizes long-field protocols without copying ordinary OSC.
    prefix: [u8; 5],
    /// Prefix progress persists across arbitrarily split reads.
    prefix_len: usize,
    /// OSC 66/99/3008 need exact semicolons beyond vte's sixteen-parameter table.
    pub(super) payload: Option<Vec<u8>>,
    /// Saturating raw size detects truncation even when vte drops controls.
    length: usize,
    /// Any stripped control makes strict protocol data invalid rather than repaired.
    pub(super) invalid: bool,
    /// CAN/SUB cancel the whole string, unlike a bad control inside one field.
    pub(super) cancelled: bool,
}
impl Validator {
    /// Observes through at most one OSC terminator before its vte callback runs.
    pub(super) fn span(&mut self, bytes: &[u8]) -> usize {
        if self.phase == Phase::Outside && !bytes.contains(&b']') {
            // Most slices contain no string introducer. Keep vte's contiguous fast path.
            if bytes.last() == Some(&0x1b) {
                self.phase = Phase::Escape;
            }
            return bytes.len();
        }
        // A slice containing an introducer must be examined until its terminator.
        // This observer does not decide dispatch; vte still owns semantic state.
        for (index, &byte) in bytes.iter().enumerate() {
            match self.phase {
                Phase::Body if matches!(byte, 0x07 | 0x18 | 0x1a | 0x1b) => {
                    // Cancellation cannot turn an incomplete write into valid data.
                    self.cancelled = matches!(byte, 0x18 | 0x1a);
                    self.invalid |= self.cancelled;
                    self.phase = if byte == 0x1b {
                        Phase::Escape
                    } else {
                        Phase::Outside
                    };
                    // Expose validity before vte invokes this OSC callback.
                    // A following OSC must not overwrite the just-completed result.
                    return index + 1;
                }
                Phase::Body => {
                    // Count controls too: vte drops them from its bounded payload.
                    self.length = self.length.saturating_add(1);
                    let prefix_byte = self.prefix_len < 5;
                    if prefix_byte {
                        self.prefix[self.prefix_len] = byte;
                        self.prefix_len += 1;
                    }
                    // These families may use more fields than vte's callback table.
                    let begin = prefix_byte
                        && (self.prefix_len == 3 && matches!(&self.prefix[..3], b"99;" | b"66;")
                            || self.prefix_len == 5
                                && &self.prefix == b"3008;"
                                && self.payload.is_none());
                    if begin {
                        self.payload = Some(self.prefix[..self.prefix_len].to_vec());
                    } else if let Some(raw) = &mut self.payload {
                        // Same raw cap as vte; malformed bodies cannot grow unbounded.
                        if raw.len() < crate::osc::MAX_BUFFER_BYTES {
                            raw.push(byte);
                        }
                    }
                    self.invalid |= byte < 0x20 || self.length >= crate::osc::MAX_BUFFER_BYTES;
                }
                Phase::Escape if byte == b']' => {
                    // ESC ] begins a fresh candidate, including across feed calls.
                    self.phase = Phase::Body;
                    self.length = 0;
                    self.prefix_len = 0;
                    self.payload = None;
                    self.invalid = false;
                    self.cancelled = false;
                }
                _ => {
                    // Preserve a final ESC so a split introducer is recognized later.
                    self.phase = if byte == 0x1b {
                        Phase::Escape
                    } else {
                        Phase::Outside
                    }
                }
            }
        }
        bytes.len()
    }
}
