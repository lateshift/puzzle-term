//! Kitty clipboard replies always use ST and echo only the sanitized start ID.
//! Read bytes are split before base64 encoding, never between MIME alternatives.
//! Reply assembly is synchronous and appends to the owner's ordered byte queue.
//! Native completion decides when assembly runs; this module never assumes that
//! posting an OS request means the clipboard transaction has succeeded.
//! A successful write has one DONE packet. A successful read has OK, zero or more
//! DATA packets, then DONE, keeping all chunks of one MIME together.
//! Each DATA payload has at most 4096 decoded bytes, including the final chunk.
//! Empty representations still need a DATA packet to distinguish present/empty
//! from unavailable data, which is represented by an error or absent alternative.
//! Read errors have a smaller status vocabulary than write errors. Map internal
//! I/O/size failures to EBUSY rather than inventing an unsupported wire status.
//! Only sanitized IDs and closed status names enter headers as literal strings.
//! MIME labels and binary payloads are always base64 encoded.
//! An unsolicited paste event has no multiplexer ID and includes a one-use
//! credential only in its opening packet; subsequent DATA packets list MIME names.
//! The password reaches the byte queue, never a diagnostic formatter or host event.
//! Read data may contain any bytes, including zeroes and invalid UTF-8.
//! Only metadata names are text; base64 isolates both from terminal control syntax.
//! A write error emits one packet rather than the read OK/DATA/DONE envelope.
//! The caller appends complete response groups to its ordinary reply buffer.
//! That keeps asynchronous completion ordered with subsequent owner replies.
//! Primary location is included on the unsolicited event to scope redemption.
//! The Session, rather than this encoder, owns grant invalidation and permission.
use crate::clipboard::{MimeData, Operation, Request, Status};
/// OSC 5522 read replies split MIME data into at most 4096 decoded bytes.
/// Reference: <https://sw.kovidgoyal.net/kitty/clipboard/> (reading clipboard data).
const MAX_READ_CHUNK_BYTES: usize = 4096;
/// Incremental ownership avoids expanding a 64-MiB read into one giant byte vector.
mod stream;
pub use stream::Response;

/// Frames one terminal outcome without invoking transport or native clipboard I/O.
pub fn respond(request: &Request, status: Status, items: &[MimeData], out: &mut Vec<u8>) {
    let mut cursor = stream::Cursor::default();
    while cursor.append_next(request, status, items, out) {}
}
/// Both operation names and statuses are trusted closed values, never wire strings.
fn header(request: &Request, status: &str, out: &mut Vec<u8>) {
    out.extend_from_slice(if matches!(request.operation, Operation::Read(_)) {
        b"\x1b]5522;type=read:status="
    } else {
        b"\x1b]5522;type=write:status="
    });
    out.extend_from_slice(status.as_bytes());
    if !request.id.is_empty() {
        out.extend_from_slice(b":id=");
        out.extend_from_slice(request.id.as_bytes());
    }
}
/// One binary chunk gets its own required padding and explicit MIME identity.
fn packet(request: &Request, item: &MimeData, data: &[u8], out: &mut Vec<u8>) {
    header(request, "DATA", out);
    out.extend_from_slice(b":mime=");
    super::base64::encode(item.mime.as_bytes(), out);
    out.push(b';');
    super::base64::encode(data, out);
    out.extend_from_slice(b"\x1b\\");
}
/// Native unsupported operations never masquerade as a successful clipboard update.
fn status_name(status: Status) -> &'static str {
    match status {
        Status::Done => "DONE",
        Status::Unavailable => "ENOSYS",
        Status::Denied => "EPERM",
        Status::Busy => "EBUSY",
        Status::Io => "EIO",
        Status::Invalid => "EINVAL",
        Status::TooLarge => "EFBIG",
    }
}

/// Announces a gesture without reading its contents or granting another location.
pub fn paste_event(
    request: &Request,
    secret: &crate::clipboard::Secret,
    items: &[MimeData],
    out: &mut Vec<u8>,
) {
    header(request, "OK", out);
    if request.primary {
        out.extend_from_slice(b":loc=primary");
    }
    out.extend_from_slice(b":pw=");
    super::base64::encode(secret.0.as_bytes(), out);
    out.extend_from_slice(b"\x1b\\");
    for item in items {
        packet(request, item, &item.data, out);
    }
    header(request, "DONE", out);
    out.extend_from_slice(b"\x1b\\");
}
