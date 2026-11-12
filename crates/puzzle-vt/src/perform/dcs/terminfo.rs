//! XTGETTCAP values from the same source compiled by `tic` (§6.12).
//!
//! Applications query these names as hex encoded ASCII. Returning the
//! checked-in source's value avoids a second hand-maintained claim about
//! which extensions Puzzle supports. `tic` syntax uses backslash escapes;
//! the wire value is the decoded capability string, with `%p` parameter
//! operators retained for the querying application to evaluate.

/// Canonical D-32 source, also compiled into the native TERMINFO directory.
const SOURCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/terminfo/xterm-puzzle.terminfo"
));

/// Finds one advertised capability, excluding `use` inheritance entries.
pub(super) fn value(name: &[u8]) -> Option<Vec<u8>> {
    // Names are ASCII in XTGETTCAP. A malformed UTF-8 name is unsupported
    // instead of being reflected into a response.
    let name = std::str::from_utf8(name).ok()?;
    for line in SOURCE.lines() {
        let line = line.trim();
        // A request cannot probe source comments or inherited capabilities;
        // the latter belong to the system xterm database, not our claim.
        if line.starts_with('#') || line.starts_with("use=") {
            continue;
        }
        let Some((key, rest)) = line.trim_end_matches(',').split_once('=') else {
            // Flags such as Tc have no string payload. A successful empty
            // value distinguishes them from a missing capability.
            if line.trim_end_matches(',') == name {
                return Some(Vec::new());
            }
            continue;
        };
        if key == name {
            // The `%p1` parameter program is returned unchanged; XTGETTCAP
            // asks for a capability definition, not a rendered sequence.
            return decode(rest);
        }
    }
    None
}

/// Decodes the ncurses source escapes used by Puzzle's capability list.
fn decode(input: &str) -> Option<Vec<u8>> {
    // Source strings are ASCII. Iterate bytes so terminal controls cannot
    // be altered by Unicode normalization or character escaping.
    let mut bytes = input.bytes();
    let mut value = Vec::with_capacity(input.len());
    while let Some(byte) = bytes.next() {
        if byte == b'^' {
            // The source's `^G` is BEL, used by the OSC title terminator.
            value.push(bytes.next()? & 0x1f);
        } else if byte == b'\\' {
            match bytes.next()? {
                b'E' | b'e' => value.push(0x1b),
                b'0' => {
                    // `\007` appears in OSC capabilities; consume all
                    // three octal digits as one byte, not a literal zero.
                    let first = bytes.next()?.checked_sub(b'0')?;
                    let second = bytes.next()?.checked_sub(b'0')?;
                    if first > 7 || second > 7 {
                        return None;
                    }
                    value.push(first * 8 + second);
                }
                // Escaped colon is required by terminfo's source grammar;
                // callers receive the actual colon in SGR parameters.
                escaped => value.push(escaped),
            }
        } else {
            value.push(byte);
        }
    }
    Some(value)
}

#[cfg(test)]
mod tests;
