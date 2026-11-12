//! Kitty control reference: <https://sw.kovidgoyal.net/kitty/graphics-protocol/>.
//! Kitty control pairs are bounded ASCII keys, not an unbounded dictionary.
//! Unknown keys are retained for forward compatibility; known values are checked
//! before any transfer or placement changes. Duplicate keys are malformed.
use crate::images::{Format, ImageError};

/// A wire value is either an unsigned/signed decimal or one literal protocol letter.
#[derive(Clone, Copy, Debug)]
enum Value {
    /// All protocol integers fit signed/unsigned 32-bit ranges, stored losslessly here.
    Number(i64),
    /// Actions, transfer medium, compression and delete selectors use one ASCII letter.
    Letter(u8),
}

/// Parsed metadata owns no payload and allocates no memory.
#[derive(Clone, Copy, Debug)]
pub struct Control {
    /// At most one entry for each uppercase/lowercase ASCII key.
    fields: [Option<Value>; 52],
}

/// Pure access and validation keep malformed controls out of all mutable state.
impl Control {
    /// Salvages only numeric reply headers from otherwise malformed requests.
    pub(crate) fn reply_headers(body: &[u8]) -> Self {
        let mut result = Self { fields: [None; 52] };
        // Even a bad control list may contain a valid correlation key. Only
        // nonnegative numeric reply fields are salvaged; actions are never run.
        let header = body.split(|byte| *byte == b';').next().unwrap_or_default();
        for pair in header.split(|byte| *byte == b',') {
            if pair.len() >= 3
                && pair[1] == b'='
                && b"iIpq".contains(&pair[0])
                && let Ok(text) = std::str::from_utf8(&pair[2..])
                && let Ok(number) = text.parse::<u32>()
            {
                result.set_number(pair[0], number);
            }
        }
        result
    }

    /// Continuation chunks cannot replace the first chunk's transmission metadata.
    pub(crate) fn continuation(&self) -> bool {
        // Unknown keys cannot silently turn a continuation into new metadata.
        // Only m and q are permitted after the original transmission header.
        self.fields
            .iter()
            .enumerate()
            .all(|(i, value)| value.is_none() || Some(i) == index(b'm') || Some(i) == index(b'q'))
    }

    /// The owner may assign a fresh image ID or inherit a continuation quiet policy.
    pub(crate) fn set_number(&mut self, key: u8, value: u32) {
        if let Some(index) = index(key) {
            self.fields[index] = Some(Value::Number(i64::from(value)));
        }
    }
    /// Parses the body excluding APC framing, returning a borrowed Base64 payload.
    pub fn parse(body: &[u8]) -> Result<(Self, &[u8]), ImageError> {
        // Public callers get both syntactic and semantic validation. The action
        // dispatcher uses syntax separately solely to generate error replies.
        let result = Self::syntax(body)?;
        result.0.validate()?;
        Ok(result)
    }

    /// Reads reply identity even when the requested action is unsupported.
    pub(crate) fn syntax(body: &[u8]) -> Result<(Self, &[u8]), ImageError> {
        // The APC filter already bounds framing, but direct core callers must
        // obey the same cap before a control table or payload can be interpreted.
        let split = body.iter().position(|b| *b == b';').unwrap_or(body.len());
        if split > crate::vt::apc::CONTROL_BYTES {
            return Err(ImageError::Invalid);
        }
        // Fixed slots avoid a hash table allocation for each image chunk.
        // Case matters: I is an image number, while i is an image identifier.
        let mut control = Self { fields: [None; 52] };
        for pair in body[..split]
            .split(|b| *b == b',')
            .filter(|p| !p.is_empty())
        {
            // A key occupies exactly one ASCII letter followed by '='. Empty
            // values and duplicate keys are ambiguous and therefore rejected.
            if pair.len() < 3 || pair[1] != b'=' {
                return Err(ImageError::Invalid);
            }
            // Checked indexing prevents arbitrary wire bytes reaching array
            // indexing; unknown alphabetic keys remain forward-compatible.
            let slot = index(pair[0]).ok_or(ImageError::Invalid)?;
            if control.fields[slot].is_some() {
                return Err(ImageError::Invalid);
            }
            // Letter values are used by actions, compression and media. Numeric
            // values are parsed separately so letters cannot silently become zero.
            let raw = &pair[2..];
            control.fields[slot] = Some(parse_value(pair[0], raw)?);
        }
        // Commands without pixels (put/delete) need no semicolon. A transmission
        // still validates its empty payload against the declared decoded length.
        let payload = body.get(split + 1..).unwrap_or_default();
        if payload.len() > crate::vt::apc::CHUNK_BYTES {
            return Err(ImageError::Invalid);
        }
        Ok((control, payload))
    }

    /// Presence differs from a zero value for mutually exclusive image identifiers.
    pub fn has(&self, key: u8) -> bool {
        index(key).is_some_and(|index| self.fields[index].is_some())
    }

    /// Returns a numeric key with its protocol default, rejecting a letter in its place.
    pub fn number(&self, key: u8, default: u32) -> Result<u32, ImageError> {
        // Missing and explicit zero are distinct at higher-level identity checks;
        // this accessor applies a default only to a genuinely absent key.
        match index(key).and_then(|i| self.fields[i]) {
            None => Ok(default),
            Some(Value::Number(n)) => u32::try_from(n).map_err(|_| ImageError::Invalid),
            Some(Value::Letter(_)) => Err(ImageError::Invalid),
        }
    }

    /// Signed z-index orders placements relative to text and cell backgrounds.
    pub fn z(&self) -> Result<i32, ImageError> {
        // z alone uses the signed accessor; treating it as a u32 would reorder
        // under-text images above the text after a numeric conversion.
        match self.fields[index(b'z').unwrap_or(0)] {
            None => Ok(0),
            Some(Value::Number(n)) => i32::try_from(n).map_err(|_| ImageError::Invalid),
            Some(Value::Letter(_)) => Err(ImageError::Invalid),
        }
    }

    /// Returns an action/medium/selector letter, with its documented default.
    pub fn letter(&self, key: u8, default: u8) -> Result<u8, ImageError> {
        match index(key).and_then(|i| self.fields[i]) {
            None => Ok(default),
            Some(Value::Letter(letter)) => Ok(letter),
            Some(Value::Number(_)) => Err(ImageError::Invalid),
        }
    }

    /// Format 32 (RGBA) is the wire default.
    pub fn format(&self) -> Result<Format, ImageError> {
        // Reject other raw depths and encodings explicitly. PNG normalization
        // belongs to the native worker, keeping all codecs out of shared core.
        match self.number(b'f', 32)? {
            24 => Ok(Format::Rgb),
            32 => Ok(Format::Rgba),
            100 => Ok(Format::Png),
            _ => Err(ImageError::Unsupported),
        }
    }

    /// Validate the supported subset before interpreting action-specific metadata.
    pub(crate) fn validate(&self) -> Result<(), ImageError> {
        // Image IDs select one stored source; image numbers select the newest
        // matching source. A request cannot ask for both interpretations at once.
        if self.has(b'i') && self.has(b'I') {
            return Err(ImageError::Invalid);
        }
        // Never expose filenames or shared memory to a remote byte producer.
        // Unsupported animations also fail here before any accumulator changes.
        if self.letter(b't', b'd')? != b'd' || !b"tTpdq".contains(&self.letter(b'a', b't')?) {
            return Err(ImageError::Unsupported);
        }
        // Relative placements remain deferred; U=1 is an invisible prototype.
        if b"PQHV".iter().any(|k| self.has(*k)) {
            return Err(ImageError::Unsupported);
        }
        // Compression is an optional single-letter protocol mode. An absent o
        // is distinct from an invalid supplied letter or a numeric replacement.
        if self.has(b'o') && self.letter(b'o', b'z')? != b'z' {
            return Err(ImageError::Unsupported);
        }
        // Validate every known numeric key even if this particular action will
        // not use it. A malformed offset cannot become valid after a later put.
        for key in b"iIpsvSxywhXYcr" {
            self.number(*key, 0)?;
        }
        self.z()?;
        // These are closed wire policies, not arbitrary nonzero booleans.
        // Rejecting undefined values avoids inconsistent quiet/cursor behavior.
        if self.number(b'q', 0)? > 2
            || self.number(b'm', 0)? > 1
            || self.number(b'C', 0)? > 1
            || self.number(b'U', 0)? > 1
        {
            return Err(ImageError::Invalid);
        }
        Ok(())
    }
}

/// ASCII key indexing is total and never indexes outside the fixed control table.
fn index(key: u8) -> Option<usize> {
    // Map both ASCII alphabets into disjoint slots; punctuation and non-ASCII
    // bytes fail before any caller can index the fixed-size metadata array.
    match key {
        b'A'..=b'Z' => Some(usize::from(key - b'A')),
        b'a'..=b'z' => Some(26 + usize::from(key - b'a')),
        _ => None,
    }
}

#[cfg(test)]
mod tests;

/// Decodes a single control value with the same signed-range rules as the wire grammar.
fn parse_value(key: u8, raw: &[u8]) -> Result<Value, ImageError> {
    // Value decoding is separate from key uniqueness and field-table construction.
    Ok(if raw.len() == 1 && raw[0].is_ascii_alphabetic() {
        Value::Letter(raw[0])
    } else {
        let text = std::str::from_utf8(raw).map_err(|_| ImageError::Invalid)?;
        // Decimal syntax is deliberately narrow: no whitespace, '+',
        // exponent, or alternate radix can disguise an out-of-range value.
        if !raw
            .iter()
            .enumerate()
            .all(|(i, b)| b.is_ascii_digit() || (i == 0 && *b == b'-'))
        {
            return Err(ImageError::Invalid);
        }
        // Use a wider signed intermediate for u32 IDs and signed z.
        // Only z may be negative; other coordinates must remain unsigned.
        let number = text.parse::<i64>().map_err(|_| ImageError::Invalid)?;
        if number < i64::from(i32::MIN)
            || number > i64::from(u32::MAX)
            || (number < 0 && key != b'z')
        {
            return Err(ImageError::Invalid);
        }
        Value::Number(number)
    })
}
