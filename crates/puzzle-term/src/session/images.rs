//! Optional graphics yields keep asynchronous native work in application order.
//! Only byte parsing pauses. Keys, resize, timers and snapshot publication remain
//! ordinary Session operations while the native worker owns a decode request.
//! Graphics status bytes spend generic writer credit before later VT queries.
use super::Session;
use crate::images::{DecodeRequest, DecodeResult, Store};
use web_time::Instant;
#[cfg(test)]
mod tests;

/// Allocated on explicit native opt-in, never in an ordinary browser Session.
pub(super) struct State {
    /// Protocol metadata and decoded pixels belong to this Session owner.
    pub(super) store: Store,
    /// One extracted APC body reuses its allocation across chunks.
    wire: Vec<u8>,
}

/// Platforms supply decode results as messages; no core callback performs I/O.
impl Session {
    /// Enables kitty graphics with a decoded byte allowance; zero disables storage.
    pub fn enable_images(&mut self, limit: usize) {
        // Desktop explicitly enables the optional protocol. Browser Sessions do
        // not acquire payload buffers or a native decoder merely by linking core.
        if self.graphics.is_none() {
            self.graphics = Some(Box::new(State {
                store: Store::new(limit, &self.terminal),
                wire: Vec::new(),
            }));
        }
    }

    /// Applies the native device's image tiling limit without borrowing GPU state.
    pub fn set_image_texture_limit(&mut self, limit: u32) {
        if let Some(state) = &mut self.graphics {
            state.store.set_texture_limit(limit, &mut self.terminal);
        }
    }

    /// Applies the same validated allowance to existing native graphics storage.
    pub(super) fn apply_image_settings(&mut self, settings: &crate::settings::Settings) {
        // Live settings change storage on its sole owner. GPU residency receives
        // the same allowance through snapshots, with independent byte accounting.
        if let Some(state) = &mut self.graphics {
            let limit = (settings.images_storage_limit_mb as usize).saturating_mul(1024 * 1024);
            state.store.set_limit(limit, &mut self.terminal);
        }
    }

    /// Immutable source/placement view for frame publication and diagnostic witnesses.
    pub fn images(&self) -> Option<&Store> {
        // Borrowed source access never transfers the mutable protocol owner.
        // Snapshots clone only immutable pixel ownership for visible instances.
        self.graphics.as_ref().map(|state| &state.store)
    }

    /// True means later application bytes must remain behind the pending image reply.
    pub fn image_decode_pending(&self) -> bool {
        // Pending means outstanding validation, even after take_request moved
        // the source buffer to the worker. It is not the decoder queue length.
        self.graphics
            .as_ref()
            .is_some_and(|state| state.store.decode_pending())
    }

    /// The native worker accepts exclusive ownership of one completed source buffer.
    pub fn take_image_decode_request(&mut self) -> Option<DecodeRequest> {
        self.graphics
            .as_mut()
            .and_then(|state| state.store.take_request())
    }

    /// A full native decode queue returns ownership; no source data is discarded.
    pub fn return_image_decode_request(&mut self, request: DecodeRequest) {
        // A rejected handoff is backpressure, not a protocol failure. Retain
        // exactly the same ticket and payload until a readiness message arrives.
        if let Some(state) = &mut self.graphics {
            state.store.return_request(request);
        }
    }

    /// Commits a worker result and emits its reply before later VT/OSC commands.
    pub(super) fn complete_image_decode(&mut self, result: DecodeResult, now: Instant) {
        if let Some(state) = &mut self.graphics {
            state.store.complete(result, &mut self.terminal);
        }
        // A completion precedes the unparsed suffix. Flushing now prevents a
        // following DA query from overtaking kitty's mandatory feature response.
        self.flush_parser_replies();
        // A successful put changes pixels without writing cells. Decoder errors
        // can still affect diagnostics; normal scheduling coalesces this wake.
        self.output_changed(now);
    }

    /// Full erase epochs and evicted tracked anchors are reconciled only when opted in.
    pub(super) fn reconcile_images(&mut self) {
        if let Some(state) = &mut self.graphics {
            state.store.reconcile(&mut self.terminal);
        }
    }

    /// Pending storage is borrowed only for the parser call, never during action dispatch.
    pub(super) fn advance_image_pending(&mut self, mut start: usize, end: usize) -> usize {
        // Report only accepted bytes to the caller. A paused decoder leaves the
        // suffix in Session's existing pending allocation, in original order.
        let initial = start;
        while start < end
            && !self.image_decode_pending()
            && !self.osc.pending()
            && self.replies.can_parse()
        {
            let Some(state) = &mut self.graphics else {
                break;
            };
            let (used, osc, graphics) = self.terminal.advance_to_reply_boundary(
                &self.pending[start..end],
                &self.osc.numbers,
                &mut self.osc.request,
                Some(&mut state.wire),
                false,
            );
            // Release the byte borrow before actions touch storage or emit replies.
            // Ordinary text between APC chunks is allowed to move the cursor.
            start += used;
            // Actions may start a decoder barrier, which the loop checks before
            // admitting another byte from this already accepted pending slice.
            self.image_boundary(osc, graphics);
            self.reply_boundary(false);
            // A partial semantic boundary must not spin on an unchanged suffix.
            if used == 0 {
                break;
            }
        }
        start - initial
    }

    /// Desktop transaction yielding preserves the unread pool suffix while decoding.
    pub(super) fn advance_image_external(&mut self, bytes: &[u8]) -> usize {
        // This entry is used when a render request awaits a sync transaction.
        // Graphics decoding and OSC dispatch remain independent yield reasons.
        let sync = self.terminal.modes.get(true, 2026) == Some(true);
        let mut used = 0;
        while used < bytes.len()
            && !self.image_decode_pending()
            && !self.osc.pending()
            && self.replies.can_parse()
        {
            let Some(state) = &mut self.graphics else {
                break;
            };
            let (count, osc, graphics) = self.terminal.advance_to_reply_boundary(
                &bytes[used..],
                &self.osc.numbers,
                &mut self.osc.request,
                Some(&mut state.wire),
                sync,
            );
            // Native drain storage retains bytes after this accepted prefix.
            // A query pauses before later bytes can generate another response.
            used += count;
            self.image_boundary(osc, graphics);
            self.reply_boundary(false);
            if count == 0 || sync && self.terminal.modes.get(true, 2026) != Some(true) {
                break;
            }
        }
        used
    }

    /// Earlier lifecycle facts precede graphics; later requests remain unparsed.
    fn image_boundary(&mut self, osc: bool, graphics: bool) {
        // Flush earlier lifecycle facts first, so a prompt/RIS in this same read
        // cannot be applied after the protocol command it precedes on the wire.
        if osc {
            // Reserve extension reply tickets only after flushing older staged replies.
            // Graphics opt-in must share the same receipt order as plain VT feeds.
            self.reply_boundary(true);
        } else if graphics {
            self.extension_boundary(false);
        }
        // Decoded LRU visibility follows the displayed viewport, including history;
        // it must not assume the program's live bottom is what the user sees.
        if graphics {
            self.reconcile_viewport(true);
        }
        if graphics && let Some(state) = &mut self.graphics {
            state
                .store
                .set_viewport(&self.terminal, self.viewport.top_index());
            // Earlier VT replies are still in the same terminal reply buffer;
            // command acknowledgements append rather than overtaking them.
            state.store.command(&state.wire, &mut self.terminal);
        }
    }
}
