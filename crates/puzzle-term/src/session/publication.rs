//! Late, damage-proportional Session publication into a caller-owned slot (D-50).
//!
//! This code advances Session-owned timers and copies cells and model values.
//! It never shapes a glyph, accepts a GPU resource or calls a drawing callback.
//! Each slot keeps its own copy history; the rendering owner keeps separate row
//! cache history. Atlas and policy changes revise the copy epoch for all slots.
//! Failed builds similarly revise every slot, including ones not currently free.
//! The host supplies the refresh time after applying the shared D-33 settle rule;
//! synchronized output stays gated here until reset or the existing timeout.
use super::Session;
use crate::frame_snapshot::FrameSnapshot;
use web_time::Instant;

/// A request may be answered without building or publishing any new content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublishOutcome {
    /// The caller's slot contains complete, sealed visible source values.
    Published,
    /// Nothing changed, fonts are not ready, or synchronized output defers damage.
    NothingNew,
}

/// Session publication needs no access to the rendering owner's fonts or caches.
impl Session {
    /// Advances one display turn and fills only damaged rows in the caller's slot.
    /// `style_epoch` and `atlas_generation` are values reported by the renderer.
    /// Errors retain the frame request and the caller keeps ownership of its slot.
    pub fn publish_frame(
        &mut self,
        now: Instant,
        slot: &mut FrameSnapshot,
        atlas_generation: u64,
        style_epoch: u64,
    ) -> Result<PublishOutcome, String> {
        // Install observation before advancing timers that may emit title/exit facts.
        // Animation facts use this injected display turn, including an idle turn
        // that ends motion without copying any additional row into the slot.
        self.host_time = Some(now);
        self.enable_host_events()?;
        // The caller owns a free slot, so earlier output can be consumed now.
        // A later feed runs on this same owner and rearms the following refresh;
        // rendering never needs a shared wake latch or access to Session.
        self.services.waker.acknowledge_frame();
        self.advance_publication(now);
        if !self.needs_frame || self.sync_suppresses(now) {
            self.collect_host_events();
            return Ok(PublishOutcome::NothingNew);
        }
        // Keep initial damage pending until metrics arrive from the font owner.
        let Some(policy) = &self.frame_policy else {
            self.collect_host_events();
            return Ok(PublishOutcome::NothingNew);
        };
        // Copy only preferences; no Session code reads a drawing cache.
        let policy = policy.clone();
        // Consume the current request before model callbacks ask for an animation frame.
        // Clearing afterwards would discard reveal/fade requests made during capture.
        self.needs_frame = false;
        if let Err(error) =
            self.capture_publication_rows(now, slot, policy, atlas_generation, style_epoch)
        {
            self.needs_frame = true;
            return Err(error);
        }
        self.viewport.presented();
        self.schedule_scroll(now);
        self.schedule_search();
        // Seal API change tracking once per publication, with the existing recycled journal.
        self.terminal.seal_frame();
        self.schedule_blink(now);
        self.collect_host_events();
        Ok(PublishOutcome::Published)
    }

    /// Copies visible rows and interaction facts at one owner-side VT boundary.
    fn capture_publication_rows(
        &mut self,
        now: Instant,
        slot: &mut FrameSnapshot,
        policy: crate::frame_builder::policy::FramePolicy,
        atlas_generation: u64,
        style_epoch: u64,
    ) -> Result<(), String> {
        // Capture invalidation and render-cache invalidation have separate histories.
        // An atlas reset must refresh every slot, not only the next available one.
        let key = (policy.revision, atlas_generation, style_epoch);
        let dpr = policy.dpr;
        let (columns, rows) = self.terminal.size();
        let track = policy.scrollbar_track(columns, rows);
        if self.publication_key != Some(key) {
            self.publication_epoch = self.publication_epoch.wrapping_add(1);
            self.publication_key = Some(key);
        }
        // Model actions finish before rows/viewport are sampled into a sealed value.
        self.capture_overlay_payload(&mut slot.overlays, now, dpr, track);
        slot.capture_session_view(&self.terminal, self.publication_epoch, &self.viewport)?;
        if self.theme_state.transitioning()
            && let Some(theme) = self.theme_state.rendered()
        {
            // The publication owns interpolated defaults; live queries answer the target.
            slot.color_defaults(theme);
        }
        slot.capture_view_images(self.images(), &self.terminal, &self.viewport);
        self.blinking_ink = slot.has_blinking_ink();
        // Cursor/selection/search and preferences share this precise VT boundary.
        // Observation is fixed for this slot even if a subscriber changes mid-build.
        slot.observe_frame = self.bus.observes(puzzle_bus::hooks::EventKinds::at(16));
        slot.policy = Some(policy);
        slot.set_selection(self.selection.range());
        slot.set_search(&self.search.fills);
        Ok(())
    }

    /// Pending output and time-based models precede the idle/synchronization decision.
    fn advance_publication(&mut self, now: Instant) {
        if !self.pending.is_empty() && self.osc.numbers.is_empty() {
            // Registered protocols declare work credits at caller feed turns,
            // never incidental frame callbacks (the extension work-credit contract). Seal the
            // already parsed prefix; the normal host continuation drains its
            // suffix between controls. A frame cannot replenish credits or move
            // past a deferred authenticated mark. The default bulk path retains
            // its existing publication-time drain without any optional callbacks.
            self.feed(&[], now);
        }
        self.pump(now);
        self.advance_theme(now);
        self.advance_scroll(now);
        self.advance_search(now);
        self.advance_blink(now);
        // Models alone schedule animations; their sealed views only paint this turn.
        self.advance_model_overlays(now);
    }
}
