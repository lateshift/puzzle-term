//! Deferred cold font work must leave a visible provisional border in its cell.
use super::*;
use puzzle_text::preparation::{Completion, Driver, Ink, Job};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

/// A zero allowance refuses every cold glyph, including diagnostic components.
#[test]
fn declined_cold_glyph_draws_a_provisional_box() {
    let fonts = FontSet::new(
        FontData::new(fixture("fonts/JetBrainsMono-Variable.ttf"), 0),
        None,
        None,
    )
    .unwrap();
    let mut context = TextContext::new(fonts, 14.0, 1.0).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    context
        .set_preparation_driver(Box::new(Decline(calls.clone())))
        .unwrap();
    context.set_completion_budget(Duration::ZERO);
    let mut builder = FrameBuilder::new(&context);
    let mut vt = Terminal::new(8, 2, 0, 0);
    vt.advance("\x1b[?25lΩ".as_bytes());
    let mut frame = RenderFrame::default();
    builder.build(&mut context, &vt, &mut frame).unwrap();
    assert!(
        calls.load(Ordering::Relaxed) > 1,
        "both font and border preparation were declined"
    );
    assert!(frame.slugs.is_empty() && frame.rasters.is_empty());
    let borders: Vec<_> = frame
        .rects
        .iter()
        .filter(|r| r.stroke_width > 0.0)
        .collect();
    assert_eq!(borders.len(), 1, "deferred glyph cell is blank");
    assert_eq!(
        borders[0].rect,
        [
            16.0,
            16.0,
            context.metrics.width_px as f32,
            context.metrics.height_px as f32
        ]
    );
    assert_eq!(borders[0].stroke_width, 1.0);
    assert_ne!(borders[0].color >> 24, 0, "provisional ink is transparent");
}

/// Counts cold requests while refusing ownership and producing no ready results.
pub(super) struct Decline(
    /// Counts only requests that reached the cold preparation owner.
    pub(super) Arc<AtomicUsize>,
);
/// All scheduler operations retain the deliberately empty injected allowance.
impl Driver for Decline {
    /// No inline allowance exists in this injected host.
    fn begin_frame(&mut self, _: Duration) {}
    /// Every cold call remains inadmissible.
    fn prepare_now(&mut self, _: &Job) -> Option<Result<Option<Ink>, String>> {
        None
    }
    /// A full worker lane refuses ownership without losing the requested cell.
    fn submit(&mut self, _: Job) -> bool {
        self.0.fetch_add(1, Ordering::Relaxed);
        false
    }
    /// No completed geometry can hide the provisional-border branch.
    fn poll(&mut self) -> Option<Completion> {
        None
    }
    /// Ready events are absent; this witness does not request polling frames.
    fn ready(&mut self) -> bool {
        false
    }
    /// The worker never accepts a job.
    fn pending(&self) -> bool {
        false
    }
    /// Declined work consumes no elastic preparation time.
    fn frame_time(&self) -> Duration {
        Duration::ZERO
    }
}
