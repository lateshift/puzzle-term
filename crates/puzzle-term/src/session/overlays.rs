//! Session-owned overlay models publish immutable state to render-owned views (D-50).
//!
//! Source events accumulate until the next model update. Only dirty or animating
//! models publish a value, and each rotating slot retains its own state version.
//! Drawing borrows frozen source data and independently owned font/shape caches.
//! No view can change hover, drag, timers or the Session's scheduling requests.
//!
//! Tracked anchors use VT's retained-position registry, including reflow and
//! eviction. Visible BufferView sampling bounds callback queries to this frame.
//! Input uses the geometry and region table acknowledged by a successful host
//! presentation; merely building a future frame never moves its hit targets.
//!
//! Semantic colors, source buffers and region tables retain their allocations.
//! Animation uses the host's injected clock and only models re-arm the Waker.
//! Source capture fills caller-owned slots while every drawing cache stays
//! permanently with the render host; presentation returns only region values.
//!
//! There are two clocks of ownership. A model sees the current Session source
//! values but only the latest presented placement geometry. A view sees frozen
//! source values plus the fresh geometry of the frame it is building. Neither
//! half borrows the other; pointer actions can advance even during cold glyph work.
//!
//! ModelRegistry tracks source interests and finite deadlines. Capture notifies
//! it once with accumulated events, advances dirty models, then publishes each
//! changed immutable value by version. Per-slot versions avoid copying a clean
//! model again merely because another slot was used for the preceding frame.
//! New drawing registrations transfer once and use monotonic provider identities.
//!
//! The source's Requests owner is deliberately absent from render snapshots.
//! Callbacks can queue one navigation action, request another frame or set a
//! finite deadline. Session drains those requests after ending the model borrow,
//! then dispatches actions through the usual bus policy instead of bypassing it.
//! A renderer can reconstruct cached drawing without knowing those requests.
//!
//! Built region tables are candidates until the host acknowledges presentation.
//! Taking a candidate transfers its buffers to hit-testing and returns the old
//! presented table as scratch. This ping-pong avoids allocating region commands
//! or geometry arrays on every dirty frame. A skipped build keeps current hits.
//! Stable provider IDs keep captures valid when other registrations are removed.
//!
//! Declaration samples include source lifetimes and shared immutable descriptions.
//! VT resolves their tracked endpoints, so eviction removes obsolete annotations
//! and reflow follows the same registry used by selection and search. The view's
//! BufferSnapshot carries only the visible slice and metadata needed by anchors.
//! It cannot accidentally expose retained history absent from this publication.
//!
//! Theme token conversion happens once per source capture, preserving authored
//! scrollbar alpha while giving panel tokens opaque RGB. Drawing receives a
//! frozen copy in reused capacity rather than another theme resolver or store.
//! Reduced motion belongs to the same capture and prevents recurring fade work.
use super::Session;
use puzzle_overlay::{OverlayRegistry, buffer::BufferSnapshot, provider::EventKinds};
use web_time::Instant;
/// Pointer callbacks use only Session-owned model data and presented geometry.
mod pointer;
/// Mutable source models and presented geometry stay with the Session.
pub(super) mod source;

/// Input-side ownership supplies frozen callbacks to the frame-building thread.
impl Session {
    /// Borrows the public provider API and requests its next possible presentation.
    pub fn overlays_mut(&mut self) -> &mut OverlayRegistry {
        self.needs_frame = true;
        self.services.waker.request_frame();
        &mut self.overlays
    }
    /// Creates a shared tracked endpoint, validated against currently retained text.
    pub fn add_marker(
        &mut self,
        pos: puzzle_overlay::geometry::Cell,
    ) -> Option<puzzle_overlay::anchor::Marker> {
        let row = self.terminal.screen().ring.line(pos.line)?;
        if pos.column >= row.cells.len() {
            return None;
        }
        Some(self.terminal.tracked.track(pos.line, pos.column))
    }
    /// Resolves a marker without copying a cell or exposing the mutable registry.
    pub fn marker(
        &self,
        marker: puzzle_overlay::anchor::Marker,
    ) -> crate::vt::tracked::PositionState {
        self.terminal.tracked.get(marker)
    }
    /// Releases ownership; stale copied handles can never resolve another cell.
    pub fn remove_marker(&mut self, marker: puzzle_overlay::anchor::Marker) {
        self.terminal.tracked.release(marker);
    }
    /// Fills a slot's sources without lending a view registry or a frame builder.
    pub(super) fn capture_overlay_payload(
        &mut self,
        payload: &mut crate::frame_snapshot::publication::OverlayPayload,
        now: Instant,
        dpr: f32,
        track: [f64; 4],
    ) {
        // Optional source models seal their own state on the same owner boundary.
        // The empty registration loop constructs no context or view payload.
        // Capture the viewport bound before borrowing VT exclusively for callbacks.
        // Source models can resolve watched endpoints, never inspect render caches.
        // Actual drawing remains after this entire owner-side publication finishes.
        let visible_rows = self.viewport.source_extent(self.terminal.size().1);
        self.osc.publish_overlays(
            &mut self.terminal,
            &mut self.overlays,
            crate::osc::SourceWindow {
                top: self.viewport.top_index(),
                rows: visible_rows,
            },
        );
        self.reconcile_decorations();
        self.viewport.scrollbar.set_track(track);
        self.update_overlay_source(now, dpr);
        let source = &self.overlay_source;
        let ctx = puzzle_overlay::model::ModelCtx {
            geometry: &source.geometry,
            time: source.time,
            theme: &source.theme,
            reduced_motion: self.overlay_reduced_motion,
            models: &source.models,
            decorations: &source.decorations,
            requests: &source.requests,
        };
        self.overlays.update(&ctx);
        self.overlays.publish(&mut payload.snapshot);
        payload.models.copy_from(&source.models);
        payload.decorations.clone_from(&source.decorations);
        payload.theme.tokens.clone_from(&source.theme.tokens);
        payload.time = source.time;
        payload.reduced_motion = self.overlay_reduced_motion;
        payload.pointer = self.overlays.pointer;
        if !self.overlays.is_empty() {
            // Copy only the visible anchor window into this slot's reusable storage.
            let count = self.viewport.visible_count(self.terminal.size().1);
            payload
                .buffer
                .get_or_insert_with(BufferSnapshot::default)
                .capture_rows(&self.terminal, count, &|row| {
                    self.viewport.source_row(row).unwrap_or(usize::MAX)
                });
        }
        self.finish_overlay_requests(now);
    }
    /// Refreshes source data without moving any interaction callback or view cache.
    fn update_overlay_source(&mut self, now: Instant, dpr: f32) {
        self.refresh_link();
        let epoch = *self.overlay_epoch.get_or_insert(now);
        // Moving this small owner out permits source sampling without aliasing Session.
        let mut source = std::mem::take(&mut self.overlay_source);
        source.time = now.saturating_duration_since(epoch).as_secs_f64();
        source.requests.frame.set(false);
        source.requests.until.set(0.0);
        source.requests.wake.set(0.0);
        self.copy_models(&mut source.models, dpr);
        self.decorations.capture(
            &self.terminal,
            self.overlays.pointer,
            &mut source.decorations,
        );
        if let Some(theme) = self.theme_state.rendered() {
            install_tokens(&mut source.theme, theme);
        }
        self.overlays.notify(self.overlay_events);
        self.overlay_events = EventKinds::default();
        self.overlay_source = source;
    }
    /// Scheduling and typed actions are consumed on the model owner, never by views.
    fn finish_overlay_requests(&mut self, now: Instant) {
        if let Some(pointer) = self.overlay_source.requests.source_pointer.take() {
            let effect = self.osc.user_action(
                Some(pointer.channel),
                crate::osc::OscUserAction::Pointer {
                    region: pointer.region,
                    event: pointer.event,
                    // Fix copy policy at the gesture, rather than after async export.
                    trim_trailing: self.selection.trim_trailing,
                },
                &mut self.terminal,
            );
            self.apply_osc_ui_effect(effect, now);
        }
        if let Some(action) = self.overlay_source.requests.action.take() {
            if let puzzle_overlay::models::Action::CopyZoneOutput(id) = action {
                self.copy_block_output(id, now);
            } else {
                let command = match action {
                    puzzle_overlay::models::Action::OpenHoveredLink => {
                        crate::messages::Command::OpenLink(self.model_overlays.link.uri.clone())
                    }
                    _ => crate::messages::Command::OverlayAction(action),
                };
                self.submit(command, puzzle_bus::Origin::User);
                self.pump(now);
            }
        }
        if let Some(deadline) = self.overlays.next_wake()
            && let Some(epoch) = self.overlay_epoch
        {
            self.services
                .waker
                .wake_at(epoch + std::time::Duration::from_secs_f64(deadline));
        }
        if self.overlay_source.requests.frame.get()
            || self.overlays.needs_build(self.overlay_source.time)
        {
            self.needs_frame = true;
            self.services.waker.request_frame();
        }
    }
    /// Merges coarse invalidations arriving during an external frame build.
    pub(super) fn overlay_changed(&mut self, kinds: EventKinds) {
        self.overlay_events.0 |= kinds.0;
    }
}
/// Copies semantic tokens into reused storage; no JSON or platform type crosses the seam.
fn install_tokens(target: &mut puzzle_overlay::provider::Theme, source: &crate::theme::Theme) {
    let ui = &source.ui;
    target.tokens.clear();
    let opaque = |rgb: [u8; 3]| u32::from_le_bytes([rgb[0], rgb[1], rgb[2], 255]);
    target.tokens.extend([
        ("foreground", opaque(source.foreground)),
        ("background", opaque(source.background)),
        ("selectionBackground", opaque(source.selection_background)),
        ("accent", opaque(ui.accent)),
        // Semantic bars use the theme's authored status hues, rather than
        // background-mixed zone fills intended for larger source rectangles.
        ("semanticError", opaque(source.palette[1])),
        ("semanticSuccess", opaque(source.palette[2])),
        ("semanticWarning", opaque(source.palette[3])),
        ("semanticInfo", opaque(source.palette[4])),
        ("scrollbarThumb", u32::from_le_bytes(ui.scrollbar_thumb)),
        ("scrollbarTrack", opaque(ui.scrollbar_track)),
        ("searchMatch", opaque(ui.search_match)),
        ("searchCurrent", opaque(ui.search_current)),
        ("link", opaque(ui.link)),
        ("zoneOk", opaque(ui.zone_ok)),
        ("zoneError", opaque(ui.zone_error)),
        ("badgeBackground", opaque(ui.badge_background)),
        ("badgeForeground", opaque(ui.badge_foreground)),
        ("tooltipBackground", opaque(ui.tooltip_background)),
        ("tooltipForeground", opaque(ui.tooltip_foreground)),
        ("pillBackground", opaque(ui.pill_background)),
        ("pillForeground", opaque(ui.pill_foreground)),
        ("bellFlash", opaque(ui.bell_flash)),
        ("ghostText", opaque(ui.ghost_text)),
        ("preeditUnderline", opaque(ui.preedit_underline)),
        ("gutter", opaque(ui.gutter)),
    ]);
}
