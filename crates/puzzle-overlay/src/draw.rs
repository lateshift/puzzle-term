//! Declarative painter commands with anchor-relative logical coordinates (§11.2).
//!
//! Retained lists belong to providers, not the GPU. The composition layer
//! resolves their anchors and prepares text/shapes in its shared TextContext.
pub use crate::shapes::ShapeRef;
use crate::{
    anchor::{Anchor, Resolved},
    geometry::{Range, Rect},
};
use kurbo::Affine;
use puzzle_frame::ZSlot;
/// Packed straight RGBA8, optionally referring to a retained gradient row.
#[derive(Clone, Debug, PartialEq)]
pub struct Paint {
    /// Solid tint; foreground gradient stops use this tint too.
    pub color: u32,
    /// Optional immutable brush, prepared in the same table as COLR emoji.
    pub gradient: Option<std::sync::Arc<puzzle_slug::paint::Paint>>,
}
/// Filled or stroked rectangle in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectSpec {
    /// Anchor-relative bounds.
    pub rect: Rect,
    /// Straight RGBA8 tint.
    pub color: u32,
    /// Rounded corner radius; zero gives a sharp rectangle.
    pub radius: f32,
    /// Zero fills, positive values stroke inward.
    pub stroke: f32,
}
/// Free text request, shaped through the host's shared faces and fallback.
#[derive(Clone, Debug, PartialEq)]
pub struct TextSpec {
    /// UTF-8 content, owned only when this retained command changes.
    pub text: String,
    /// Logical font size.
    pub size: f32,
    /// Optional loaded face ID; None uses the primary style and normal fallback.
    pub face: Option<u32>,
    /// Regular/bold/italic/bold-italic as a compact 0..3 style class.
    pub style: u8,
    /// Straight RGBA8 foreground.
    pub color: u32,
    /// Logical affine transform after natural text layout.
    pub transform: Affine,
}
/// Ink-only changes; neutral grid placement and selection remain unchanged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Effect {
    /// Optional replacement straight RGBA8 ink.
    pub color: Option<u32>,
    /// Alpha multiplier, clamped to 0..1 by the renderer seam.
    pub opacity: f32,
    /// Cell-origin-relative ink transform (offset/scale/rotation/shear).
    pub transform: Affine,
    /// Suppress ink while preserving text and cells for selection/copy.
    pub hidden: bool,
}
impl Default for Effect {
    /// Keeps ink unchanged until an explicit effect is requested.
    fn default() -> Self {
        Self {
            color: None,
            opacity: 1.0,
            transform: Affine::IDENTITY,
            hidden: false,
        }
    }
}
/// Provider-local identity, independent of the global registry slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegionId(pub u64);
/// Portable pointer shapes mapped only by the platform shells.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CursorIcon {
    /// Normal terminal/text cursor policy.
    #[default]
    Default,
    /// Clickable annotation or button.
    Pointer,
    /// A draggable handle.
    Grab,
    /// Active handle drag.
    Grabbing,
    /// Editable query text.
    Text,
}
/// One painter operation, including the reserved M15 raster primitive.
#[derive(Clone, Debug, PartialEq)]
pub enum Primitive {
    /// Rect pipeline fill, stroke or rounded panel.
    Rect(RectSpec),
    /// Cached Slug shape with arbitrary affine placement.
    Shape(ShapeRef, Affine, Paint),
    /// Free text through the same shared fallback as grid text.
    Text(TextSpec),
    /// Immutable RGBA source and anchor-relative logical destination rectangle.
    Raster(puzzle_frame::RasterRef, Rect),
    /// Grid-ink transformation over an inclusive interval.
    GlyphEffect(Range, Effect),
    /// Pointer hit region; it draws no pixels.
    Region(Rect, RegionId, CursorIcon),
    /// Push an anchor-relative scissor, intersecting any previous clip.
    ClipPush(Rect),
    /// Restore the enclosing scissor.
    ClipPop,
}
/// A retained primitive and its explicit painter order and text lifetime.
#[derive(Clone, Debug, PartialEq)]
pub struct Draw {
    /// Text-following or fixed surface attachment.
    pub anchor: Anchor,
    /// Painter's order relative to grid and cursor slots.
    pub slot: ZSlot,
    /// Remove the content permanently when its tracked source becomes Gone.
    pub auto_remove: bool,
    /// Actual geometry or input operation.
    pub primitive: Primitive,
    /// The Session model seals these positions before the render view builds.
    pub resolved: Resolved,
}
/// Owned reusable list, rebuilt only when its provider is dirty or animating.
#[derive(Default)]
pub struct DrawList {
    /// Submission order is preserved within each z-slot.
    pub draws: Vec<Draw>,
}
impl DrawList {
    /// Appends a command; initial resolution waits for the live Session sample.
    pub fn push(&mut self, anchor: Anchor, slot: ZSlot, primitive: Primitive) {
        self.draws.push(Draw {
            anchor,
            slot,
            auto_remove: false,
            primitive,
            resolved: Resolved::Hidden,
        });
    }
}
