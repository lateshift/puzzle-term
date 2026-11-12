//! Declarative annotations and their frozen frame records (design §11.3).
//!
//! Session owns definitions and source lifetimes. The single built-in provider
//! sees immutable definitions plus sampled positions, never offscreen VT rows.
use crate::{
    anchor::{Anchor, Resolved},
    draw::{Effect, Paint},
    geometry::Rect,
};
use kurbo::Affine;
use puzzle_frame::ZSlot;
use std::sync::Arc;
/// The six SGR underline styles, shared by declarative annotations (§6.5).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnderlineStyle {
    /// No underline ink.
    None,
    /// One solid rule.
    Single,
    /// Two parallel rules.
    Double,
    /// Seamless quadratic wave.
    Curly,
    /// Rounded repeated dots.
    Dotted,
    /// Repeated straight dashes.
    Dashed,
}
/// Painter implementation for every declarative kind, using the public API.
pub mod render;

/// Stable annotation identity; removing one never redirects an old handle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecorationId(pub u64);
/// Natural text appearance independent of its placement beside the source.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    /// Logical pixel size, validated before a declaration is retained.
    pub size: f32,
    /// Optional loaded face identity; None uses ordinary primary/fallback faces.
    pub face: Option<u32>,
    /// Regular, bold, italic or bold-italic as 0..3.
    pub style: u8,
    /// Straight RGBA8 foreground.
    pub color: u32,
}
/// Natural annotation positions around the first/last source cells.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Placement {
    /// Starts after the final source cell.
    After,
    /// Ends before the first source cell.
    Before,
    /// Sits one line above the first source cell.
    Above,
    /// Sits one line below the first source cell.
    Below,
    /// Starts at the right edge of the last source line.
    EndOfLine,
}
/// SVG coordinates can fit source geometry or use an explicit logical affine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShapeFit {
    /// Fits into the first source cell, preserving aspect ratio.
    Cell,
    /// Fits into the bounding rectangle of visible source segments.
    Range,
    /// Applies an anchor-relative transform to the authored SVG coordinates.
    Custom(Affine),
}
/// One common annotation; complex extensions can still implement providers.
#[derive(Clone, Debug, PartialEq)]
pub enum DecorationKind {
    /// Cell-aligned background fill.
    Highlight {
        /// Annotation foreground packed as straight RGBA8.
        fill: u32,
    },
    /// Font-adjacent underline, including curly/dotted/dashed variants.
    Underline {
        /// VT's shared underline catalogue.
        style: UnderlineStyle,
        /// Annotation foreground packed as straight RGBA8.
        color: u32,
    },
    /// Inward one-device-pixel border around each visible source segment.
    Box {
        /// Border tint.
        stroke: u32,
        /// Logical corner radius.
        radius: f32,
    },
    /// Gutter label; icon glyphs use the same text/fallback pipeline.
    GutterBadge {
        /// UTF-8 label or bundled icon scalar.
        text: String,
        /// Annotation foreground packed as straight RGBA8.
        color: u32,
    },
    /// Free text without changing grid cells or application output.
    InlineText {
        /// Annotation text.
        text: String,
        /// Face, size and color.
        style: TextStyle,
        /// Position relative to source text.
        placement: Placement,
    },
    /// Immutable SVG fill through the same Slug path cache as providers.
    Shape {
        /// SVG path data, parsed once on API mutation.
        svg_path: String,
        /// Mapping from SVG to source geometry.
        fit: ShapeFit,
        /// Solid or gradient fill.
        paint: Paint,
    },
    /// Ink-only grid transformation; neutral geometry stays unchanged.
    GlyphEffect(Effect),
}
/// Optional region behaviour does not give an annotation keyboard focus.
#[derive(Clone, Debug, PartialEq)]
pub enum RegionSpec {
    /// Show a floating label while hovered; its button events consume input.
    Tooltip(String),
}
/// Public retained declaration; mutations go through Session's controller.
#[derive(Clone, Debug, PartialEq)]
pub struct Decoration {
    /// Tracked or fixed attachment, resolved by the Session model before publication.
    pub anchor: Anchor,
    /// Common painter behaviour.
    pub kind: DecorationKind,
    /// Explicit submission layer; gutter content normally uses Top.
    pub slot: ZSlot,
    /// Drop on eviction or a source-row rewrite; reflow alone preserves it.
    pub auto_remove: bool,
    /// Optional tooltip and pointer ownership.
    pub region: Option<RegionSpec>,
}
/// Copy-only frame record shares definitions without cloning annotation strings.
#[derive(Clone, Debug)]
pub struct Sample {
    /// Public annotation identity also names its provider-local hit region.
    pub id: DecorationId,
    /// Immutable source owned by Session; frame callbacks only borrow it.
    pub declaration: Arc<Decoration>,
    /// Marker positions sealed by the model for render-side view callbacks.
    pub resolved: Resolved,
    /// Parsed SVG retained by the definition's lifetime, never re-parsed per frame.
    pub shape: Option<crate::shapes::ShapeRef>,
}
/// Returns the union of row rectangles without requiring another allocation.
pub(super) fn union(rects: &[Rect]) -> Option<Rect> {
    let first = *rects.first()?;
    let mut lo = first.origin;
    let mut hi = [lo[0] + first.size[0], lo[1] + first.size[1]];
    for rect in &rects[1..] {
        // Range fitting uses visible geometry; no provider queries hidden rows.
        for axis in 0..2 {
            lo[axis] = lo[axis].min(rect.origin[axis]);
            hi[axis] = hi[axis].max(rect.origin[axis] + rect.size[axis]);
        }
    }
    Some(Rect {
        origin: lo,
        size: [hi[0] - lo[0], hi[1] - lo[1]],
    })
}
