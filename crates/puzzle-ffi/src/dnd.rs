//! Call-scoped native drop MIME copies and structured registration callbacks.
//! OSC grammar, permission, generation and reply encoding remain below the FFI.
//! Hosts forward only real user gestures to their creating surface, capture bytes
//! at Drop, and keep paste-the-path behavior until drop/registration is enabled.
//!
//! The C ABI represents a single observation with a flat, checked argument list.
//! Kind 0 covers both enter and movement; the owner detects the first offer.
//! Kind 1 releases a hover without carrying any MIME values.
//! Kind 2 supplies immutable data from the actual drop, never from an OSC request.
//! Coordinates are logical view points, with the same contract as pointer input.
//! The host never parses OSC 72 or computes its grid and pixel coordinates.
//!
//! Registration callbacks include a decimal-string generation to preserve all u64 bits.
//! An array registers native MIME types; null unregisters the native destination.
//! Acceptance reports copy/move/reject and the program's ordered MIME preference list.
//! Conclusion releases captured gesture resources without unregistering the destination.
//! All callback payloads are owned until tick returns; retaining hosts copy their bytes.
//! No callback runs from this input function or from the render thread.
//!
//! The public boundary rejects invalid kinds, nonfinite points and excessive counts first.
//! Only then can initialized foreign array elements be inspected.
//! Each nested range remains call-scoped and is copied before returning to C.
//! A saturating sum checks the aggregate size before each binary allocation.
//! MIME names are UTF-8, printable, bounded and cannot contain OSC list delimiters.
//! Duplicates are ambiguous for one-based protocol reads and therefore reject atomically.
//! No input buffer or native object survives as a borrowed reference in Rust state.
//!
//! The safe embedding layer repeats semantic checks for Rust hosts that bypass C.
//! The Session remains the only protocol/state owner after this translation step.
//! Use-after-free handles and wrong-thread calls follow the normal registry checks.
//! A delayed valid observation with a stale registration is ignored by that owner.
//! Tests call this boundary with malformed arrays without needing a GPU surface.
//! The native fixture exercises actual tick delivery for both macOS attachment kinds.
//! ASan and Miri check the same range-copy code independently of wire conformance.

use crate::{
    abi::{Bytes, MimeItem, ResultCode},
    memory, registry,
};
use puzzle_embed::{DropEffect, DropInput, DropKind};
use serde_json::json;
/// Parsed semantics cross the callback boundary; no host implements OSC parsing.
pub(crate) fn event(effect: DropEffect) -> super::events::OwnedEvent {
    // Translating effects here keeps JSON and C details outside the Session owner.
    let (name, value) = match effect {
        DropEffect::Registration { generation, mimes } => (
            "drop/registration",
            json!({"generation":generation.to_string(), "mimes":mimes.map(|s| s.split_whitespace().map(str::to_owned).collect::<Vec<_>>())}),
        ),
        DropEffect::Acceptance {
            generation,
            operation,
            mimes,
        } => (
            "drop/acceptance",
            json!({"generation":generation.to_string(), "operation":operation,
                "mimes":mimes.split_whitespace().collect::<Vec<_>>()}),
        ),
        DropEffect::Concluded {
            generation,
            operation,
        } => (
            "drop/concluded",
            json!({"generation":generation.to_string(), "operation":operation}),
        ),
    };
    // JSON owns all callback data through app_tick; a retaining host copies it.
    (name.into(), value.to_string().into_bytes().into())
}
/// Forwards one native user gesture without waiting for Session or retaining C storage.
///
/// ## Safety
/// `items` addresses count initialized MimeItems with readable call-scoped ranges.
/// Move carries MIME names with empty data. Empty arrays may be null.
#[allow(
    unsafe_code,
    reason = "D-43: validated call-scoped native MIME drop copy"
)]
#[allow(
    clippy::too_many_arguments,
    reason = "flat C ABI carries one native drag observation"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn puzzle_surface_drop(
    id: u64,
    generation: u64,
    kind: u32,
    x: f64,
    y: f64,
    operations: u32,
    items: *const MimeItem,
    count: usize,
) -> ResultCode {
    registry::result((|| {
        let kind = match kind {
            0 => DropKind::Move,
            1 => DropKind::Leave,
            2 => DropKind::Drop,
            _ => return Err(ResultCode::InvalidArgument),
        };
        // Reject numeric ranges before dereferencing foreign payloads.
        if generation == 0
            || !x.is_finite()
            || !y.is_finite()
            || count > puzzle_term::dnd::MAX_ITEMS
            || kind != DropKind::Leave && !(1..=3).contains(&operations)
            || kind == DropKind::Leave && count != 0
        {
            return Err(ResultCode::InvalidArgument);
        }
        // SAFETY: caller promises initialized live elements; helper checks range/alignment.
        let items = unsafe { memory::array(items, count)? };
        // Copies finish during this call, before the caller may free native drag objects.
        let owned = copy_items(items, kind)?;
        registry::lock()?
            .surface(id)?
            .surface
            .drop_input(DropInput {
                generation,
                kind,
                position: [x, y],
                operations,
                items: owned,
            })
            .map_err(ResultCode::from)
    })())
}
/// Validate aggregate bytes before copying; no partially admitted drop reaches Session.
#[allow(unsafe_code, reason = "D-43: individual checked MIME buffer copies")]
fn copy_items(
    items: &[MimeItem],
    kind: DropKind,
) -> Result<Vec<puzzle_term::clipboard::MimeData>, ResultCode> {
    let mut bytes = 0usize;
    // Capacity is bounded by the already checked 16-element foreign array.
    let mut owned = Vec::with_capacity(items.len());
    for item in items {
        bytes = bytes.saturating_add(item.data.len);
        if item.mime.len == 0
            || item.mime.len > 256
            || bytes > puzzle_term::dnd::MAX_BYTES
            || kind == DropKind::Move && item.data.len != 0
        {
            return Err(ResultCode::InvalidArgument);
        }
        // SAFETY: private caller has the exported function's readable MIME-range contract.
        let mime = unsafe { memory::text(item.mime)? };
        if owned
            .iter()
            .any(|prior: &puzzle_term::clipboard::MimeData| prior.mime == mime)
            || !mime.contains('/')
            || !mime
                .bytes()
                .all(|b| b.is_ascii_graphic() && !b";:".contains(&b))
        {
            return Err(ResultCode::InvalidArgument);
        }
        // SAFETY: binary ranges remain readable through the exported call; bytes are owned now.
        let data = unsafe {
            memory::bytes(Bytes {
                data: item.data.data,
                len: item.data.len,
            })?
        };
        owned.push(puzzle_term::clipboard::MimeData {
            mime,
            data: data.into(),
        });
    }
    Ok(owned)
}
