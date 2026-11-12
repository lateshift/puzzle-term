//! MIME clipboard callbacks and asynchronous host answers (M18.3, D-50).
//!
//! A callback is a request, never an acknowledgement. The host echoes its token
//! after its OS transaction finishes; Session puts the protocol reply on its
//! ordinary ordered writer. Binary buffers are copied only during this call.
//! Both entry points copy foreign ranges while the caller still owns them.
//! Only owned strings/Arc bytes reach Session messages; no C pointer survives.
//! The shared array helper rejects null nonempty, misaligned and overflowing ranges.
//! Readable storage and initialized elements remain the caller's C safety contract.
//! Counts and lengths are checked before traversing or copying a foreign payload.
//! Status integers are explicitly matched so unknown values never become Rust enums.
//! A generation sentinel distinguishes ordinary operations from gesture reads.
//! UINT64_MAX cannot be used as a trusted paste generation.
//! Callback correlation is decimal text, preserving u64 precision in JSON bridges.
//! The callback contains the operation, MIME alternatives and expected generation.
//! It deliberately omits the protocol password and multiplexer ID.
//! Hosts return completion only after native work succeeds or fails.
//! The completion method posts a message and never waits for Session or a drawable.
//! The paste method is reserved for a user gesture and calls embed's secure generator.
//! Host code needs revision tracking, but never implements token generation itself.
//! Both NSView and borrowed-layer hosts use this exact same C contract.
//! Primary selection maps consistently with existing OSC 52 behavior.
//! Clipboard replies are assembled in the core, not duplicated in the C bridge.
//! Token zero is an API write notification and never a valid completion target.
use crate::{
    abi::{self, ResultCode},
    memory, registry,
};
use puzzle_term::clipboard::{
    Answer, MAX_BYTES, MAX_TYPES, MimeData, Operation, Status, Transaction,
};
use serde_json::json;

/// Converts one owned host request to the existing callback's JSON value contract.
pub(crate) fn event(transaction: Transaction) -> super::events::OwnedEvent {
    // Decimal tokens remain exact in JavaScript-based host bridges as well as C.
    // Binary JSON arrays remain byte-exact; no UTF-8 conversion is applied to PNG.
    let (operation, items) = match transaction.request.operation {
        Operation::Read(names) => ("read", json!(names)),
        Operation::Write(items) => (
            "write",
            json!(
                items
                    .iter()
                    .map(|item| json!({"type": item.mime, "data": item.data.as_ref()}))
                    .collect::<Vec<_>>()
            ),
        ),
    };
    // The host maps primary to the standard clipboard, matching our OSC 52 path.
    let payload = json!({"token": transaction.token.to_string(), "operation": operation,
        "primary": transaction.request.primary, "revision": transaction.expected_revision.map(|v| v.to_string()), "items": items});
    (
        "clipboard/mime".into(),
        payload.to_string().into_bytes().into(),
    )
}
/// Validated numeric status; arbitrary C integers never become Rust enum values.
fn status(value: u32) -> Result<Status, ResultCode> {
    match value {
        0 => Ok(Status::Done),
        1 => Ok(Status::Unavailable),
        2 => Ok(Status::Denied),
        3 => Ok(Status::Busy),
        4 => Ok(Status::Io),
        5 => Ok(Status::Invalid),
        6 => Ok(Status::TooLarge),
        _ => Err(ResultCode::InvalidArgument),
    }
}
/// Completes one MIME request without waiting for the session or pasting its data.
///
/// # Safety
/// `items` addresses `count` initialized MimeItems, each containing readable
/// call-scoped buffers. Empty arrays may be null. No input pointer is retained.
#[allow(
    unsafe_code,
    reason = "D-43: checked C MIME array and binary-buffer copy"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn puzzle_surface_clipboard_mime_answer(
    id: u64,
    token: u64,
    result: u32,
    revision: u64,
    items: *const abi::MimeItem,
    count: usize,
) -> ResultCode {
    registry::result((|| {
        // Reject invalid numeric tags before interpreting any foreign payload.
        let status = status(result)?;
        if count > MAX_TYPES || token == 0 {
            return Err(ResultCode::InvalidArgument);
        }
        // SAFETY: caller promises a live initialized array; helper checks its range/alignment.
        let inputs = unsafe { memory::array(items, count)? };
        // Aggregate capacity bounds many individually valid small alternatives.
        let mut bytes = 0usize;
        let mut owned = Vec::with_capacity(count);
        // Check each binary length before copying, not after allocating its contents.
        for item in inputs {
            bytes = bytes.saturating_add(item.data.len);
            if bytes > MAX_BYTES || item.mime.len > 256 {
                return Err(ResultCode::InvalidArgument);
            }
            // SAFETY: the MIME range is readable until this call returns and is copied now.
            let mime = unsafe { memory::text(item.mime)? };
            if mime.is_empty() || !mime.bytes().all(|b| b.is_ascii_graphic()) {
                return Err(ResultCode::InvalidArgument);
            }
            // SAFETY: binary data obeys the same call-scoped readable range contract.
            let data = unsafe { memory::bytes(item.data)? };
            owned.push(MimeData {
                mime,
                data: data.into(),
            });
        }
        // Handle lookup also enforces the app thread before posting the owned message.
        let mut registry = registry::lock()?;
        registry
            .surface(id)?
            .surface
            .clipboard_answer(Answer {
                token,
                revision: (revision != u64::MAX).then_some(revision),
                status,
                items: owned,
            })
            .map_err(ResultCode::from)
    })())
}

/// Completes a trusted user paste gesture; secure credential generation lives in embed.
///
/// # Safety
/// `mimes` addresses `count` initialized Bytes, each readable for this call; `text`
/// follows the same contract. No foreign buffer or pointer is retained.
#[allow(unsafe_code, reason = "D-43: checked call-scoped MIME names and text")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn puzzle_surface_clipboard_paste(
    id: u64,
    primary: u32,
    revision: u64,
    mimes: *const abi::Bytes,
    count: usize,
    text: abi::Bytes,
) -> ResultCode {
    registry::result((|| {
        if primary > 1 || revision == u64::MAX || count > MAX_TYPES || text.len > MAX_BYTES {
            return Err(ResultCode::InvalidArgument);
        }
        // SAFETY: caller promises initialized live elements; helper checks alignment and range.
        let inputs = unsafe { memory::array(mimes, count)? };
        // The gesture carries availability metadata; actual bytes are read only on redemption.
        let mut names = Vec::with_capacity(count);
        // Empty or control-bearing labels cannot be emitted into the MIME listing.
        for input in inputs {
            if input.len == 0 || input.len > 256 {
                return Err(ResultCode::InvalidArgument);
            }
            // SAFETY: each call-scoped name is copied before the host may release it.
            let name = unsafe { memory::text(*input)? };
            if !name.bytes().all(|byte| byte.is_ascii_graphic()) {
                return Err(ResultCode::InvalidArgument);
            }
            names.push(name);
        }
        // SAFETY: optional fallback text has the standard readable UTF-8 buffer contract.
        let text = unsafe { memory::text(text)? };
        registry::lock()?
            .surface(id)?
            .surface
            .clipboard_paste(primary == 1, revision, names, Some(text))
            .map_err(ResultCode::from)
    })())
}
