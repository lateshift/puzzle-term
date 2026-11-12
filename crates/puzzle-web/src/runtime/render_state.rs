//! Host-owned drawing caches consume sealed Session publications (D-50).
//!
//! Web needs one reusable slot because both owners run on the same thread.
//! Neither the slot nor the builder returns to Session. Failed/skipped images
//! preserve pending uploads and the input targets of the last actual presentation.
use super::Runtime;
use puzzle_term::frame_builder::{FrameBuilder, context::TextContext};
use puzzle_term::{FrameResult, PublishOutcome, SessionMessage};
use web_time::Instant;

/// Per-terminal drawing ownership; shared fonts and GPU resources remain external.
#[derive(Default)]
pub(super) struct RenderState {
    /// The first available text context initializes a cache, never the Session.
    pub(super) builder: Option<FrameBuilder>,
    /// Row-copy history is retained independently of builder cache history.
    snapshot: puzzle_term::frame_snapshot::FrameSnapshot,
    /// Instance and upload allocations survive skipped surface presentations.
    pub(super) frame: puzzle_term::frame::RenderFrame,
    /// Prior hit-table storage comes back through HostEvent for warm reuse.
    pub(super) regions: puzzle_term::overlay::regions::PresentedRegions,
}

/// Publication and presentation are separate operations even on a single thread.
impl Runtime {
    /// Publishes old-context ink before selecting a differently budgeted resource.
    ///
    /// Prepared glyphs are shared, but pending upload spans belong to the first
    /// terminal that built them. Replacing that terminal's resource must finish
    /// this handoff even when its surface was hidden or acquisition failed.
    /// Otherwise another pane can reuse a ready handle whose pixels never arrived.
    /// Only queue writes run here: no presentation, new snapshot or user callback.
    pub(super) fn retire_text_uploads(&mut self) -> Result<(), String> {
        // Warm replacements with no outstanding geometry need no GPU work.
        if self.render.frame.uploads.is_empty() {
            return Ok(());
        }
        // Headless terminals have no sampled stores or sibling GPU addresses.
        let Some(text) = &self.text else {
            return Ok(());
        };
        // Borrow the old device incarnation; a new key may have a smaller allowance.
        let renderer = text.gpu.renderer.borrow();
        let renderer = renderer
            .as_ref()
            .ok_or("GPU owner unavailable during text replacement")?;
        let mut painter = text.painter.borrow_mut();
        // A hidden first frame may prepare ink before creating its lazy painter.
        if painter.is_none() {
            let surface = self
                .surface
                .as_ref()
                .ok_or("surface missing during text replacement")?;
            // Format comes from this surface, but no drawable is acquired or held.
            let size = self.render.frame.target_size;
            let format = renderer
                .surface_format(surface, size[0], size[1])
                .map_err(|e| e.to_string())?;
            let mut owner = puzzle_render::FrameRenderer::new(renderer, format);
            // Restore the retiring owner's policy before accepting its spans.
            if let Some(context) = text.context.borrow().as_ref() {
                owner.set_glyph_budget(renderer, context.glyph_budget_bytes());
            }
            *painter = Some(owner);
        }
        // Another terminal can still reference this old shared atlas. A skipped
        // presentation must not lose the uploads when this terminal changes key.
        painter
            .as_mut()
            .ok_or("painter missing")?
            .write_frame_uploads(renderer, &mut self.render.frame)
            .map_err(|e| e.to_string())
    }

    /// Resource replacement invalidates render caches and sends only metrics to Session.
    pub(super) fn set_text_context(&mut self, context: &TextContext) -> Result<(), String> {
        if let Some(builder) = &mut self.render.builder {
            // Different sources may share metrics and atlas numbers; clear cache identity too.
            builder.set_context(context);
        }
        self.apply(SessionMessage::FontMetrics(puzzle_term::FontMetrics {
            cell: context.metrics(),
            font_size: context.font_size(),
            dpr: context.dpr(),
            atlas_generation: context.atlas_generation(),
        }))
    }

    /// Builds only published damage; errors demand a recapture without discarding uploads.
    pub(super) fn build_frame(
        &mut self,
        now: Instant,
        context: &mut TextContext,
    ) -> Result<Option<&mut puzzle_term::frame::RenderFrame>, String> {
        let builder = self
            .render
            .builder
            .get_or_insert_with(|| FrameBuilder::new(context));
        // Shared contexts can evict pages while another terminal is being built.
        // The generation check invalidates this terminal's retained shape handles.
        builder.sync_generation(context.atlas_generation());
        // Copy only rows whose identity/version differ from this reusable slot.
        // Cursor, palette and overlay states are sealed with those same rows.
        let outcome = self.session.publish_frame(
            now,
            &mut self.render.snapshot,
            context.atlas_generation(),
            builder.style_epoch(),
        )?;
        // An idle publication neither consumes prepared uploads nor submits an image.
        if outcome == PublishOutcome::NothingNew {
            return Ok(None);
        }
        // All glyph admission still follows the unchanged browser text-context policy.
        if let Err(error) =
            builder.build_published(context, &mut self.render.snapshot, &mut self.render.frame)
        {
            // Damage was sealed before drawing; force recapture after a failed build.
            self.apply(SessionMessage::FrameResult(FrameResult::BuildFailed))?;
            return Err(error);
        }
        Ok(Some(&mut self.render.frame))
    }

    /// Commits hit targets only after the renderer submitted this frame successfully.
    pub(super) fn frame_presented(&mut self) -> Result<(), String> {
        if let Some(builder) = &mut self.render.builder {
            builder.capture_presented_regions(&mut self.render.regions);
        }
        // Skipped or failed presentations never reach this completed-frame event path.
        // Keep counters and hit regions in the same single-threaded Session order.
        let stats = self
            .render
            .builder
            .as_mut()
            .and_then(|builder| builder.take_frame_statistics());
        if let Some(stats) = stats {
            self.apply(SessionMessage::FrameResult(FrameResult::Statistics(stats)))?;
        }
        // Move the table once; Session returns the preceding allocation in its outbox.
        // Pointer routing therefore cannot see regions from an unsubmitted frame.
        let regions = std::mem::take(&mut self.render.regions);
        self.apply(SessionMessage::FrameResult(FrameResult::Presented(regions)))
    }
}
