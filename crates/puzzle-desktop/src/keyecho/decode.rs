//! Decodes terminal input without assuming PTY read boundaries (§6.8).
//!
//! Legacy Ctrl+letter is one C0 byte. Kitty reports use CSI code;modifier u,
//! optionally with an event kind after `:` and associated text after another
//! semicolon. Query replies share the CSI-u final but start with `?`, so
//! they must be recognized before user keys. Unknown input becomes RAW
//! evidence instead of being silently counted as a successful key.
//!
//! CSI is a stream grammar, not a PTY read grammar. An escape may end at
//! the next read, so the decoder buffers a suffix until it finds a final
//! byte in the ECMA-48 range. A 256-byte cap prevents malformed input
//! from growing that buffer without bound; the capped bytes become RAW.
//! The key simulator recognizes only ASCII letters because K4 exercises
//! Ctrl+A through Ctrl+Z. Other input should fail visibly on its RAW row
//! instead of being assigned an invented command meaning.
//! Kitty's modifier number is one-based; subtracting one yields the
//! neutral bit mask used by the adapter. A `:3` event is release, while
//! omitted event kinds are presses. Associated text after a later semicolon
//! is irrelevant to the key identity and remains within the CSI boundary.

/// One decoded key transition used by the screen witness.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct KeyReport {
    /// Layout-aware ASCII letter; other codes are shown as RAW.
    pub letter: char,
    /// Neutral modifier mask; Ctrl is bit 2 and Shift is bit 0.
    pub modifiers: u8,
    /// Kitty's press/repeat/release kind, with legacy input as press.
    pub phase: Phase,
}

/// A release changes counters but never performs the command twice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    /// Initial keydown.
    Press,
    /// Auto-repeat keydown.
    Repeat,
    /// Keyup reported only under the requested kitty flags.
    Release,
}

/// Replies and keys share one input byte stream from the terminal.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Event {
    /// DECRQM says mode 2026 is supported and currently set or reset.
    SyncSupported,
    /// The terminal answered the kitty keyboard flags query.
    KittyReply,
    /// Other startup query reply with no keyboard action.
    Ignore,
    /// One recognized letter and its modifier/event fields.
    Key(KeyReport),
    /// Invalid or unknown bytes must remain visible in the witness.
    Raw(Vec<u8>),
}

/// Incomplete escape sequence retained across PTY reads.
#[derive(Default)]
pub(super) struct Decoder {
    /// At most 256 unconsumed bytes before returning RAW evidence.
    pending: Vec<u8>,
}

impl Decoder {
    /// Appends a bounded read while retaining any earlier CSI prefix.
    pub(super) fn feed(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
    }

    /// Returns one event, or waits for the rest of an escape sequence.
    pub(super) fn next(&mut self) -> Option<Event> {
        let first = *self.pending.first()?;
        // Legacy C0 and printable ASCII each consume one byte; an ESC
        // prefix needs a separate CSI scan because it may be incomplete.
        if first != 0x1b {
            self.pending.remove(0);
            return Some(match first {
                1..=26 => Event::Key(KeyReport {
                    letter: char::from(b'a' + first - 1),
                    modifiers: 4,
                    phase: Phase::Press,
                }),
                b'a'..=b'z' => Event::Key(KeyReport {
                    letter: char::from(first),
                    modifiers: 0,
                    phase: Phase::Press,
                }),
                _ => Event::Raw(vec![first]),
            });
        }
        self.escape()
    }

    /// Finds a CSI final without treating a split read as a bad key.
    fn escape(&mut self) -> Option<Event> {
        if self.pending.len() == 1 {
            return None;
        }
        if self.pending[1] != b'[' {
            // Unknown two-byte escapes are evidence, not a guessed letter.
            return Some(Event::Raw(self.pending.drain(..2).collect()));
        }
        let end = self
            .pending
            .iter()
            .enumerate()
            .skip(2)
            .find_map(|(i, byte)| (0x40..=0x7e).contains(byte).then_some(i));
        if let Some(end) = end {
            let bytes: Vec<u8> = self.pending.drain(..=end).collect();
            return Some(parse_csi(bytes));
        }
        // A malformed unclosed sequence cannot consume unbounded memory.
        // Keeping the first 256 bytes as RAW also exposes the fault.
        (self.pending.len() >= 256).then(|| Event::Raw(self.pending.drain(..256).collect()))
    }
}

/// Separates protocol replies from user keys by CSI prefix and final.
fn parse_csi(bytes: Vec<u8>) -> Event {
    let body = &bytes[2..bytes.len() - 1];
    let final_byte = bytes[bytes.len() - 1];
    if body.starts_with(b"?2026;") && body.ends_with(b"$") && final_byte == b'y' {
        // DECRQM distinguishes recognized set/reset (1/2) from unsupported
        // (0); only recognized modes may wrap synchronized updates.
        return match body.get(6) {
            Some(b'1' | b'2') => Event::SyncSupported,
            _ => Event::Ignore,
        };
    }
    if body.first() == Some(&b'?') && final_byte == b'u' {
        return Event::KittyReply;
    }
    if final_byte == b'u'
        && let Some(key) = parse_key(body)
    {
        return Event::Key(key);
    }
    // Other DECRQM answers are startup metadata, not user key mistakes.
    if body.first() == Some(&b'?') && final_byte == b'y' {
        return Event::Ignore;
    }
    Event::Raw(bytes)
}

/// Parses only the ASCII-letter rows K4 exercises from kitty CSI-u.
fn parse_key(body: &[u8]) -> Option<KeyReport> {
    let text = std::str::from_utf8(body).ok()?;
    let mut fields = text.split(';');
    // Kitty flag 4 appends shifted/base alternatives to this field. The
    // simulator acts on the primary layout key, not its physical alternate.
    let code: u32 = fields.next()?.split(':').next()?.parse().ok()?;
    let letter = char::from_u32(code)?.to_ascii_lowercase();
    if !letter.is_ascii_lowercase() {
        return None;
    }
    let mut mode = fields.next()?.split(':');
    let modifier: u8 = mode.next()?.parse().ok()?;
    let phase = match mode.next().unwrap_or("1") {
        "1" => Phase::Press,
        "2" => Phase::Repeat,
        "3" => Phase::Release,
        _ => return None,
    };
    Some(KeyReport {
        letter,
        modifiers: modifier.checked_sub(1)?,
        phase,
    })
}
