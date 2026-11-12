//! Single-threaded test host owns drawing independently of the Session (D-50).
//!
//! Tests can edit source state through Deref, but every draw goes through the
//! same sealed publication API as desktop and web. No cache is lent or returned.
use super::*;
use crate::{
    FrameResult, HostEvent, PublishOutcome, SessionMessage, frame_snapshot::FrameSnapshot,
};
use std::ops::{Deref, DerefMut};

/// Independent source and drawing owners, composed only by this test fixture.
pub(super) struct TestHost {
    /// Mutable VT and models have exactly one owner.
    pub(super) session: Session,
    /// Font-independent copy history survives every draw.
    pub(super) snapshot: FrameSnapshot,
    /// Retained drawing caches live with the host, never inside Session.
    pub(super) builder: Option<FrameBuilder>,
    /// GPU instance arrays retain capacity across skipped presentations.
    frame: RenderFrame,
    /// The previous presented hit table returns here for reuse.
    regions: puzzle_overlay::regions::PresentedRegions,
}

impl TestHost {
    /// Constructs independent owners without inventing font-ready source state.
    pub(super) fn new(config: SessionConfig, services: Services) -> Self {
        Self::from_session(Session::new(config, services))
    }
    /// Attaches test drawing storage to an already configured source owner.
    pub(super) fn from_session(session: Session) -> Self {
        Self {
            session,
            snapshot: Default::default(),
            builder: None,
            frame: Default::default(),
            regions: Default::default(),
        }
    }
    /// Parser-only scheduling tests inject metrics without loading font outlines.
    pub(super) fn publish(&mut self, now: Instant) -> Option<()> {
        if self.session.frame_policy.is_none() {
            // Only this synthetic host installs geometry; production waits for real metrics.
            let metrics = puzzle_text::metrics::CellMetrics {
                width_px: 8,
                height_px: 16,
                baseline_px: 12,
                device_size: 14.0,
                cap_snapped: false,
                underline_px: 1,
            };
            self.session.frame_policy = Some(crate::frame_builder::policy::FramePolicy::new(
                crate::FontMetrics {
                    cell: metrics,
                    font_size: 14.0,
                    dpr: 1.0,
                    atlas_generation: 0,
                },
                self.session.cursor_settings,
            ));
        }
        // Advance timers and seal rows even when the test does not need glyphs.
        (self
            .session
            .publish_frame(now, &mut self.snapshot, 0, 0)
            .expect("publication")
            == PublishOutcome::Published)
            .then_some(())
    }
    /// Draws only sealed source values with permanently host-owned row caches.
    pub(super) fn draw(
        &mut self,
        now: Instant,
        context: &mut TextContext,
    ) -> Option<&mut RenderFrame> {
        let builder = self
            .builder
            .get_or_insert_with(|| FrameBuilder::new(context));
        builder.sync_generation(context.atlas_generation());
        if self
            .session
            .publish_frame(
                now,
                &mut self.snapshot,
                context.atlas_generation(),
                builder.style_epoch(),
            )
            .expect("publication")
            == PublishOutcome::NothingNew
        {
            return None;
        }
        // The model owner is no longer borrowed while shape and glyph caches are touched.
        builder
            .build_published(context, &mut self.snapshot, &mut self.frame)
            .expect("draw");
        Some(&mut self.frame)
    }
    /// Commits candidate regions only when the test explicitly presents its image.
    pub(super) fn presented(&mut self) {
        if let Some(builder) = &self.builder {
            builder.capture_presented_regions(&mut self.regions);
        }
        self.session
            .apply(
                SessionMessage::FrameResult(FrameResult::Presented(std::mem::take(
                    &mut self.regions,
                ))),
                Instant::now(),
            )
            .expect("presentation");
        // Hit-table storage is recycled through the same event queue as live hosts.
        while let Some(event) = self.session.take_event() {
            if let HostEvent::RegionsReturned(regions) = event {
                self.regions = regions;
            }
        }
    }
}

impl Deref for TestHost {
    type Target = Session;
    fn deref(&self) -> &Session {
        &self.session
    }
}
impl DerefMut for TestHost {
    fn deref_mut(&mut self) -> &mut Session {
        &mut self.session
    }
}
