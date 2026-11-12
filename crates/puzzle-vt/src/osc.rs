//! Bounded, uninterpreted OSC values for Session-owned extensions (D-44).
//! No bus, plugin, clock or transport enters VT. The parser only captures an
//! explicitly registered unknown number and yields at its terminator. Storage
//! is reused by the caller and never exceeds vte's existing fixed OSC bound.

/// Fixed raw OSC buffer cap from design §6.4 and §16 (64 KiB).
/// The vte parser and raw observers share it so truncation has one boundary.
pub const MAX_BUFFER_BYTES: usize = 64 * 1024;

/// The raw observer preserves controls and counts complete wire sequences.
pub(crate) mod wire;

/// Exact bytes of one unknown OSC, with its selector separate from the payload.
#[derive(Default, Debug)]
pub struct OscRequest {
    /// Numeric command identity; built-in numbers never enter this seam.
    pub number: u16,
    /// Original numeric spelling, never reconstructed from number. Authenticated
    /// protocols must reject aliases such as leading zeroes before verifying MACs.
    pub selector: Vec<u8>,
    /// Semicolons inside the body remain bytes, never lossy UTF-8 conversion.
    pub payload: Vec<u8>,
    /// Replies must use BEL when the request did, otherwise ESC-backslash.
    pub bell_terminated: bool,
    /// The parser lost bytes or parameters; a handler can reject incomplete input.
    pub truncated: bool,
    /// Wire size including introducer, number, separator and actual terminator.
    pub sequence_bytes: usize,
}

/// Borrowed registration interests and one reusable result for this parser turn.
pub(crate) struct Capture<'a> {
    /// Only explicitly registered numbers justify capturing payload bytes.
    pub(crate) numbers: &'a [u16],
    /// Caller-owned allocation survives later extension turns.
    pub(crate) request: &'a mut OscRequest,
    /// vte's termination callback sees this after complete OSC dispatch.
    pub(crate) ready: bool,
}

impl Capture<'_> {
    /// Marks only the numeric dispatch; the raw observer supplies the exact bytes.
    /// vte strips controls and splits parameters, so copying those fields would
    /// silently repair malformed reports before the extension could validate them.
    pub(crate) fn capture(&mut self, number: u16) {
        if self.numbers.contains(&number) {
            self.request.number = number;
            self.ready = true;
        }
    }
}

/// Built-in semantics cannot be shadowed by an extension registration.
pub fn is_builtin(number: u16) -> bool {
    matches!(number, 0..=2 | 4 | 5 | 7..=19 | 21 | 22 | 52 | 66 | 72 | 99 | 104 | 105 | 110..=119 | 133 | 633 | 777 | 3008 | 5522)
}
