//! Winit file-only drag adapter; full MIME negotiation is the C host contract.
//! Winit cannot set OS acceptance or provide native drag positions. This subset
//! offers paths at the latest pointer point and enforces program acceptance before
//! delivery. It never opens a file or turns an unaccepted drop into clipboard data.
//!
//! The full product contract is Surface.drop_input and its structured callbacks.
//! Winit exposes only file enter/drop/leave, without a drag-position update callback.
//! Its macOS delegate also accepts OS file drags unconditionally before program feedback.
//! Consequently this adapter cannot promise native cursor acceptance feedback.
//! The user-approved subset enforces the program's acceptance before delivering bytes.
//! Hosts with native dragging APIs implement the full contract without this limitation.
//!
//! Files are offered as text/uri-list and text/plain, not opened as file contents.
//! URI encoding preserves path bytes and escapes delimiters instead of inventing URLs.
//! Plain text preserves the path spelling for applications that want filenames.
//! An unregistered surface retains shell-quoted paste-path behavior.
//! Registered rejection must not fall back to paste, which would bypass program intent.
//!
//! Winit emits one callback per path, so about_to_wait groups one native drop batch.
//! A source with too many paths rejects as a whole rather than truncating silently.
//! Hovered paths are bounded independently of their eventual native drop callback.
//! Offer movement is coalesced with the ordinary paced pointer turn (P12).
//! The latest pointer position is a documented limitation of the file-only subset.
//! Pointer coordinates remain logical view-local points; Session owns inset subtraction.
//!
//! Registration replaces all prior host state, including accepted MIME preferences.
//! A program starts rejected and must explicitly accept copying before data delivery.
//! The subset never advertises move because it does not transfer or delete files.
//! An empty preference update preserves the preceding selection as OSC 72 specifies.
//! A conclusion or leave clears acceptance so the next gesture cannot inherit it.
//! Native callbacks are associated with the surface that owns this InputWindow.
//!
//! Hover names carry empty immutable data; no payload read is possible before Drop.
//! At Drop both ordinary representations are captured in memory and bounded together.
//! The preferred list may filter unavailable alternatives but never opens another source.
//! Copied bytes go through the normal nonblocking Surface message path.
//! The OS can release the file-drag object immediately after its callbacks finish.
//! No worker, native bridge, timer or unsafe platform glue is introduced here.
//!
//! Unused windows have no Target allocation and do not scan paths while rendering.
//! The one optional field is inspected only on relevant pointer/native drag events.
//! A registered idle target requests no frames until its hover changes.
//! File encoding tests use synthetic paths so they never depend on personal data.
//! Acceptance of richer MIME formats is covered separately by synthetic scenarios.
//! Those scenarios exercise exactly the same Session messages as the C host API.

use super::InputWindow;
use puzzle_embed::{DropEffect, DropInput, DropKind};
use puzzle_term::clipboard::MimeData;
use std::path::PathBuf;
/// Optional registered interaction state, absent from ordinary windows.
#[derive(Default)]
pub(super) struct Target {
    /// Per-surface registration generation; replacements discard this whole value.
    generation: u64,
    /// Only copying file paths is supported by this winit adapter.
    accepted: bool,
    /// Names preferred by the program, empty means both ordinary representations.
    preferred: Vec<String>,
    /// Bounded file paths are supplied by a real OS drag, never OSC output.
    hovered: Vec<PathBuf>,
    /// One pending batch groups winit's one-event-per-file callbacks.
    dropped: Vec<PathBuf>,
    /// Motion is sent only on a paced flush.
    dirty: bool,
}
impl Target {
    /// Finish one gesture even when its offered data is rejected or over the cap.
    fn capture_drop(&mut self) -> Option<Vec<MimeData>> {
        let paths = std::mem::take(&mut self.dropped);
        let accepted = std::mem::replace(&mut self.accepted, false);
        // A rejected gesture cannot lend its hover or acceptance to the next drag.
        self.hovered.clear();
        if !accepted || paths.len() > puzzle_term::dnd::MAX_ITEMS {
            return None;
        }
        let mut items = representations(&paths);
        // Preference filtering happens after native capture, never during hover.
        if !self.preferred.is_empty() {
            items.retain(|item| self.preferred.contains(&item.mime));
        }
        (!items.is_empty()).then_some(items)
    }
}
impl InputWindow {
    /// Registration and acceptance are typed values after owner-side policy.
    pub(super) fn drop_effect(&mut self, effect: DropEffect) {
        match effect {
            DropEffect::Registration { generation, mimes } => {
                // Replacing the allocation also discards obsolete native acceptance.
                self.drop_target = mimes.map(|_| {
                    Box::new(Target {
                        generation,
                        ..Default::default()
                    })
                });
            }
            DropEffect::Acceptance {
                generation,
                operation,
                mimes,
            } => {
                if let Some(target) = &mut self.drop_target
                    && target.generation == generation
                {
                    // Winit offers path copies; move acceptance cannot be honored safely.
                    target.accepted = operation == 1;
                    // An omitted preference list means unchanged, rather than rejecting all.
                    if !mimes.is_empty() {
                        target.preferred = mimes.split_whitespace().map(str::to_owned).collect();
                    }
                }
            }
            DropEffect::Concluded { generation, .. } => {
                if let Some(target) = &mut self.drop_target
                    && target.generation == generation
                {
                    target.hovered.clear();
                    target.dropped.clear();
                    target.accepted = false;
                }
            }
        }
    }
    /// Collect native enter/drop paths without reading any pointed-to file.
    pub(super) fn file_drag(&mut self, path: PathBuf, drop: bool) {
        // A program registration changes routing, never the meaning of the OS gesture.
        let Some(target) = &mut self.drop_target else {
            if drop {
                self.sent(self.session.paste(quote_path(&path)));
            }
            return;
        };
        // Native callbacks arrive per file, then flush together at the event boundary.
        let paths = if drop {
            &mut target.dropped
        } else {
            &mut target.hovered
        };
        // Curated eager native capture is bounded; extra paths are rejected as a batch.
        if paths.len() <= puzzle_term::dnd::MAX_ITEMS {
            paths.push(path);
        }
        target.dirty = true;
        self.sent(self.session.request_frame());
    }
    /// Leaving revokes the whole hover and any pending uncommitted native batch.
    pub(super) fn drag_left(&mut self) {
        if let Some(target) = &mut self.drop_target {
            let generation = target.generation;
            target.hovered.clear();
            target.dropped.clear();
            target.accepted = false;
            target.dirty = false;
            self.sent(self.session.drop_input(DropInput {
                generation,
                kind: DropKind::Leave,
                position: self.drop_position,
                operations: 1,
                items: Vec::new(),
            }));
        }
    }
    /// Pointer motion changes the offer position, coalesced with ordinary input (P12).
    pub(super) fn drag_position(&mut self, position: [f64; 2]) {
        self.drop_position = position;
        if let Some(target) = &mut self.drop_target {
            target.dirty |= !target.hovered.is_empty();
        }
    }
    /// Drop batches flush at the event boundary; motion only on the paced pointer turn.
    pub(in crate::window) fn flush_drop(&mut self, motion: bool) {
        let Some(target) = &mut self.drop_target else {
            return;
        };
        // Actual Drop must flush promptly; hover alone waits for the paced motion turn.
        let drop = !target.dropped.is_empty();
        if !drop && (!motion || !target.dirty || target.hovered.is_empty()) {
            return;
        }
        target.dirty = false;
        // Capture consumes the gesture on every exit, including rejection and size errors.
        let items = if drop {
            let Some(items) = target.capture_drop() else {
                return;
            };
            items
        } else {
            if target.hovered.len() > puzzle_term::dnd::MAX_ITEMS {
                return;
            }
            names()
        };
        let input = DropInput {
            generation: target.generation,
            kind: if drop { DropKind::Drop } else { DropKind::Move },
            position: self.drop_position,
            operations: 1,
            items,
        };
        // This send queues owner work and cannot block the native event loop on parsing.
        self.sent(self.session.drop_input(input));
    }
}
/// Hover names carry no bytes, so they cannot grant a read before a real drop.
fn names() -> Vec<MimeData> {
    ["text/uri-list", "text/plain"]
        .map(|mime| MimeData {
            mime: mime.into(),
            data: (&b""[..]).into(),
        })
        .into()
}
/// URI escaping preserves arbitrary Unix path bytes; data contains names only.
fn representations(paths: &[PathBuf]) -> Vec<MimeData> {
    let mut uris = String::new();
    let mut plain = String::new();
    // CRLF separates URI-list entries; plain names use newlines without shell interpretation.
    for path in paths {
        uris.push_str("file://");
        // Percent encoding preserves non-UTF-8 Unix filenames without dropping path bytes.
        for &byte in path.as_os_str().as_encoded_bytes() {
            if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
                uris.push(char::from(byte));
            } else {
                use std::fmt::Write;
                let _ = write!(uris, "%{byte:02X}");
            }
        }
        uris.push_str("\r\n");
        if !plain.is_empty() {
            plain.push('\n');
        }
        plain.push_str(&path.to_string_lossy());
    }
    if uris.len().saturating_add(plain.len()) > puzzle_term::dnd::MAX_BYTES {
        return Vec::new();
    }
    vec![
        MimeData {
            mime: "text/uri-list".into(),
            data: uris.into_bytes().into(),
        },
        MimeData {
            mime: "text/plain".into(),
            data: plain.into_bytes().into(),
        },
    ]
}
/// Unregistered sessions retain the conventional shell-quoted paste-path action.
fn quote_path(path: &std::path::Path) -> String {
    format!("'{}' ", path.to_string_lossy().replace('\'', "'\\''"))
}
#[cfg(test)]
mod tests;
