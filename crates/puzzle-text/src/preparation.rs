//! Owned cold-glyph work crosses a host scheduler, never a core thread or lock.
//!
//! Font parsing and hint-engine initialization run outside a display frame.
//! An already parsed face may produce one measured, admitted shape inline.
//! Paths, quadratics and bands have one owner and are transferred into the
//! live atlas; no validation shape is prepared solely to be discarded.
use crate::{
    FontData,
    glyph::{GlyphKey, GlyphPlacement},
};
use puzzle_slug::{
    bands::{BandBuilder, BandSet},
    shape::QuadShape,
};
use std::time::Duration;
/// Parsed ownership is shared by native workers and browser idle tasks.
pub mod faces;

/// Source parameters independent of a table borrow or device address.
#[derive(Clone)]
pub struct Job {
    /// Actual placement also identifies a size-dependent hinted outline.
    pub placement: GlyphPlacement,
    /// Immutable bytes share the existing loaded font allocation.
    pub data: Option<FontData>,
    /// Overflow fitting and synthesis are retained if the source row disappears.
    pub style: [bool; 3],
    /// Primary style instances bind both engines to this weight.
    pub weight: u16,
}
/// Converted geometry ready for atlas packing, with a path for pressure masks.
pub struct Outline {
    /// The validated outline also supplies the device-size CPU oracle.
    pub path: kurbo::BezPath,
    /// Curve reduction is performed once, on the admitted preparation path.
    pub shape: QuadShape,
    /// Band choice and curve membership belong to that same preparation.
    pub bands: BandSet,
}
impl Outline {
    /// Completes size-independent Slug work without allocating any atlas addresses.
    pub fn new(path: kurbo::BezPath) -> Result<Option<Self>, String> {
        let shape = match QuadShape::from_path(&path, puzzle_slug::shape::DEFAULT_TOLERANCE) {
            Ok(shape) => shape,
            // Contextual space components own layout cells but have no ink.
            Err(puzzle_slug::shape::ShapeError::Empty) => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        let bands = BandBuilder.build(&shape, 16).map_err(|e| e.to_string())?;
        Ok(Some(Self { path, shape, bands }))
    }
}
/// Owned loaded-font ink, selected once before any atlas addresses are allocated.
pub enum Ink {
    /// Ordinary geometry is already reduced and banded for packing.
    Outline(Outline),
    /// Authored color graphs are always traversed off the display frame.
    Color(crate::color::ColorLayers),
}
/// A completed job contains either real ink or an authoritative empty component.
pub struct Completion {
    /// Original key prevents another font context consuming this result.
    pub placement: GlyphPlacement,
    /// Original fit, italic and bold determine device-size overflow pixels.
    pub style: [bool; 3],
    /// Every path is transferred once; an error retains its source report.
    pub result: Result<Option<Ink>, String>,
}
/// Platform-owned scheduling with nonblocking frame entry points (R9).
pub trait Driver: Send {
    /// Starts the same host-derived allowance as platform fallback preparation.
    fn begin_frame(&mut self, allowance: Duration);
    /// Attempts only a parsed, bounded face whose measured cost fits the allowance.
    fn prepare_now(&mut self, job: &Job) -> Option<Result<Option<Ink>, String>>;
    /// Transfers a cold job to a bounded worker or browser idle task.
    fn submit(&mut self, job: Job) -> bool;
    /// Transfers already completed work without waiting for it.
    fn poll(&mut self) -> Option<Completion>;
    /// Ready work, rather than outstanding work, requests an idle frame.
    fn ready(&mut self) -> bool;
    /// Non-frame observers can await accepted jobs without forcing them inline.
    fn pending(&self) -> bool;
    /// Actual admitted work is excluded from the ordinary frame predictor.
    fn frame_time(&self) -> Duration;
}
/// An indivisible call must fit strictly before the host's admission endpoint.
/// Hosts supply time left after ordinary work and D-33's wake margin.
pub fn fits(remaining: Duration, predicted: Duration) -> bool {
    remaining > predicted
}

/// Loaded-face work is keyed by a complete glyph identity, including hinting.
pub(crate) struct Pending {
    /// Authoritative empty components are facts, never queued again for each frame.
    pub(crate) empty: std::collections::HashSet<GlyphKey>,
    /// Only accepted jobs suppress duplicates; a refused submission remains retryable.
    pub(crate) keys: std::collections::HashSet<GlyphKey>,
    /// The platform owns scheduling and parsed face lifetimes.
    pub(crate) driver: Box<dyn Driver>,
}

/// Recent percentile evidence shared by platform-owned admission endpoints.
pub mod cost;
