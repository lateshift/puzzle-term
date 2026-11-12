//! Kitty OSC 5522 wire assembly; shared clipboard values live outside this family.
//! Reference: <https://sw.kovidgoyal.net/kitty/clipboard/>.
//! No native I/O occurs here. Complete transactions enter Session policy as events.
//! A write has three phases: explicit start, binary accumulation, and commit.
//! Only commit produces a host request. Native clipboard state cannot be changed
//! by an incomplete packet sequence or by ordinary text between packets.
//! The Option on Terminal owns this transaction. Taking it while processing a
//! packet makes an error drop all accumulated representations automatically.
//! After that drop, later wdata/walias packets have no transaction to mutate.
//! A new type=write is the sole way to start accumulating again.
//! The start packet owns the reply ID and location. Continuation packet metadata
//! cannot redirect an acknowledgement to another multiplexer client.
//! A MIME stream may span arbitrarily split packets, including base64 groups.
//! Changing MIME closes the previous stream; missing final padding aborts it.
//! An empty wdata packet commits, and an empty transaction means clipboard clear.
//! Aliases are resolved at commit so they may arrive before their target data.
//! Their Arc references share bytes rather than duplicating the binary payload.
//! Repeated mapping names take the last assignment, following kitty's map model.
//! Read requests need no retained assembly, but their encoded MIME list is bounded.
//! Malformed reads have no defined error packet and are ignored by wire policy.
//! Write errors have a defined status and use the original start packet identity.
//! Limits apply to decoded bytes and metadata counts separately; base64 overhead
//! cannot turn a small wire string into an unbounded collection of alternatives.
//! This module owns no read permission. Session applies that policy after parsing.
//! In particular a syntactically valid password is merely data at this layer.
//! The shared Request type lets native and web hosts use identical wire semantics.
//! No timestamp, OS handle, font or render state enters protocol assembly.
//! Ordinary glyph output retains its original parser order between these packets.
/// Shared strict encoding for metadata, binary streams and outgoing chunks.
mod base64;
/// Metadata validation and sanitization is separate from binary stream assembly.
mod metadata;
/// Ordered protocol response framing, shared with asynchronous host completion.
mod reply;
use crate::{
    clipboard::{MAX_TYPES, MimeData, Operation, Request, Status, Write},
    terminal::{Terminal, TerminalEvent},
};
/// Replies are public through shared clipboard types, not through origin internals.
pub use reply::{Response, paste_event, respond};
/// Handles one packet; any write error aborts until a new explicit start.
/// `params` are vte fields; `truncated` also covers raw stripped controls.
/// A missing active write makes continuation packets inert.
pub(crate) fn dispatch(term: &mut Terminal, params: &[&[u8]], truncated: bool) {
    // vte splits OSC fields on semicolons; metadata is the second field.
    let raw = params.get(1).copied().unwrap_or_default();
    let Some(meta) = metadata::parse(raw) else {
        // Corrupt write metadata cannot silently leave a partial transaction alive.
        if raw.windows(10).any(|v| v == b"type=wdata")
            || raw.windows(11).any(|v| v == b"type=walias")
        {
            abort(term, Status::Invalid);
        }
        return;
    };
    // Reject a raw invalid/truncated packet even if its visible base64 looks valid.
    if truncated {
        if meta.kind == b"read" {
            return;
        }
        abort(term, Status::Invalid);
        return;
    }
    let payload = params.get(2).copied().unwrap_or_default();
    // Base64 cannot contain semicolons; extra fields are malformed, not concatenated.
    if params.len() > 3 {
        if meta.kind != b"read" {
            abort(term, Status::Invalid);
        }
        return;
    }
    match meta.kind {
        // Starting again replaces the entire uncommitted write, not individual MIME data.
        b"write" => {
            term.host_protocols.clipboard_write = Some(Box::new(Write {
                id: meta.id,
                primary: meta.primary,
                ..Default::default()
            }))
        }
        b"read" => read(term, meta, payload),
        b"wdata" | b"walias" => continue_write(term, &meta, payload),
        _ => {}
    }
}
/// Applies a continuation to the owned transaction, dropping it on any error.
/// Native policy sees only a successful commit; partial bytes never escape.
fn continue_write(term: &mut Terminal, meta: &metadata::Metadata<'_>, payload: &[u8]) {
    // Taking ownership makes every error an atomic transaction abort.
    let Some(mut write) = term.host_protocols.clipboard_write.take() else {
        return;
    };
    // Alias packets do not close the current binary MIME stream.
    let result = if meta.kind == b"walias" {
        alias(&mut write, &meta.mime, payload)
    } else {
        data(&mut write, &meta.mime, payload)
    };
    match result {
        Err(status) => respond(
            &Request {
                id: write.id,
                primary: write.primary,
                operation: Operation::Write(Vec::new()),
                password: None,
            },
            status,
            &[],
            &mut term.replies,
        ),
        // Completion alone publishes an immutable native request.
        Ok(true) => commit(term, *write),
        Ok(false) => term.host_protocols.clipboard_write = Some(write),
    }
}
/// A malformed read has no error reply according to kitty's wire grammar.
/// Valid bounded names become one request event; no host content is read here.
fn read(term: &mut Terminal, meta: metadata::Metadata<'_>, payload: &[u8]) {
    let Some(text) = base64::decode(payload, MAX_TYPES * 257) else {
        return;
    };
    // Listing requests use the same bounded representation names as ordinary reads.
    let names: Vec<_> = text.split_ascii_whitespace().map(str::to_owned).collect();
    if names.len() > MAX_TYPES || names.iter().any(|s| !metadata::valid_mime(s)) {
        return;
    }
    // Policy and OS access happen after this parser turn, on the Session owner.
    term.events.push(TerminalEvent::ClipboardRequest(Request {
        id: meta.id,
        primary: meta.primary,
        operation: Operation::Read(names),
        password: meta.password,
    }));
}
/// Ends the current MIME only after its continuous base64 stream has terminated.
/// Returns Invalid for an incomplete group and TooLarge for excessive types.
/// Moving buffers into Arc storage avoids a second complete image copy.
fn finish(write: &mut Write) -> Result<(), Status> {
    // One to three remaining symbols cannot form a correctly padded stream.
    if !write.tail.is_empty() {
        return Err(Status::Invalid);
    }
    if write.mime.is_empty() {
        return Ok(());
    }
    // Move decoded bytes once into shared storage, retaining no duplicate buffer.
    let item = MimeData {
        mime: std::mem::take(&mut write.mime),
        data: std::mem::take(&mut write.bytes).into(),
    };
    // A later region for the same MIME replaces its earlier mapping (reference parity).
    write.items.retain(|old| old.mime != item.mime);
    if write.items.len() == MAX_TYPES {
        return Err(Status::TooLarge);
    }
    write.items.push(item);
    write.padded = false;
    Ok(())
}
/// Empty MIME commits; changing MIME finalizes exactly one stream, never a packet.
/// The bool reports commit, not whether data happened to arrive in this packet.
/// Decoder errors discard the outer transaction rather than its latest chunk.
fn data(write: &mut Write, mime: &str, payload: &[u8]) -> Result<bool, Status> {
    // The commit marker cannot also carry data for an unspecified representation.
    if mime.is_empty() {
        if !payload.is_empty() {
            return Err(Status::Invalid);
        }
        finish(write)?;
        return Ok(true);
    }
    // A change of type is the only boundary that finalizes continuous base64.
    if write.mime != mime {
        finish(write)?;
        write.mime = mime.into();
    }
    base64::feed(write, payload)?;
    Ok(false)
}
/// Aliases share immutable bytes and may precede their target's actual data.
/// Target and alias names obey the same metadata restrictions as MIME data.
/// Alias storage is bounded even when no binary data has arrived yet.
fn alias(write: &mut Write, mime: &str, payload: &[u8]) -> Result<bool, Status> {
    if mime.is_empty() {
        return Err(Status::Invalid);
    }
    // Alias metadata has its own bound, independent of the decoded image allowance.
    let names = base64::decode(payload, MAX_TYPES * 257).ok_or(Status::Invalid)?;
    for name in names.split_ascii_whitespace() {
        if !metadata::valid_mime(name) {
            return Err(Status::Invalid);
        }
        if write.aliases.len() == MAX_TYPES {
            return Err(Status::TooLarge);
        }
        // Defer resolution until all target representations have arrived.
        write.aliases.push((mime.into(), name.into()));
    }
    Ok(false)
}
/// Resolves aliases once; no partial clipboard update escapes a failed transaction.
/// The final combined mapping count includes aliases and explicit alternatives.
/// An overfull mapping replies EFBIG instead of publishing partial contents.
fn commit(term: &mut Terminal, mut write: Write) {
    // Resolve against the evolving map so aliases can refer to preceding aliases.
    for (target, name) in write.aliases {
        // Missing targets add nothing; existing targets share their allocation.
        if let Some(item) = write.items.iter().find(|v| v.mime == target).cloned() {
            // Alias assignment replaces an existing mapping in arrival order.
            write.items.retain(|old| old.mime != name);
            if write.items.len() == MAX_TYPES {
                respond(
                    &Request {
                        id: write.id,
                        primary: write.primary,
                        operation: Operation::Write(Vec::new()),
                        password: None,
                    },
                    Status::TooLarge,
                    &[],
                    &mut term.replies,
                );
                return;
            }
            write.items.push(MimeData {
                mime: name,
                data: item.data,
            });
        }
    }
    term.events.push(TerminalEvent::ClipboardRequest(Request {
        id: write.id,
        primary: write.primary,
        operation: Operation::Write(write.items),
        password: None,
    }));
}
/// Discards the complete partial write and emits exactly one error for its start ID.
/// With no write in progress there is no identity to acknowledge.
fn abort(term: &mut Terminal, status: Status) {
    // Removing the Option suppresses duplicate errors from subsequent continuation packets.
    if let Some(write) = term.host_protocols.clipboard_write.take() {
        respond(
            &Request {
                id: write.id,
                primary: write.primary,
                operation: Operation::Write(Vec::new()),
                password: None,
            },
            status,
            &[],
            &mut term.replies,
        );
    }
}

#[cfg(test)]
/// Full parser fixtures exercise stream boundaries and bounded binary ownership.
mod tests;
