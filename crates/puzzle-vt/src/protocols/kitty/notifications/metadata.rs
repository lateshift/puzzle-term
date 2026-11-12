//! Explicit OSC 99 options merge across chunks; absent options keep prior values.
//! Identifiers are validated before they can enter an application reply.
//!
//! Persistent options use Option so an omitted key differs from an explicit reset.
//! Encoding, payload kind and completion are local defaults for each OSC packet.
//! Identifiers may later be echoed into terminal input, so their alphabet excludes
//! separators and controls even when an OS could otherwise display such a name.
//! Unknown keys and actions are ignored to allow future protocol revisions.
//! Invalid values for known keys reject the packet instead of guessing a host action.
use crate::notification::{Notification, Occasion};
/// Packet-local encoding/payload options do not leak into the next packet.
pub(super) struct Metadata {
    /// Identifier, with a finite byte bound for correlation and replies.
    pub(super) id: Option<String>,
    /// Protocol default is title for every packet.
    pub(super) payload: String,
    /// d=0 withholds display until a later completed packet.
    pub(super) done: bool,
    /// Each packet declares whether its payload is encoded.
    pub(super) encoded: bool,
    /// Optional scalars distinguish absent metadata from explicit reset values.
    urgency: Option<u8>,
    /// Host condition is evaluated only on the completed request.
    occasion: Option<Occasion>,
    /// Each supplied action list starts from the protocol defaults.
    actions: Option<(bool, bool)>,
    /// A later c=0 may disable a preceding chunk's requested close report.
    close: Option<bool>,
}
/// Packet parsing and persistent-state updates remain separate operations.
impl Metadata {
    /// Unknown keys are ignored for extensibility; unsafe or malformed known data is refused.
    pub(super) fn parse(raw: &str) -> Option<Self> {
        // Defaults are packet-local, unlike the options retained by Partial.
        let mut out = Self {
            id: None,
            payload: "title".into(),
            done: true,
            encoded: false,
            urgency: None,
            occasion: None,
            actions: None,
            close: None,
        };
        // Colon separates metadata; semicolons belong to the outer OSC grammar.
        for pair in raw.split(':').filter(|s| !s.is_empty()) {
            let (key, value) = pair.split_once('=')?;
            match key {
                "i" => {
                    // Only the protocol identifier alphabet is safe to echo into a reply.
                    if value.is_empty()
                        || value.len() > 256
                        || !value
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"_-+.".contains(&b))
                    {
                        return None;
                    }
                    out.id = Some(value.into());
                }
                "p" => out.payload = value.into(),
                // Completion and encoding accept only explicit protocol boolean values.
                "d" => out.done = boolean(value)?,
                "e" => out.encoded = boolean(value)?,
                "c" => out.close = Some(boolean(value)?),
                // Urgency is a closed protocol enum, never an arbitrary OS priority.
                "u" => {
                    out.urgency = Some(match value {
                        "0" => 0,
                        "1" => 1,
                        "2" => 2,
                        _ => return None,
                    })
                }
                // Host visibility is evaluated later, not guessed while parsing metadata.
                "o" => {
                    out.occasion = Some(match value {
                        "always" => Occasion::Always,
                        "unfocused" => Occasion::Unfocused,
                        "invisible" => Occasion::Invisible,
                        _ => return None,
                    })
                }
                // A supplied action list starts with focus enabled, even when only report is named.
                "a" => out.actions = Some(actions(value)),
                _ => {}
            }
        }
        // Successful parsing has validated every persistent scalar before merge.
        Some(out)
    }
    /// Repeated options replace previous metadata while omitted keys preserve it.
    pub(super) fn apply(&self, out: &mut Notification) {
        // The caller already selected the partial by this optional identifier.
        out.id.clone_from(&self.id);
        // Updating a chunk preserves every persistent field that was not supplied.
        if let Some(u) = self.urgency {
            out.urgency = u;
        }
        // An explicit occasion replaces the previous chunk's condition.
        if let Some(o) = self.occasion {
            out.occasion = o;
        }
        if let Some((focus, report)) = self.actions {
            out.focus = focus;
            out.report = report;
        }
        // c=0 is meaningful: it can cancel a preceding request for close reports.
        if let Some(close) = self.close {
            out.close_report = close;
        }
    }
}
/// Only the protocol's two boolean spellings are accepted.
/// Rust boolean parsing is unsuitable here: the wire spells booleans as digits.
fn boolean(value: &str) -> Option<bool> {
    match value {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    }
}

/// Each action list starts with focus enabled; unknown actions permit extension.
fn actions(value: &str) -> (bool, bool) {
    // Report is opt-in while focusing is the protocol's default action.
    let (mut focus, mut report) = (true, false);
    // Negation affects only the named action; it does not erase other options.
    for action in value.split(',') {
        match action {
            // An explicit focus re-enables it after a preceding -focus in this list.
            "focus" => focus = true,
            "-focus" => focus = false,
            // Reporting writes a protocol reply only after user activation.
            "report" => report = true,
            "-report" => report = false,
            _ => {}
        }
    }
    (focus, report)
}
