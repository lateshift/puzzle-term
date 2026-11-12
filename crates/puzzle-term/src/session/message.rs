//! Typed operations for the one Session owner on desktop and web (D-50).
//!
//! Messages are owned values, not callbacks into another thread. The platform
//! chooses a channel or a direct call; this module has no synchronization.
//! Commands retain their bus policies and origin. Pointer ownership is decided
//! before encoding, on the same owner as query focus and terminal modes.
//!
//! Surface changes and OS PTY resize are intentionally separate values: a
//! surface can follow a drag immediately while the existing quiet-period rule
//! coalesces SIGWINCH. Applying either never acquires a font or GPU resource.
//! A registration result carries the sender's token rather than returning a
//! handle synchronously across owners. Model and view ownership enter together;
//! only the drawing half subsequently travels with snapshot publication.
//! This core seam excludes diagnostic closures. Platform test/probe channels
//! may execute diagnostic requests on their owner without changing this API.
//! Presentation results describe successful submission rather than merely a
//! finished CPU build. New hit regions cannot become active before their ink.
//! Failed preparation requests a retry; skipped presentation preserves uploads
//! with the host and cannot replace the previously presented input geometry.
//! Clipboard host answers carry owned MIME bytes and a checked native generation.
use super::{Session, cursor_settings::CursorSettings};
use crate::{keybind::Keybindings, messages::Command, settings::Settings};
use puzzle_bus::Origin;
use puzzle_overlay::{
    decoration::{Decoration, DecorationId},
    model::OverlayModel,
    provider::PointerEvent,
    registry::ProviderId,
    view::OverlayView,
};
use puzzle_text::metrics::CellMetrics;
use web_time::Instant;

/// Font resources never cross into Session; only their measured grid geometry does.
#[derive(Clone, Copy)]
pub struct FontMetrics {
    /// Integer physical dimensions and baseline used by VT pixel reports.
    pub cell: CellMetrics,
    /// Logical font size used to derive overlay and grid extents.
    pub font_size: f32,
    /// Device pixels per logical pixel for signed pointer conversion.
    pub dpr: f32,
    /// Renderer atlas incarnation used to invalidate stale row handles.
    pub atlas_generation: u64,
}

/// Complete host mutation seam; no message borrows a Session or a text context.
pub enum SessionMessage {
    /// Host has seen prior output; fresh output may emit the next activity edge.
    ActivitySeen,
    /// Host UI rectangles in logical surface points; empty clears exclusions.
    OccludedRects(Vec<puzzle_overlay::geometry::Rect>),
    /// Choose built-in or host query controls without changing platform identity.
    SearchUi(crate::search::SearchUi),
    /// Return current plain selection text by a correlated asynchronous host event.
    SelectedText(u64),
    /// Actual FIFO writer consumption, never mere receipt of an input callback.
    ReplyCompleted(u64),
    /// Explicit writer failure closes this session without pretending delivery succeeded.
    ReplyFailed(String),
    /// Native MIME completion stays distinct from user paste events.
    ClipboardAnswer(crate::clipboard::Answer),
    /// Trusted gesture plus clipboard revision, never a programmatic read.
    ClipboardPaste(crate::clipboard::Paste),
    /// Raw modifier or consumed pointer input still revokes a pending paste grant.
    UserActivity,
    /// Trusted native gesture data is separate from untrusted OSC negotiation.
    Drop(crate::dnd::Input),
    /// Native notification completion is routed to the originating Session only.
    NotificationFeedback(String, crate::notification::Feedback),
    /// Host visibility feeds the notification occasion policy independently of focus.
    NotificationVisibility(bool),
    /// Harness selects in-window presentation; system hosts leave this disabled.
    NotificationToasts,
    /// Policy-controlled key, committed text, IME, scroll, search or paste.
    Command(Command, Origin),
    /// Native bounded decoder completion resumes application parsing in source order.
    ImageDecoded(crate::images::DecodeResult),
    /// Exact native texture limit drives image admission, including filtering borders.
    ImageTextureLimit(u32),
    /// Browser printable candidate: policy decides whether native text may follow.
    NativeKey(puzzle_vt::input::KeyEvent, Origin),
    /// Surface-relative overlay input, including Leave without an app mouse event.
    OverlayPointer(PointerEvent),
    /// One native refresh sample; current Session modes decide protocol deduplication.
    PointerMotion(crate::pointer::PointerInput),
    /// Latest backing extent, independent of the throttled grid/PTY changes.
    SurfaceSize([u32; 2]),
    /// Cell resize/reflow sampled once per host frame during a drag.
    GridResize {
        /// Requested visible columns, clamped by VT.
        columns: usize,
        /// Requested visible rows, clamped by VT.
        rows: usize,
        /// Physical extent associated with this cell grid.
        surface: [u32; 2],
    },
    /// Delayed OS resize and ordered mode-2048 reply after the quiet interval.
    PtyResize([u32; 2]),
    /// Font/scale measurements copied by the render-side resource owner.
    FontMetrics(FontMetrics),
    /// Validated settings, owned because their source store stays with the host.
    Settings(Box<Settings>),
    /// Shortcut replacement; unchanged tables preserve their pending leader.
    Keybindings(Keybindings),
    /// Explicit caret preferences without changing DECSCUSR state.
    CursorSettings(CursorSettings),
    /// Accessibility preference shared by scrolling and overlay animation.
    ReducedMotion(bool),
    /// Physical inset of the grid within the backing surface.
    Padding(u32),
    /// Reserve whole left UI cells without changing vertical padding.
    GutterColumns(u8),
    /// Idempotent registration of search, scrollbar and new-output models/views.
    InstallModelOverlays,
    /// Host-owned optional protocol moves to the Session thread before later output.
    RegisterOsc {
        /// Unhandled numeric identity; built-ins and duplicates are rejected.
        number: u16,
        /// The protocol owns all mutable state on the Session owner.
        handler: Box<dyn crate::osc::OscHandler>,
    },
    /// Trusted host control for an already registered optional handler.
    /// This internal owner message is never decoded from PTY or public commands.
    OscControl {
        /// Registration identity; a sibling handler cannot receive the payload.
        number: u16,
        /// Owned typed extension; private material must have a redacted wire view.
        control: puzzle_bus::wire::Extension,
    },
    /// Bounded suggestion data, never bytes until the explicit acceptance message.
    GhostText(Option<String>),
    /// Commits the current suggestion through the normal input policy.
    AcceptGhostText,
    /// Explicit debug visibility toggles opt-in frame observation.
    PerformanceHud(bool),
    /// Presentation acknowledgement carries targets without returning a builder.
    FrameResult(FrameResult),
    /// One new interaction model and its drawing counterpart.
    AddOverlay {
        /// Host correlation token returned with the registration result.
        request: u64,
        /// Mutable interaction state moves to the Session owner.
        model: Box<dyn OverlayModel>,
        /// View moves onward exactly once with a published snapshot.
        view: Box<dyn OverlayView>,
    },
    /// Removes a registration and its presented hit targets.
    RemoveOverlay(ProviderId),
    /// Creates a tracked endpoint on its exclusive Session owner.
    AddMarker {
        /// Host correlation token returned with the optional endpoint.
        request: u64,
        /// Retained source cell, validated by Session::add_marker.
        cell: puzzle_overlay::geometry::Cell,
    },
    /// Releases only this caller-owned tracked endpoint.
    RemoveMarker(puzzle_overlay::anchor::Marker),
    /// Validated declarative addition with an asynchronous identity result.
    AddDecoration {
        /// Host token for the returned addition result.
        request: u64,
        /// Immutable declaration validated before installation.
        declaration: Decoration,
    },
    /// Atomic replacement of one existing declarative definition.
    UpdateDecoration(DecorationId, Decoration),
    /// Idempotent disposal of declarative content and its private witnesses.
    RemoveDecoration(DecorationId),
}

/// Completion of host-owned rendering; failed work cannot replace hit targets.
pub enum FrameResult {
    /// Counters from an actually submitted frame, absent when no observer exists.
    Statistics(crate::messages::FrameStats),
    /// Successful submission returns precisely the regions the user now sees.
    Presented(puzzle_overlay::regions::PresentedRegions),
    /// Invalid frame construction leaves damage requested for a complete retry.
    BuildFailed,
    /// Acquisition or pacing skipped this build; its uploads stay with the host.
    Skipped,
}

/// Applying a value has the same policy ordering as the prior direct host methods.
impl Session {
    /// Executes one operation immediately on the owning thread.
    /// Errors describe rejected geometry or definitions; no partial declaration is installed.
    pub fn apply(&mut self, message: SessionMessage, now: Instant) -> Result<(), String> {
        self.host_time = Some(now);
        self.enable_host_events()?;
        if let SessionMessage::ReplyCompleted(id) = &message {
            // Receipt processing is scalar. A normal owner continuation encodes
            // the next bounded packet turn after priority controls have drained.
            return self.reply_completed(*id);
        }
        if let SessionMessage::ActivitySeen = &message {
            // A scalar host acknowledgement changes no UI/input policy or clipboard
            // grant. It needs no event collection and never wakes an idle frame.
            self.activity_seen();
            return Ok(());
        }
        // Expire a stalled transaction before accepting a delayed native completion.
        self.clipboard
            .finish(self.terminal.reset_epoch, now, &mut self.terminal.replies);
        self.clipboard
            .reconcile_grant(self.terminal.clipboard_grant_epoch);
        if matches!(
            &message,
            SessionMessage::NativeKey(_, Origin::User)
                | SessionMessage::OverlayPointer(_)
                | SessionMessage::PointerMotion(_)
                | SessionMessage::Command(_, Origin::User)
                | SessionMessage::UserActivity
        ) {
            self.clipboard.revoke_paste_grant();
        }
        // Flush earlier output facts before this input's interaction results.
        self.collect_host_events();
        let result = self.apply_message(message, now);
        // A timeout can complete while processing geometry rather than a bus command.
        self.flush_clipboard_replies();
        // Even a failed OS resize may have produced a valid grid fact and wake.
        self.collect_host_events();
        result
    }

    /// Dispatches state mutations without invoking callbacks on a foreign owner.
    fn apply_message(&mut self, message: SessionMessage, now: Instant) -> Result<(), String> {
        match message {
            SessionMessage::OccludedRects(rects) => self.set_occluded_rects(rects)?,
            SessionMessage::SearchUi(ui) => self.set_search_ui(ui),
            SessionMessage::SelectedText(request) => self.selected_text_requested(request),
            SessionMessage::ReplyCompleted(id) => self.reply_completed(id)?,
            SessionMessage::ReplyFailed(error) => self.fail_reply(error),
            SessionMessage::UserActivity => {}
            SessionMessage::Drop(input) => self.drop_input(input, now)?,
            SessionMessage::NotificationFeedback(token, outcome) => {
                self.notification_feedback(&token, outcome)
            }
            SessionMessage::NotificationVisibility(visible) => self.notifications.visible = visible,
            SessionMessage::NotificationToasts => self.enable_notification_toasts(),
            SessionMessage::ClipboardPaste(paste) => self.clipboard_paste(paste, now),
            SessionMessage::ClipboardAnswer(answer) => self.clipboard_answer(answer),
            SessionMessage::ImageDecoded(result) => self.complete_image_decode(result, now),
            SessionMessage::ImageTextureLimit(limit) => self.set_image_texture_limit(limit),
            SessionMessage::Command(command, origin) => {
                self.apply_host_command(command, origin, now)
            }
            SessionMessage::NativeKey(key, origin) => self.apply_native_key(key, origin, now),
            SessionMessage::OverlayPointer(event) => self.apply_overlay_pointer(event, now),
            SessionMessage::PointerMotion(input) => self.apply_motion(input, now),
            SessionMessage::SurfaceSize(size) => self.set_surface_size(size),
            SessionMessage::GridResize {
                columns,
                rows,
                surface,
            } => self.resize_grid(columns, rows, surface)?,
            SessionMessage::PtyResize(size) => self.resize_pty(size)?,
            SessionMessage::FontMetrics(font) => {
                // Reject bad divisors before viewport or pointer policy changes.
                font.validate()?;
                self.set_font_metrics(font);
            }
            SessionMessage::Settings(settings) => self.apply_settings(&settings, now),
            SessionMessage::Keybindings(bindings) => {
                // An unrelated settings reload must not flush an in-flight leader.
                if self.keybindings.list() != bindings.list()
                    || self.keybindings.timeout_ms != bindings.timeout_ms
                {
                    self.set_keybindings(bindings);
                }
                // A genuinely replaced table flushes held keys in their original order.
                self.pump(now);
            }
            SessionMessage::CursorSettings(settings) => self.set_cursor_settings(settings, now),
            SessionMessage::ReducedMotion(enabled) => self.set_reduced_motion(enabled),
            SessionMessage::Padding(pixels) => self.set_padding(pixels),
            SessionMessage::GutterColumns(columns) => self.set_gutter_columns(columns),
            SessionMessage::InstallModelOverlays => self.install_model_overlays(),
            SessionMessage::FrameResult(result) => self.apply_frame_result(result, now),
            SessionMessage::GhostText(text) => self.set_ghost_text(text.as_deref()),
            SessionMessage::AcceptGhostText => self.accept_ghost_text(now),
            SessionMessage::PerformanceHud(shown) => self.set_performance_hud(shown)?,
            other => self.apply_overlay_change(other)?,
        }
        Ok(())
    }

    /// Publish whether a native key permits its subsequent text/IME commit.
    fn apply_native_key(&mut self, key: puzzle_vt::input::KeyEvent, origin: Origin, now: Instant) {
        // Bindings may consume keydown; allowed native commits still route through Session.
        let allowed = self.pump_native_key(key, origin, now);
        self.host_events
            .push_back(super::host_events::HostEvent::NativeTextAllowed(allowed));
    }

    /// Models consume physical pointer input before the terminal sees unhandled samples.
    fn apply_host_command(&mut self, mut command: Command, origin: Origin, now: Instant) {
        if let Command::Pointer(input) = &mut command {
            // OS callbacks may precede an earlier queued font/resize update.
            *input = self.owner_pointer(*input);
            if matches!(
                input.event.kind,
                puzzle_vt::input::MouseKind::Press | puzzle_vt::input::MouseKind::Release
            ) {
                // Button transitions cannot inherit the previous drag’s report identity.
                self.motion.reset();
            }
            // Geometry comes from the last presentation, not a frame being prepared.
            let result = self.route_overlay_sample(*input, now);
            // Single-thread browser hosts need this result to prevent native gestures.
            self.host_events
                .push_back(super::host_events::HostEvent::PointerHandled(
                    result.handled,
                ));
            self.host_events
                .push_back(super::host_events::HostEvent::CursorChanged(result.cursor));
            if result.handled {
                // Consumed overlay input must never also generate application bytes.
                return;
            }
        }
        // Key routing, query input modes and encoding remain one bus transaction.
        self.submit(command, origin);
        self.pump(now);
    }

    /// Cold registration edits return identities through the same host event queue.
    fn apply_overlay_change(&mut self, message: SessionMessage) -> Result<(), String> {
        use super::host_events::HostEvent;
        match message {
            SessionMessage::AddMarker { request, cell } => {
                let marker = self.add_marker(cell);
                self.host_events
                    .push_back(HostEvent::MarkerAdded { request, marker });
            }
            SessionMessage::RemoveMarker(marker) => self.remove_marker(marker),
            SessionMessage::RegisterOsc { number, handler } => {
                self.register_osc_handler(number, handler)?;
            }
            SessionMessage::OscControl {
                number,
                mut control,
            } => {
                self.control_osc_handler(number, &mut control)?;
            }
            SessionMessage::AddOverlay {
                request,
                model,
                view,
            } => {
                // Registry IDs are monotonic; removal cannot alias a later registration.
                let id = self.overlays_mut().add(model, view);
                self.host_events
                    .push_back(HostEvent::OverlayAdded { request, id });
            }
            SessionMessage::RemoveOverlay(id) => {
                self.overlays_mut().remove(id);
            }
            SessionMessage::AddDecoration {
                request,
                declaration,
            } => {
                // Validation precedes installation and identity allocation.
                let result = self.decorations_mut().add(declaration);
                self.host_events
                    .push_back(HostEvent::DecorationAdded { request, result });
            }
            SessionMessage::UpdateDecoration(id, declaration) => {
                self.decorations_mut().update(id, declaration)?
            }
            SessionMessage::RemoveDecoration(id) => {
                self.decorations_mut().remove(id);
            }
            // This helper handles only cold overlay edits dispatched by apply above.
            _ => return Err("operation is not an overlay change".into()),
        }
        Ok(())
    }
}

/// Results change interaction and scheduling, never render-side caches.
impl Session {
    /// Commits successful presented geometry and returns its predecessor for reuse.
    fn apply_frame_result(&mut self, result: FrameResult, now: Instant) {
        match result {
            FrameResult::Statistics(stats) => self.frame_statistics(stats, now),
            FrameResult::Presented(regions) => {
                self.overlay_source.geometry.clone_from(regions.geometry());
                // The renderer can recycle the old allocation in a later slot.
                let previous = self.overlays.presented(regions);
                self.host_events
                    .push_back(super::host_events::HostEvent::RegionsReturned(previous));
            }
            FrameResult::BuildFailed => {
                // Captured damage must not disappear after an unsuccessful build.
                self.publication_key = None;
                self.needs_frame = true;
                self.services.waker.request_frame();
            }
            // FIFO acquisition failures neither discard glyph uploads nor change hit targets.
            FrameResult::Skipped => {}
        }
        self.schedule_blink(now);
    }
}

/// Public measurement values are checked before they can become coordinate divisors.
impl FontMetrics {
    /// Rejects malformed external geometry before changing any live Session policy.
    fn validate(self) -> Result<(), String> {
        if self.cell.width_px == 0
            || self.cell.height_px == 0
            || !self.cell.device_size.is_finite()
            || self.cell.device_size <= 0.0
            || !self.font_size.is_finite()
            || self.font_size <= 0.0
            || !self.dpr.is_finite()
            || self.dpr <= 0.0
        {
            return Err(
                "font measurements require positive finite size, DPR and cell dimensions".into(),
            );
        }
        Ok(())
    }
}
