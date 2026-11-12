//! Recording-only OSC 5522 redaction preserves offsets without storing credentials.
//! Separate reader/writer instances retain framing across arbitrary OS chunks.
//! The ordinary, unrecorded transport never runs this scanner or allocates copies.
//! OSC numeric spellings are normalized by the parser, so redaction also handles
//! leading zeroes and the optional plus sign instead of only the canonical spelling.
//! Only the numeric prefix is observed; the private body is never retained.
//! Replacing bytes one-for-one preserves recording offsets and frame byte counts.
//! String terminators stay exact so unrelated following output remains replayable.
//! Clipboard bodies are intentionally not replayable from a redacted recording.
//! Read and write directions have separate state so interleaving cannot mix framing.
//! The scanner exists only when recording was explicitly enabled by the host.

/// Numeric OSC recognition needs only scalar state, no payload buffering.
#[derive(Default)]
pub(super) struct Redactor {
    /// Introducer phase: zero outside, one after ESC, two in the OSC number.
    prefix: u8,
    /// Checked numeric spelling also accepts leading zeros, like the VT dispatcher.
    number: Option<u16>,
    /// A leading plus is valid only before any digit, following Rust numeric parsing.
    digits: bool,
    /// All clipboard metadata/data stays private until the string terminator.
    body: bool,
}

impl Redactor {
    /// Copies diagnostic bytes, replacing credential-bearing string bodies by stars.
    pub(super) fn filter(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut safe = Vec::with_capacity(bytes.len());
        for &byte in bytes {
            if self.body {
                // BEL, cancellation and ESC end this string; ST's slash is ordinary.
                if matches!(byte, 7 | 24 | 26 | 27) {
                    self.body = false;
                    self.prefix = u8::from(byte == 27);
                    safe.push(byte);
                } else {
                    safe.push(b'*');
                }
            } else {
                safe.push(byte);
                self.observe(byte);
            }
        }
        safe
    }

    /// Recognizes the same numeric family without retaining private metadata.
    fn observe(&mut self, byte: u8) {
        if self.prefix == 2 {
            match byte {
                b'0'..=b'9' => {
                    self.digits = true;
                    self.number = self.number.and_then(|number| {
                        number.checked_mul(10)?.checked_add(u16::from(byte - b'0'))
                    });
                    return;
                }
                b'+' if !self.digits && self.number == Some(0) => {
                    // Consume one optional sign; a second plus invalidates the number.
                    self.digits = true;
                    return;
                }
                b';' => self.body = self.number == Some(5522),
                _ => {}
            }
        } else if self.prefix == 1 && byte == b']' {
            self.prefix = 2;
            self.number = Some(0);
            self.digits = false;
            return;
        }
        // A new ESC begins a candidate after any mismatching byte or terminator.
        self.prefix = u8::from(byte == 27);
    }
}
