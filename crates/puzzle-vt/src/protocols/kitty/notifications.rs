//! Kitty OSC 99 chunk assembly and query grammar (M18.4, D-58).
//! Reference: <https://sw.kovidgoyal.net/kitty/desktop-notifications/>.
//! Partial title/body streams are bounded and allocated only after d=0. Unknown
//! payloads are ignored, as the protocol requires, and never advertised as supported.
//! Queries write directly to VT's reply FIFO, before a following DA request.
//!
//! The parser deliberately has no host clock: rate limiting and occasions belong
//! to Session after assembly finishes. Incomplete IDs hold separate title/body
//! streams, since a sender can alternate those payload types before d=1.
//! Packet encoding and d/p defaults apply independently to each packet, while
//! notification actions, urgency and occasion persist until explicitly replaced.
//! Removing a partial before appending makes any malformed continuation discard
//! that transaction rather than expose partly decoded text to a host.
//! A capability query never consumes or completes another transaction.
//! The finite table evicts abandoned partials by arrival order, independently
//! of the admitted-notification table managed by the Session security policy.
use crate::{
    notification::{MAX_NOTIFICATIONS, MAX_TEXT, Notification, Request},
    terminal::{Terminal, TerminalEvent},
};
/// OSC 99 permits 2048 plain UTF-8 bytes per payload before encoding.
/// Reference: <https://sw.kovidgoyal.net/kitty/desktop-notifications/> (chunking).
const MAX_PLAIN_PAYLOAD_BYTES: usize = 2048;
/// OSC 99 separately permits 4096 encoded bytes per Base64 payload chunk.
/// Reference: <https://sw.kovidgoyal.net/kitty/desktop-notifications/> (encoding).
const MAX_ENCODED_PAYLOAD_BYTES: usize = 4096;

/// Per-packet metadata, applied only where explicitly specified.
mod metadata;
/// Base64 supports both independently encoded chunks and split encoded streams.
mod text;
use metadata::Metadata;

/// Incomplete messages exist only after explicit chunking, never for ordinary output.
#[derive(Default)]
pub(crate) struct Assembly {
    /// Insertion order gives bounded oldest-first eviction of abandoned transactions.
    entries: Vec<Partial>,
}
/// One identifier has independent title/body encoding tails.
#[derive(Default)]
struct Partial {
    /// Metadata survives omitted keys on subsequent chunks.
    notification: Notification,
    /// UTF-8 and base64 boundaries need not coincide with OSC packets.
    title: text::Text,
    /// Switching payload type does not discard the other stream's tail.
    body: text::Text,
}
/// Parses one complete OSC without interpreting host policy or blocking on native work.
pub(crate) fn dispatch(term: &mut Terminal, params: &[&[u8]], truncated: bool) {
    // Metadata must be UTF-8 even when the payload is base64.
    let Some(raw) = params.get(1).and_then(|b| std::str::from_utf8(b).ok()) else {
        tracing::debug!(event = "notification.invalid", reason = "metadata UTF-8");
        return;
    };
    // Validate reply identifiers before any query can echo them.
    let Some(meta) = Metadata::parse(raw) else {
        // Do not log untrusted text or identifiers from a rejected packet.
        tracing::debug!(event = "notification.invalid", reason = "metadata grammar");
        return;
    };
    // Queries and close requests are independent of a partially assembled title.
    // Advertise only implemented payloads/actions; icon and button data stay unsupported.
    if meta.payload == "?" {
        reply(
            term,
            meta.id.as_deref().unwrap_or("0"),
            "?",
            "a=focus,report:c=1:o=always,unfocused,invisible:p=title,body,close,?:u=0,1,2",
        );
        return;
    }
    // A close cancels pending assembly and separately asks Session to remove a live item.
    if meta.payload == "close" {
        // Missing IDs cannot accidentally close every anonymous notification.
        if let Some(id) = meta.id {
            // A later continuation must not resurrect the canceled partial.
            if let Some(state) = &mut term.host_protocols.notifications {
                state
                    .entries
                    .retain(|p| p.notification.id.as_ref() != Some(&id));
            }
            term.events
                .push(TerminalEvent::DesktopNotification(Request::Close(id)));
        }
        return;
    }
    // Unimplemented icon/buttons/alive have no host action and are absent from the query.
    if !matches!(meta.payload.as_str(), "title" | "body") {
        return;
    }
    // Take the owner temporarily; emitting a VT event does not borrow the assembly table.
    assemble(term, params, truncated, meta);
}
/// Merge a bounded text packet after queries, close and unsupported payloads were handled.
fn assemble(term: &mut Terminal, params: &[&[u8]], truncated: bool, meta: Metadata) {
    let mut state = term.host_protocols.notifications.take().unwrap_or_default();
    // Only equal optional IDs belong to the same incomplete transaction.
    let index = state
        .entries
        .iter()
        .position(|p| p.notification.id == meta.id);
    let mut partial = index.map(|i| state.entries.remove(i)).unwrap_or_default();
    // Omitted persistent options inherit; d/e/p remain packet-local.
    meta.apply(&mut partial.notification);
    // Count structural separators too when called with already split fields.
    let bytes =
        params.iter().skip(2).map(|p| p.len()).sum::<usize>() + params.len().saturating_sub(3);
    // These are protocol packet limits, independent of the assembled field cap.
    let limit = if meta.encoded {
        MAX_ENCODED_PAYLOAD_BYTES
    } else {
        MAX_PLAIN_PAYLOAD_BYTES
    };
    // Reject oversized chunks as a transaction; following printable output remains untouched.
    let ok =
        !truncated && params.len() >= 3 && bytes <= limit && append(&mut partial, &meta, params);
    // Only a completed transaction may produce a visible host request.
    if !ok {
        // Rejection discards the whole partial, never a shortened host notification.
        tracing::debug!(event = "notification.invalid", reason = "payload or limit");
    }
    if ok && meta.done {
        finish(term, partial);
    } else if ok {
        // Bound hostile unfinished identifiers without growing the table indefinitely.
        if state.entries.len() == MAX_NOTIFICATIONS {
            state.entries.remove(0);
        }
        state.entries.push(partial);
    }
    // Reuse the allocated table after first use; RIS frees its contents.
    term.host_protocols.notifications = Some(state);
}
/// Append exact semicolons instead of silently truncating a notification body.
fn append(partial: &mut Partial, meta: &Metadata, params: &[&[u8]]) -> bool {
    // Each payload has its own UTF-8 bytes and incomplete encoded quartet.
    let text = if meta.payload == "body" {
        &mut partial.body
    } else {
        &mut partial.title
    };
    // Rejoining applies only to payload text, never to metadata delimiters.
    for (i, bytes) in params.iter().skip(2).enumerate() {
        if i != 0 && !text.feed(b";", meta.encoded) {
            return false;
        }
        if !text.feed(bytes, meta.encoded) {
            return false;
        }
    }
    true
}
/// Validate UTF-8 only after every encoded tail has arrived, then strip controls.
fn finish(term: &mut Terminal, mut partial: Partial) {
    // An incomplete or noncanonical encoded tail invalidates the whole notification.
    let Some(title) = partial.title.finish() else {
        tracing::debug!(event = "notification.invalid", reason = "title encoding");
        return;
    };
    // Body validity matters even if title alone would be displayable.
    let Some(body) = partial.body.finish() else {
        tracing::debug!(event = "notification.invalid", reason = "body encoding");
        return;
    };
    // Strip decoded controls as well as raw controls before host delivery.
    partial.notification.title = super::super::strings::clean(title, MAX_TEXT);
    partial.notification.body = super::super::strings::clean(body, MAX_TEXT);
    // Kitty promotes the body when no title was supplied.
    if partial.notification.title.is_empty() {
        std::mem::swap(
            &mut partial.notification.title,
            &mut partial.notification.body,
        );
    }
    // An empty title and body has no native effect or policy admission.
    if !partial.notification.title.is_empty() {
        term.events
            .push(TerminalEvent::DesktopNotification(Request::Post(
                partial.notification,
            )));
    }
}
/// IDs have already passed the identifier grammar; replies cannot inject input.
fn reply(term: &mut Terminal, id: &str, payload: &str, body: &str) {
    term.replies
        .extend_from_slice(format!("\x1b]99;i={id}:p={payload};{body}\x1b\\").as_bytes());
}
#[cfg(test)]
mod tests;
