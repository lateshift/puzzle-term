//! Origin: kitty; <https://sw.kovidgoyal.net/kitty/graphics-protocol/>.
//! Incremental kitty APC extraction ahead of vte (design §6.4, M15.1).
//!
//! vte still receives the introducer and terminator, so its string state and
//! interrupted UTF-8 retain their normal semantics. Only a graphics body's
//! bytes are removed. Other strings and ordinary output pass without copying.
//! A consumer may stop at a semantic VT boundary or after a complete graphics
//! command; the returned source offset leaves every later reply in order.
//! Protocol reference: https://sw.kovidgoyal.net/kitty/graphics-protocol/.
mod scan;
#[cfg(test)]
mod tests;

/// Kitty limits each base64 transmission chunk to 4,096 encoded bytes.
pub const CHUNK_BYTES: usize = 4096;
/// One-letter ASCII keys have at most eleven signed decimal value characters.
/// Allow every upper/lowercase key once, including '=' and a separating comma.
pub const CONTROL_BYTES: usize = 52 * (1 + 1 + 11 + 1);
/// Storage holds control data, its semicolon, and one protocol chunk only.
pub const BODY_BYTES: usize = CONTROL_BYTES + 1 + CHUNK_BYTES;

/// Consumer of ordered VT spans and complete, bounded graphics commands.
pub trait ApcSink {
    /// Parses a borrowed VT span and returns its consumed prefix length.
    /// A shorter prefix yields immediately without consuming any later bytes.
    fn passthrough(&mut self, bytes: &[u8]) -> usize;
    /// Handles control data and encoded payload, excluding ESC _ G and ST.
    /// Returning false yields after this command, before subsequent VT queries.
    fn graphics(&mut self, body: &[u8]) -> bool;
}

/// Only the prefix and graphics string need state; ordinary VT stays in vte.
#[derive(Clone, Copy, Default, Eq, PartialEq)]
enum Phase {
    /// Search for an underscore immediately preceded by ESC.
    #[default]
    Search,
    /// ESC _ already reached vte; only an immediate G selects kitty graphics.
    Candidate,
    /// Accumulate one bounded body while withholding it from vte.
    Body,
    /// ESC already reached vte; backslash completes ST, other bytes cancel.
    Terminator,
}

/// Reusable byte pre-filter, independent of clocks, threads and image storage.
#[derive(Default)]
pub struct ApcFilter {
    /// State persists across kernel reads, parser slices and control yields.
    phase: Phase,
    /// An ESC at the end of a passed span may introduce the next slice's APC.
    previous_escape: bool,
    /// Allocated only on the first kitty command, then reused for every chunk.
    body: Vec<u8>,
    /// Control and payload lengths have independent protocol-derived bounds.
    control_bytes: usize,
    /// None until the optional separating semicolon starts encoded pixel data.
    payload_bytes: Option<usize>,
    /// Discard the entire overlong command, logging exactly once for that string.
    overflow: bool,
}

/// Every consumed byte is either passed once or belongs to one graphics body.
impl ApcFilter {
    /// Skips the adapter on an ordinary slice with no possible APC introducer.
    /// Slice contains uses the standard library's optimized byte search. Keeping
    /// this separate lets Terminal retain its original bulk-parser call shape.
    pub(crate) fn can_skip(&self, bytes: &[u8]) -> bool {
        self.phase == Phase::Search && !bytes.contains(&b'_')
    }

    /// Records only bytes actually accepted by a semantic parser yield.
    pub(crate) fn passed(&mut self, bytes: &[u8]) {
        if let Some(last) = bytes.last() {
            self.previous_escape = *last == 0x1b;
        }
    }

    /// Non-yielding bulk parsing accepts the entire supplied span.
    pub(crate) fn plain(&mut self, bytes: &[u8]) -> bool {
        if self.phase != Phase::Search || bytes.contains(&b'_') {
            return false;
        }
        if let Some(last) = bytes.last() {
            // Empty feeds must not forget an ESC saved at the previous boundary.
            self.previous_escape = *last == 0x1b;
        }
        true
    }

    /// Feeds a source prefix, stopping only when the sink explicitly yields.
    /// Malformed strings recover on CAN/SUB, ST or a new ESC sequence. Ordinary
    /// text after a missing terminator is still part of that string, as in VT.
    pub fn advance(&mut self, bytes: &[u8], sink: &mut impl ApcSink) -> usize {
        let mut used = 0;
        while used < bytes.len() {
            let (count, keep_going) = match self.phase {
                Phase::Search => self.search(&bytes[used..], sink),
                Phase::Candidate => self.candidate(&bytes[used..], sink),
                Phase::Body => self.body_span(&bytes[used..], sink),
                Phase::Terminator => self.terminator(&bytes[used..], sink),
            };
            used += count;
            if !keep_going {
                break;
            }
            // A zero-byte transition only abandons Terminator for Search; the
            // next iteration therefore either consumes input or yields to vte.
        }
        used
    }

    /// Resets storage only after G proves the candidate is graphics, not text.
    fn begin_body(&mut self) {
        if self.body.capacity() == 0 {
            // Reserve exactly the security bound, avoiding geometric overshoot.
            self.body.reserve_exact(BODY_BYTES);
        }
        self.body.clear();
        self.control_bytes = 0;
        self.payload_bytes = None;
        self.overflow = false;
        self.phase = Phase::Body;
    }

    /// Counts even discarded bytes, with saturation on arbitrarily long input.
    fn retain(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            let too_long = if let Some(length) = &mut self.payload_bytes {
                *length = length.saturating_add(1);
                *length > CHUNK_BYTES
            } else if byte == b';' {
                self.payload_bytes = Some(0);
                false
            } else {
                self.control_bytes = self.control_bytes.saturating_add(1);
                self.control_bytes > CONTROL_BYTES
            };
            if too_long && !self.overflow {
                self.overflow = true;
                self.body.clear();
                tracing::warn!(
                    event = "vt.graphics_apc_oversized",
                    "discarding overlong graphics command through its terminator"
                );
            }
            if !self.overflow {
                self.body.push(byte);
            }
        }
    }
}
