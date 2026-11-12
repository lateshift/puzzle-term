//! Drop-data permission and bounded reply amplification for OSC 72.
//! A hover never grants data access. Reads are synchronous over immutable bytes,
//! so requests do not retain an unbounded queue or asynchronous OS resources.
//!
//! The protocol's x index is one-based into the most recent complete Drop MIME list.
//! A hover is not a drop, even if the source advertises the same MIME names.
//! Attempted pre-drop reads receive EPERM without ending the hover negotiation.
//! A valid read operates on bytes copied by the host during the user's gesture.
//! Nothing here reads the clipboard, opens a URI or follows a native filesystem path.
//!
//! The y/Y forms request remote URI or directory operations, outside this local subset.
//! They receive EINVAL and terminate the captured drop, rather than retaining OS work.
//! A zero index with no remote indices is the client's conclusion, not a data read.
//! Its operation is reported to the host, which can release gesture-specific resources.
//! Read amplification is bounded across the entire reply FIFO, not only each packet.
//! That matters when one parser slice contains many repeated reads of a large drop.
//! Base64 expansion and packet headers are charged before writing a single data byte.
//! A rejected read releases held bytes and reports cancellation through the same owner.

use super::{Metadata, operation, response};
use crate::{
    Terminal,
    dnd::{Effect, MAX_BYTES},
    terminal::TerminalEvent,
};
/// Resolve the protocol's MIME/conclusion/remote forms without ever opening files.
pub(super) fn request(term: &mut Terminal, meta: Metadata, bell: bool) {
    // X belongs to a remote reply handle; it does not select a local representation.
    let [index, uri, _, directory] = meta.coordinates;
    let client = term
        .host_protocols
        .dnd
        .state
        .as_ref()
        .map_or(0, |s| s.client);
    if index == 0 && uri == 0 && directory == 0 {
        if term
            .host_protocols
            .dnd
            .state
            .as_ref()
            .is_some_and(|s| s.items.is_some())
        {
            finish(term, operation(meta.operation));
        }
        return;
    }
    // Before a real Drop, EPERM is explicitly nonterminating (kitty security rule).
    let Some(items) = term
        .host_protocols
        .dnd
        .state
        .as_ref()
        .and_then(|s| s.items.as_ref())
    else {
        response::error(&mut term.replies, meta, client, "t=R", "EPERM", bell);
        return;
    };
    // Validate indices while borrowing immutable bytes, before appending any reply.
    let error = if uri != 0 || directory != 0 {
        Some("EINVAL")
    } else if index < 1 || index as u64 > items.len() as u64 {
        Some("ENOENT")
    } else {
        None
    };
    if let Some(error) = error {
        response::error(&mut term.replies, meta, client, "t=R", error, bell);
        // Error termination revokes further reads for this particular captured drop.
        finish(term, 0);
        return;
    }
    // Only a validated positive index reaches this subtraction and slice access.
    let bytes = &items[index as usize - 1].data;
    if term.reply_streaming() {
        // One retained immutable source replaces an eagerly expanded 64-MiB read.
        // Session stops here before accepting a subsequent read or ordinary text.
        let data = std::sync::Arc::clone(bytes);
        term.push_reply_source(Box::new(response::Data::new(index, client, data, bell)));
        return;
    }
    // Bound amplification within one parser slice even if hostile output repeats
    // a large read thousands of times before Session flushes its ordered reply FIFO.
    // Two 64-MiB wire allowances cover one maximum-sized encoded representation.
    let packets = bytes.len().div_ceil(3072);
    let required = bytes
        .len()
        .div_ceil(3)
        .saturating_mul(4)
        .saturating_add((packets + 1) * 128);
    if term.replies.len().saturating_add(required) > MAX_BYTES * 2 {
        response::error(&mut term.replies, meta, client, "t=R", "EMFILE", bell);
        // Error termination revokes further reads for this particular captured drop.
        finish(term, 0);
        return;
    }
    // Appending is synchronous; the next parser query follows these bytes in FIFO order.
    response::data(&mut term.replies, index, client, bytes, bell);
}
/// Errors and explicit completion discard held data and return a native lifecycle fact.
fn finish(term: &mut Terminal, operation: u32) {
    if let Some(state) = &mut term.host_protocols.dnd.state {
        state.clear();
    }
    term.events.push(TerminalEvent::Drop(Effect::Concluded {
        generation: term.host_protocols.dnd.generation,
        operation,
    }));
}
