//! Kitty OSC 72 registration, MIME negotiation and drop reads (M18.5, D-58).
//! Reference: <https://sw.kovidgoyal.net/kitty/dnd-protocol/>.
//! Like the reference terminal, native data is captured eagerly, clients are treated as local,
//! and drag-out/remote file reads receive errors. No OS or file I/O enters VT.
//! Queries write into the parser reply FIFO before a following DA request.
//! State and chunk storage exist only after registration; ordinary grid output
//! adds no scans, caches or frame callbacks. RIS and child exit unregister it.
//!
//! There are two independent identities: a native registration generation and i=.
//! The generation prevents late host messages from reaching a replacement target.
//! The protocol client ID routes replies through multiplexers, and can be omitted.
//! Neither identity grants clipboard access or access to arbitrary filesystem paths.
//!
//! The parser has already bounded the raw OSC packet before this dispatcher runs.
//! The additional 4096-byte check is the protocol's payload limit, excluding metadata.
//! Larger MIME lists are assembled through m=1 packets and a final m=0 packet.
//! Only the first packet supplies command metadata; continuations add payload bytes.
//! A query may interrupt that assembly without consuming its partial transaction.
//! An explicit conflicting command aborts rather than changing the first metadata.
//! The aggregate cap prevents indefinitely unfinished registration/status payloads.
//! A failed assembly discards its bytes before replying with a fixed POSIX error.
//!
//! Registration is local even if a program supplies a remote machine identifier.
//! The reference terminal likewise accepts that identifier without implementing remote file transfer.
//! The supported eager-drop path uses a small curated list of native representations.
//! No support response claims filesystem edits, drag-out or directory enumeration.
//! Unsupported action requests receive errors rather than fabricating successful I/O.
//! Response-only types are never interpreted as host actions from untrusted output.
//!
//! Host effects are typed values; the Session forwards them through command policy.
//! Registration and acceptance reach the host asynchronously through its event queue.
//! The session thread never waits for an OS drag callback or an application response.
//! The native host is responsible for preserving its OS acceptance state until Drop.
//! The pure model knows only the immutable bytes explicitly supplied at that point.
//! Read replies share Terminal.replies with DA, CPR and other terminal responses.
//! A later request therefore cannot overtake data from an earlier completed request.
//!
//! MIME parsing accepts spaces only as separators, not as part of a MIME name.
//! Keeping the original ordered list preserves the index order visible to the host.
//! Unknown or malformed numeric metadata is never echoed as arbitrary terminal text.
//! The response encoder receives validated numbers and fixed error-name strings only.
//! Invalid commands end a held drag and notify the host so acceptance cannot stick.
//! The registration itself remains until A, RIS, child exit or a replacement a.
//! Ordinary output never calls this dispatcher and allocates no drag state.

use crate::{
    Terminal,
    dnd::{Effect, MAX_MIME_BYTES, State},
    terminal::TerminalEvent,
};
/// OSC 72 limits each payload to 4096 bytes after any Base64 encoding.
/// Shared by incoming validation and outgoing packets; metadata is excluded.
/// Reference: <https://sw.kovidgoyal.net/kitty/dnd-protocol/> (chunking).
const MAX_PAYLOAD_BYTES: usize = 4096;

/// Strict numeric grammar is separate from lifecycle and response encoding.
mod metadata;
pub(crate) use metadata::Metadata;
/// Chunked data/error encoding keeps every payload within the protocol's 4096 bytes.
mod response;
pub(crate) use response::emit as encode;
/// Read replies hold no platform handles and never open dropped file paths.
mod read;
/// A packet cannot change native registration until its complete list is valid.
pub(crate) fn dispatch(term: &mut Terminal, fields: &[&[u8]], bell: bool) {
    let Some(meta) = fields.get(1).and_then(|b| Metadata::parse(b)) else {
        return;
    };
    // Semicolons in payloads would change the OSC field structure, so reject them.
    if fields.len() > 3 || term.clipboard_wire_invalid() {
        invalid(term, meta, "EINVAL", bell);
        return;
    }
    // Queries neither consume nor complete an outstanding chunk chain.
    if meta.kind == b'q' {
        encode(term, "t=q", meta.client, b"", bell);
        return;
    }
    // A missing payload is meaningful for register-all, unregister and queries.
    let payload = fields.get(2).copied().unwrap_or_default();
    if payload.len() > MAX_PAYLOAD_BYTES {
        invalid(term, meta, "EFBIG", bell);
        return;
    }
    let Some((meta, payload)) = assemble(term, meta, payload, bell) else {
        return;
    };
    // Interpretation sees only a complete bounded transaction, never a partial list.
    command(term, meta, &payload, bell);
}
/// First-chunk metadata is authoritative, with at most one bounded assembly.
fn assemble(
    term: &mut Terminal,
    meta: Metadata,
    payload: &[u8],
    bell: bool,
) -> Option<(Metadata, Vec<u8>)> {
    // Removing the partial chain makes every error release its previous allocation.
    let partial = term
        .host_protocols
        .dnd
        .state
        .as_mut()
        .and_then(|s| s.partial.take());
    if let Some((first, mut bytes)) = partial {
        // Explicitly conflicting commands are errors; omitted t inherits the first.
        if meta.explicit && meta.kind != first.kind {
            invalid(term, first, "EINVAL", bell);
            return None;
        }
        if bytes.len().saturating_add(payload.len()) > MAX_MIME_BYTES {
            invalid(term, first, "EFBIG", bell);
            return None;
        }
        // Aggregate size was checked before growing the sole assembly buffer.
        bytes.extend_from_slice(payload);
        if meta.more {
            if let Some(s) = &mut term.host_protocols.dnd.state {
                s.partial = Some((first, bytes));
            }
            return None;
        }
        return Some((first, bytes));
    }
    // Only target registration/status needs a long incoming payload in this subset.
    if meta.more
        && (meta.kind == b'a' && meta.coordinates[0] == 0
            || meta.kind == b'm' && term.host_protocols.dnd.state.is_some())
    {
        let state = term
            .host_protocols
            .dnd
            .state
            .get_or_insert_with(|| Box::new(State::default()));
        state.partial = Some((meta, payload.to_vec()));
        return None;
    }
    Some((meta, payload.to_vec()))
}
/// All native intents become typed facts; Session applies ordinary command policy.
fn command(term: &mut Terminal, meta: Metadata, payload: &[u8], bell: bool) {
    match meta.kind {
        b'a' if meta.coordinates[0] == 1 => {} // Remote machine IDs are accepted, never disclosed.
        b'a' => {
            let Some(mimes) = mime_list(payload) else {
                invalid(term, meta, "EINVAL", bell);
                return;
            };
            // Every registration replaces the old lifetime, including pending drop bytes.
            let Some(generation) = term.host_protocols.dnd.generation.checked_add(1) else {
                invalid(term, meta, "EMFILE", bell);
                return;
            };
            term.host_protocols.dnd.generation = generation;
            term.host_protocols.dnd.state = Some(Box::new(State::registered(meta.client)));
            term.events.push(TerminalEvent::Drop(Effect::Registration {
                generation: term.host_protocols.dnd.generation,
                mimes: Some(mimes),
            }));
        }
        b'A' => term.clear_drop_target(),
        // Feedback is an intent for the native host, not permission to read native data.
        b'm' => {
            let Some(mimes) = mime_list(payload) else {
                invalid(term, meta, "EINVAL", bell);
                return;
            };
            if term
                .host_protocols
                .dnd
                .state
                .as_ref()
                .is_some_and(|s| s.registered)
            {
                term.events.push(TerminalEvent::Drop(Effect::Acceptance {
                    generation: term.host_protocols.dnd.generation,
                    operation: operation(meta.operation),
                    mimes,
                }));
            }
        }
        b'r' => read::request(term, meta, bell),
        b'o' if matches!(meta.coordinates[0], 1 | 2) => {}
        b'o' | b'p' | b'P' => {
            response::error(&mut term.replies, meta, meta.client, "t=E", "EPERM", bell)
        }
        b'e' | b'E' => response::error(&mut term.replies, meta, meta.client, "t=E", "EINVAL", bell),
        // Response-only and unsupported future directions have no effect/capability.
        _ => {}
    }
}
/// Copy and move are the only accepted operations, every other value rejects.
pub(crate) fn operation(value: u32) -> u32 {
    if matches!(value, 1 | 2) { value } else { 0 }
}
/// Invalid packet aborts only its pending target transaction and never consumes output.
fn invalid(term: &mut Terminal, meta: Metadata, code: &str, bell: bool) {
    if let Some(state) = &mut term.host_protocols.dnd.state {
        state.partial = None;
        state.clear();
        if state.registered {
            term.events.push(TerminalEvent::Drop(Effect::Concluded {
                generation: term.host_protocols.dnd.generation,
                operation: 0,
            }));
        }
    }
    let client = term
        .host_protocols
        .dnd
        .state
        .as_ref()
        .map_or(meta.client, |s| s.client);
    response::error(&mut term.replies, meta, client, "t=R", code, bell);
}
/// MIME names may contain neither protocol delimiters nor whitespace/controls.
pub(crate) fn valid_mime(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 256
        && text.contains('/')
        && text
            .bytes()
            .all(|b| b.is_ascii_graphic() && !b";:".contains(&b))
}
/// The bounded plain MIME list preserves order for host registration and preferences.
fn mime_list(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    if text
        .split(' ')
        .filter(|s| !s.is_empty())
        .any(|s| !valid_mime(s))
    {
        return None;
    }
    // Preserve preferences in wire order rather than sorting native type names.
    Some(text.to_owned())
}
#[cfg(test)]
mod tests;
