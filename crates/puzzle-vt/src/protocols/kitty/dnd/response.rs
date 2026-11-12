//! OSC 72 packets never contain raw user bytes; binary drop data is base64.
//! The native subset has no file readers or pending asynchronous request queue.
//!
//! An OSC string cannot carry arbitrary binary data because ESC and BEL are delimiters.
//! Data replies use RFC 4648 Base64; MIME lists remain printable protocol text.
//! The 4096-byte limit applies to each payload, independently of its numeric header.
//! A 3072-byte raw chunk expands to one complete 4096-byte encoded chunk.
//! Stack scratch avoids allocating a temporary encoded String for every chunk.
//! A partial final group receives the required padding inside its own packet.
//!
//! The last nonempty packet has m=0, then an empty packet marks end-of-data.
//! Even an empty representation therefore produces an unambiguous EOF response.
//! Plain lists use the same continuation flag but need no separate EOF packet.
//! An absent client identifier is omitted rather than inventing a multiplexer route.
//! Errors carry validated numeric indices and a fixed error token only.
//! Caller-selected BEL/ST is preserved for request replies, as required by design §6.5.
//! Unsolicited movement/drop reports use ST because they have no triggering terminator.
//! All bytes append to one existing FIFO; encoding never writes to a transport itself.
//! No per-packet OS call or asynchronous completion can reorder a following DA reply.

use super::{MAX_PAYLOAD_BYTES, Metadata};
/// Immutable source bytes and scalar offsets survive stalled writer credit.
mod stream;
pub(super) use stream::Data;
/// Large MIME lists use the same bounded packet cursor as binary data.
pub(crate) fn emit(
    term: &mut crate::Terminal,
    header: &str,
    client: u32,
    payload: &[u8],
    bell: bool,
) {
    // I/O-owning Sessions transfer a cursor at this exact parser boundary.
    // Standalone VT callers keep their original eager Vec reply contract.
    if term.reply_streaming() {
        term.push_reply_source(Box::new(stream::List::new(header, client, payload, bell)));
    } else {
        encode(&mut term.replies, header, client, payload, bell);
    }
}
/// RFC 4648 expands three raw bytes to four encoded bytes under OSC 72's cap.
/// Derive the binary chunk from the shared wire limit, including any padding.
/// Reference: <https://sw.kovidgoyal.net/kitty/dnd-protocol/> (binary payloads).
const MAX_RAW_CHUNK_BYTES: usize = MAX_PAYLOAD_BYTES / 4 * 3;
/// Emit one <=4096-byte payload with the registered multiplexer identity.
pub(crate) fn encode(out: &mut Vec<u8>, header: &str, client: u32, payload: &[u8], bell: bool) {
    // Plain MIME lists may cross a packet boundary just like binary replies.
    if payload.is_empty() {
        packet(out, header, client, payload, bell);
        return;
    }
    let chunks = payload.len().div_ceil(MAX_PAYLOAD_BYTES);
    for (index, chunk) in payload.chunks(MAX_PAYLOAD_BYTES).enumerate() {
        let more = u8::from(index + 1 < chunks);
        packet(out, &format!("{header}:m={more}"), client, chunk, bell);
    }
}
/// Error indices echo only validated numeric fields, never arbitrary program text.
pub(super) fn error(
    out: &mut Vec<u8>,
    meta: Metadata,
    client: u32,
    kind: &str,
    error: &str,
    bell: bool,
) {
    let [x, y, _, dir] = meta.coordinates;
    let header = format!("{kind}:x={x}:y={y}:Y={dir}");
    encode(out, &header, client, error.as_bytes(), bell);
}
/// A raw 3072-byte span expands to exactly the allowed 4096 encoded bytes.
pub(super) fn data(out: &mut Vec<u8>, index: i64, client: u32, bytes: &[u8], bell: bool) {
    // The same per-packet encoder serves the lazy cursor and this eager API.
    let chunks = bytes.len().div_ceil(MAX_RAW_CHUNK_BYTES);
    for (number, source) in bytes.chunks(MAX_RAW_CHUNK_BYTES).enumerate() {
        data_packet(out, index, client, source, number + 1 < chunks, bell);
    }
    // The protocol uses a separate empty EOF, including for an empty MIME value.
    packet(out, &format!("t=r:x={index}:m=0"), client, b"", bell);
}

/// One packet preserves the validated header, numeric identity and terminator.
fn packet(out: &mut Vec<u8>, header: &str, client: u32, payload: &[u8], bell: bool) {
    out.extend_from_slice(b"\x1b]72;");
    // Headers contain only protocol keys and sanitized numeric identities.
    out.extend_from_slice(header.as_bytes());
    if client != 0 {
        out.extend_from_slice(format!(":i={client}").as_bytes());
    }
    out.push(b';');
    out.extend_from_slice(payload);
    out.extend_from_slice(if bell { b"\x07" } else { b"\x1b\\" });
}

/// One binary protocol packet uses stack scratch and exact RFC 4648 padding.
fn data_packet(out: &mut Vec<u8>, index: i64, client: u32, source: &[u8], more: bool, bell: bool) {
    /// Standard alphabet prevents binary bytes from introducing OSC controls.
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = [0u8; MAX_PAYLOAD_BYTES];
    let mut used = 0;
    for group in source.chunks(3) {
        // Complete triples produce four digits; absent tail bytes contribute zero
        // bits and are represented by padding rather than uninitialized scratch.
        let a = group[0];
        let b = group.get(1).copied().unwrap_or(0);
        let c = group.get(2).copied().unwrap_or(0);
        encoded[used] = ALPHABET[(a >> 2) as usize];
        encoded[used + 1] = ALPHABET[(((a & 3) << 4) | (b >> 4)) as usize];
        encoded[used + 2] = if group.len() > 1 {
            ALPHABET[(((b & 15) << 2) | (c >> 6)) as usize]
        } else {
            b'='
        };
        encoded[used + 3] = if group.len() > 2 {
            ALPHABET[(c & 63) as usize]
        } else {
            b'='
        };
        used += 4;
    }
    // Bound the slice to written digits: unused stack bytes never reach the wire.
    packet(
        out,
        &format!("t=r:x={index}:m={}", u8::from(more)),
        client,
        &encoded[..used],
        bell,
    );
}

#[cfg(test)]
/// Eager and credit-driven paths produce the identical protocol transcript.
mod tests;
