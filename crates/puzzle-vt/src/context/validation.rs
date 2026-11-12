//! Shared decoded-field validation for wire commands and recorded facts.
//! The parser unescapes once; replay must validate decoded values without
//! interpreting literal backslashes as a second round of wire escapes.
//! UAPI.15 distinguishes categorical values, opaque strings, unsigned decimal
//! identities and hexadecimal machine/boot IDs. Validation is deliberately
//! independent per key: a corrupt optional field cannot hide a valid ancestor.
use super::{FieldKey, MAX_TEXT};

/// Validate retained metadata using the same rules for OSC and event replay.
pub(super) fn valid(key: FieldKey, value: &str) -> bool {
    // Hosts receive decoded text, so a literal backslash is valid at this boundary.
    if value.chars().count() > MAX_TEXT
        || value.chars().any(|c| c <= '\u{1f}' || c == '\u{7f}')
        || value.is_empty() && key != FieldKey::Cmdline
    {
        return false;
    }
    // A type is descriptive but closed: arbitrary strings cannot acquire a
    // meaning that the host did not agree to display.
    match key {
        FieldKey::Type => matches!(
            value,
            "boot"
                | "container"
                | "vm"
                | "elevate"
                | "chpriv"
                | "subcontext"
                | "remote"
                | "shell"
                | "command"
                | "app"
                | "service"
                | "session"
        ),
        // No signs, separators or saturation: identities are unsigned decimal u64.
        FieldKey::Pid | FieldKey::PidFdId => {
            value.bytes().all(|b| b.is_ascii_digit())
                && value.len() <= 20
                && value.parse::<u64>().is_ok()
        }
        FieldKey::MachineId | FieldKey::BootId => {
            // ID128's ABNF permits hyphenated UUID spelling and bare hex.
            // Do not normalize case or formatting: metadata is reported verbatim.
            (32..=36).contains(&value.len())
                && value.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
        }
        // Other recognized keys are opaque text. Interpreting paths, usernames
        // or hostnames belongs to a host, never to this untrusted parser.
        _ => true,
    }
}
