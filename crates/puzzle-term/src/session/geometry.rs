//! Session geometry changes supplied by the platform (§4.3, §6.6).
//! Resize remaps tracked selection cells through the common VT registry.
//! Reconciliation rebases row-version witnesses before the next frame.
//!
//! Fonts and window backing pixels come from a shell, while the terminal
//! grid and row caches remain neutral. A resize reaches VT and the PTY
//! together; DPR changes only the cell measurements until the host derives
//! a new grid. The host throttles a drag before calling `resize`.
//!
//! The font size is logical pixels while `CellMetrics` is in device pixels.
//! Computing metrics in this neutral layer gives browser and native hosts
//! identical glyph origins for a given DPR (D-27). `set_surface_size` is
//! separate from `resize`: a window can change its backing immediately
//! without forcing a PTY SIGWINCH for every drag callback. Once the host's
//! throttle expires, VT reflows and the transport receives one matching
//! cell-and-pixel extent. A failed transport resize leaves the local grid
//! usable and still requests presentation; the host logs the OS failure.
use super::Session;
use super::cursor_settings::CursorSettings;
use crate::{
    frame_builder::{context::TextContext, policy::FramePolicy},
    platform::PtySize,
};
use puzzle_text::metrics::CellMetrics;
use web_time::Instant;

impl Session {
    /// Selects a host-owned text context without copying its font bytes.
    pub fn set_text_context(&mut self, context: &TextContext) {
        self.set_font_metrics(super::message::FontMetrics {
            cell: context.metrics(),
            font_size: context.font_size(),
            dpr: context.dpr(),
            atlas_generation: context.atlas_generation(),
        });
    }

    /// Accepts render-side measurements without passing a font owner into Session.
    /// Pixel geometry is small immutable data and is valid on the web as well.
    /// The message boundary validates finite dimensions before this internal update.
    /// No face lookup or atlas access is needed to update VT pixel reports.
    pub(super) fn set_font_metrics(&mut self, font: super::message::FontMetrics) {
        self.overlay_changed(puzzle_overlay::provider::EventKinds::GEOMETRY);
        // Store measurements once on the Session owner, even before its first frame.
        // Pointer and timer turns only need these values, never a drawing cache.
        let builder = self
            .frame_policy
            .get_or_insert_with(|| FramePolicy::new(font, self.cursor_settings));
        builder.set_font_metrics(font);
        builder.set_cursor_settings(self.cursor_settings);
        if let Some(theme) = self.theme_state.rendered() {
            builder.set_theme(theme);
        }
        let metrics = font.cell;
        // XTWINOPS pixel replies use the same font geometry as the frame;
        // a placeholder size would mislead applications querying cells.
        self.terminal.cell_pixels = cell_pixels(metrics);
        // The same device-cell height converts input rows and renders snapshots.
        self.viewport.set_row_height(metrics.height_px);
        self.reconcile_viewport(false);
        self.needs_frame = true;
        self.services.waker.request_frame();
    }

    /// Borrows provisional composition for host candidate placement and M9 overlays.
    pub fn preedit(&self) -> &crate::messages::PreeditState {
        &self.preedit
    }

    /// Returns device-cell dimensions after platform font bytes arrive.
    #[must_use]
    pub fn cell_metrics(&self) -> Option<CellMetrics> {
        self.frame_policy.as_ref().map(|policy| policy.metrics)
    }

    /// Reflows the VT grid and reports its physical extent to the PTY.
    pub fn resize(&mut self, columns: usize, rows: usize, surface: [u32; 2]) -> Result<(), String> {
        self.resize_grid(columns, rows, surface)?;
        self.resize_pty(surface)
    }

    /// Applies the latest physical grid once per host frame during a drag.
    pub fn resize_grid(
        &mut self,
        columns: usize,
        rows: usize,
        surface: [u32; 2],
    ) -> Result<(), String> {
        let Some(builder) = self.frame_policy.as_mut() else {
            return Err("terminal fonts are not ready for device-pixel resize".into());
        };
        builder.surface = Some(surface);
        // Primary reflow and alternate truncation are VT rules, so the
        // host supplies dimensions rather than editing either screen.
        // A blocked query may still own an older reply prefix. Only this resize
        // may be coalesced; earlier program replies must retain their FIFO place.
        let reply_start = self.terminal.replies.len();
        self.terminal.resize(columns, rows);
        self.overlay_changed(puzzle_overlay::provider::EventKinds::GEOMETRY);
        // Mode 2048's report belongs after the matching OS PTY resize.
        // Keep it out of the ordinary parser reply buffer during the
        // 25 ms coalescing window, so fresh output cannot flush it early.
        self.pending_resize_reply.clear();
        self.pending_resize_reply
            .extend_from_slice(&self.terminal.replies[reply_start..]);
        self.terminal.replies.truncate(reply_start);
        self.reconcile_selection();
        self.search.dirty = true;
        // VT reflow has already remapped the top anchor and source markers.
        self.reconcile_viewport(false);
        self.refresh_scrollbar();
        self.forward_events();
        self.needs_frame = true;
        self.services.waker.request_frame();
        Ok(())
    }

    /// Sends one PTY resize and its matching in-band report after quiet.
    pub fn resize_pty(&mut self, surface: [u32; 2]) -> Result<(), String> {
        let (columns, rows) = self.terminal.size();
        let size = PtySize {
            columns: columns.clamp(2, u16::MAX as usize) as u16,
            rows: rows.clamp(1, u16::MAX as usize) as u16,
            pixel_width: surface[0].min(u16::MAX as u32) as u16,
            pixel_height: surface[1].min(u16::MAX as u32) as u16,
        };
        // VT must reflow even if an external PTY has exited. A failed OS
        // resize is reported, while the host still needs a fresh frame.
        let result = self.services.transport.resize(size);
        // VT emits the optional mode-2048 in-band report while resizing;
        // write it after SIGWINCH so the app observes the new OS dimensions.
        if !self.pending_resize_reply.is_empty() {
            self.terminal.replies.append(&mut self.pending_resize_reply);
            self.flush_parser_replies();
            self.pending_resize_reply.clear();
        }
        result
    }

    /// Moves all cell and cursor origins by a host-selected physical inset.
    pub fn set_padding(&mut self, pixels: u32) {
        if let Some(builder) = self.frame_policy.as_mut() {
            let padding = pixels.min(4096);
            if builder.padding != padding {
                // Insets change every row's placement, not its VT content version.
                builder.padding = padding;
                builder.revision = builder.revision.wrapping_add(1);
            }
            // Geometry-only changes still update the future overlay track.
            self.reconcile_viewport(false);
            // The grid itself is unchanged; only its surface origin moves.
            self.needs_frame = true;
            self.services.waker.request_frame();
        }
    }

    /// Reserves host UI columns; the host supplies the matching reduced PTY grid.
    /// Glyph resources survive because only assembly origins change.
    pub fn set_gutter_columns(&mut self, columns: u8) {
        if let Some(policy) = self.frame_policy.as_mut() {
            if policy.gutter_columns == columns {
                return;
            }
            // Changing UI space invalidates placement policy, not source versions.
            // The matching native host resize reflows to the reduced grid width.
            policy.gutter_columns = columns;
            policy.revision = policy.revision.wrapping_add(1);
            self.overlay_changed(puzzle_overlay::provider::EventKinds::GEOMETRY);
            self.reconcile_viewport(false);
            self.needs_frame = true;
            self.services.waker.request_frame();
        }
    }

    /// Tracks a resized backing store before the grid/PTY throttle expires.
    pub fn set_surface_size(&mut self, size: [u32; 2]) {
        if let Some(builder) = self.frame_policy.as_mut() {
            // Painting can follow the new swapchain size while a delayed
            // grid resize still uses the old row and column count.
            builder.surface = Some(size);
            self.needs_frame = true;
            self.services.waker.request_frame();
        }
    }

    /// Switches cursor focus geometry without touching any VT row version.
    pub fn set_focus(&mut self, focused: bool) {
        apply_focus(
            &self.terminal,
            self.frame_policy.as_mut(),
            &mut self.services,
            &mut self.replies,
            &mut self.needs_frame,
            focused,
        );
    }

    /// Applies cursor appearance on the next frame without editing VT rows.
    pub fn set_cursor_settings(&mut self, settings: CursorSettings, now: Instant) {
        if self.cursor_settings == settings {
            return;
        }
        self.cursor_settings = settings;
        if let Some(builder) = self.frame_policy.as_mut() {
            builder.set_cursor_settings(settings);
            // An Off setting draws through a hidden blink phase, but it
            // cancels the wakeup. Re-enabling Auto after the 15-second idle
            // cutoff would otherwise leave that old hidden phase forever.
            builder.blink = true;
        }
        // Re-arm from the current visible phase without inventing keyboard
        // activity; after the idle cutoff, the caret stays solid (P2).
        self.blink_deadline = None;
        self.needs_frame = true;
        self.services.waker.request_frame();
        self.schedule_blink(now);
    }
}

/// Clamps the device metrics to VT's 16-bit XTWINOPS report fields.
fn cell_pixels(metrics: CellMetrics) -> (u16, u16) {
    // Actual rendering retains u32 metrics. Only the protocol reply has
    // u16 fields, so oversized external DPR values cannot wrap to zero.
    (
        metrics.width_px.min(u16::MAX as u32) as u16,
        metrics.height_px.min(u16::MAX as u32) as u16,
    )
}

/// Focus updates share one rule between direct native calls and typed bus input.
/// Reports stay ordered in the same drain as keys; geometry does not edit rows.
pub(super) fn apply_focus(
    terminal: &crate::vt::Terminal,
    builder: Option<&mut FramePolicy>,
    services: &mut crate::platform::Services,
    replies: &mut super::replies::Lane,
    needs_frame: &mut bool,
    focused: bool,
) {
    // DECSET 1004 opt-in focus reports are input bytes, so they bypass
    // frame scheduling just like a key and preserve event order (§6.8).
    let modes = puzzle_vt::input::InputModes::from_terminal(terminal);
    let bytes = puzzle_vt::input::encode_focus(focused, modes);
    if !bytes.is_empty()
        && let Err(error) = replies.input(services.transport.as_mut(), &bytes)
    {
        tracing::warn!(event="session.focus_write_failed", %error, "focus report could not reach the PTY");
    }
    if let Some(builder) = builder {
        builder.focused = focused;
        *needs_frame = true;
        services.waker.request_frame();
    }
}
