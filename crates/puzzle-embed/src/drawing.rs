//! Single-threaded diagnostic drawing uses the D-50 publication seam.
//!
//! The interactive desktop uses channel endpoints. Headless scenarios can keep
//! source and drawing on one thread, but retain separate owners and sealed slots.
//! This helper owns no Session and never lends its drawing cache to one.
//!
//! Publication and presentation are separate operations. A scenario may draw
//! repeatedly before taking a screenshot, so glyph uploads remain in its frame
//! until the GPU executor acknowledges them. The same distinction keeps pointer
//! regions attached to the last successful submission instead of the latest build.
//! Source capture owns blink, search and interaction state; shaping sees only the
//! sealed slot and can never mix newly fed cells into an earlier publication.
//! Font readiness is authoritative: parser-only calls use publication without
//! creating a builder, while drawing initializes one from real measured metrics.
//! Numeric scenarios have no event-loop receiver, so they consume host facts
//! locally after each turn. Bus observers still see the original facts.
use puzzle_term::{
    FrameResult, HostEvent, PublishOutcome, Session, SessionMessage,
    frame::RenderFrame,
    frame_builder::{FrameBuilder, context::TextContext},
    frame_snapshot::FrameSnapshot,
};
use std::time::Instant;

/// Diagnostic host storage survives all authored feed/input/presentation turns.
#[derive(Default)]
pub struct Drawing {
    /// Initialized when real font metrics become available.
    builder: Option<FrameBuilder>,
    /// Copy history is independent of the retained row-instance history.
    slot: FrameSnapshot,
    /// Pending uploads survive skipped GPU submission, as in the live shell.
    frame: RenderFrame,
    /// Region allocations circulate through the same source event queue.
    regions: puzzle_term::overlay::regions::PresentedRegions,
}
impl Drawing {
    /// Publishes and builds one sealed diagnostic frame, never reaching VT during shaping.
    pub fn draw(
        &mut self,
        session: &mut Session,
        now: Instant,
        context: &mut TextContext,
    ) -> Result<Option<&mut RenderFrame>, String> {
        // Keep row geometry between scenario turns rather than rebuilding a cold cache.
        let builder = self
            .builder
            .get_or_insert_with(|| FrameBuilder::new(context));
        // Device recovery changes handle generations while source row identities survive.
        builder.sync_generation(context.atlas_generation());
        if session.publish_frame(
            now,
            &mut self.slot,
            context.atlas_generation(),
            builder.style_epoch(),
        )? == PublishOutcome::NothingNew
        {
            // An idle publication must still release change-only UI facts.
            self.drain_events(session);
            return Ok(None);
        }
        // A failure revises copy history for the next attempt without dropping uploads.
        if let Err(error) = builder.build_published(context, &mut self.slot, &mut self.frame) {
            session.apply(SessionMessage::FrameResult(FrameResult::BuildFailed), now)?;
            return Err(error);
        }
        // Numeric/headless hosts consume UI facts locally instead of retaining an unbounded outbox.
        self.drain_events(session);
        Ok(Some(&mut self.frame))
    }
    /// Advances parser-only scenarios without pretending fonts are ready for drawing.
    pub fn publish(
        &mut self,
        session: &mut Session,
        now: Instant,
    ) -> Result<PublishOutcome, String> {
        let outcome = session.publish_frame(now, &mut self.slot, 0, 0)?;
        self.drain_events(session);
        Ok(outcome)
    }
    /// Only actually submitted frames replace hit targets on the model owner.
    pub fn presented(&mut self, session: &mut Session, now: Instant) -> Result<(), String> {
        if let Some(builder) = &mut self.builder {
            builder.capture_presented_regions(&mut self.regions);
            if let Some(stats) = builder.take_frame_statistics() {
                session.apply(
                    SessionMessage::FrameResult(FrameResult::Statistics(stats)),
                    now,
                )?;
            }
        }
        // Returning the table transfers its allocation to the model owner by value.
        session.apply(
            SessionMessage::FrameResult(FrameResult::Presented(std::mem::take(&mut self.regions))),
            now,
        )?;
        self.drain_events(session);
        Ok(())
    }
    /// UI observations are drained while reusable region buffers return to this host.
    fn drain_events(&mut self, session: &mut Session) {
        // The scenario host consumes facts locally; the bus still exposes them to observers.
        while let Some(event) = session.take_event() {
            if let HostEvent::RegionsReturned(regions) = event {
                // Reuse the previous presented table instead of growing one per screenshot.
                self.regions = regions;
            }
        }
    }
}
