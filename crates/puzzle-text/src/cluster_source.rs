//! Platform-neutral sources for complete missing clusters (design §8.3).
//!
//! The host supplies these implementations: browser Canvas2D, or desktop
//! font discovery. Both receive the same immutable grid placement. They
//! may choose different ink, but never change cells or the terminal cursor.
use crate::{
    font_set::FontStyle,
    glyph::{GlyphPlacement, RasterGlyph},
    presentation::Presentation,
};

/// A borrowed cluster request; selectors remain available to the source.
#[derive(Clone, Copy)]
pub struct ClusterRequest<'a> {
    /// Complete UTF-8 grapheme, never a standalone default-ignorable scalar.
    pub cluster: &'a str,
    /// Primary style class, independent of the source's font family.
    pub style: FontStyle,
    /// Requested text or emoji appearance, resolved before source selection.
    pub presentation: Presentation,
}

/// Ink returned through the vector/bitmap backend seam (D-28).
pub enum ClusterGlyph {
    /// Complete lookup found no drawable font; this final box never awaits a wake.
    Missing(u32),
    /// Discovery is queued; render a hex box without caching it under this source key.
    Pending(u32),
    /// Size-independent em-space outline, retaining the grid baseline.
    Outline(kurbo::BezPath),
    /// Converted, banded geometry transfers from a host worker without repeat preparation.
    PreparedOutline {
        /// One validated shape supplies vector packing and device-size overflow sampling.
        ink: crate::preparation::Outline,
        /// Missing native bold retains the ordinary synthetic-weight policy.
        bold: bool,
        /// Missing native italic retains the ordinary instance-shear policy.
        italic: bool,
    },
    /// A matched system outline with the font's missing-style synthesis flags.
    StyledOutline {
        /// Borrowed font data has already become owned em-space geometry.
        path: kurbo::BezPath,
        /// Missing bold is drawn twice with the specified device-pixel offset.
        bold: bool,
        /// Missing italic is a 12-degree instance shear, preserving the path.
        italic: bool,
    },
    /// D-27 retains its unhinted validation outline alongside the once-prepared hinted draw.
    ValidatedOutline {
        /// Size-independent validation ink becomes an ordinary unhinted atlas entry.
        unhinted: crate::preparation::Outline,
        /// Pixel-fitted outline prepared once for the requested device size.
        hinted: crate::preparation::Outline,
        /// Missing native bold is synthesized by the ordinary backend policy.
        bold: bool,
        /// Missing native italic is synthesized without altering validation geometry.
        italic: bool,
    },
    /// Device-size samples; raster atlas ownership remains with the host context.
    Bitmap(RasterGlyph),
    /// Complete ordered COLR graph, whose fonts can be released after traversal.
    Color(crate::color::ColorLayers),
}

/// Emoji source, tried first for emoji and last for explicit text appearance.
pub trait EmojiSource: Send {
    /// Allows cold work only inside the host's measured remaining frame allowance.
    fn begin_frame(&mut self, _allowance: std::time::Duration) {}
    /// Actual synchronous source work is excluded from ordinary-frame cost prediction.
    fn frame_preparation_time(&self) -> std::time::Duration {
        std::time::Duration::ZERO
    }
    /// Reports already completed work without waiting or starting a polling timer.
    fn ready(&mut self) -> bool {
        false
    }
    /// Allows an asynchronous source to validate its first real draw instead of
    /// extracting and discarding an outline merely to answer a coverage query.
    /// True reserves a provisional source identity; prepare may still return Pending.
    fn may_prepare(&mut self, request: ClusterRequest<'_>) -> bool {
        self.can_draw(request)
    }
    /// Drains completed platform work; true invalidates provisional row ink.
    fn poll_changed(&mut self) -> bool {
        false
    }
    /// Pending work is observable to non-frame probes without waiting for discovery.
    fn pending(&self) -> bool {
        false
    }
    /// Transfers one completed variant even when its original cell has scrolled away.
    /// Synchronous sources have no queued ownership and keep the default empty lane.
    fn take_ready(&mut self) -> Option<(GlyphPlacement, ClusterGlyph)> {
        None
    }
    /// Reports coverage for this whole grapheme and style, not its base alone.
    fn can_draw(&mut self, request: ClusterRequest<'_>) -> bool;
    /// Produces ink for a stable placement, or rejects malformed font data.
    fn prepare(
        &mut self,
        request: ClusterRequest<'_>,
        placement: &GlyphPlacement,
    ) -> Result<ClusterGlyph, String>;
}

/// Last platform fallback before builtin tofu (browser fonts or fontique).
pub trait PlatformFallback: Send {
    /// Supplies refresh slack without introducing a clock or scheduling policy into core.
    fn begin_frame(&mut self, _allowance: std::time::Duration) {}
    /// Actual synchronous source work is excluded from ordinary-frame cost prediction.
    fn frame_preparation_time(&self) -> std::time::Duration {
        std::time::Duration::ZERO
    }
    /// A ready result requests a frame even if application output has stopped.
    fn ready(&mut self) -> bool {
        false
    }
    /// Provisional source admission is distinct from confirmed drawable coverage.
    /// Synchronous sources retain their existing authoritative coverage contract.
    fn may_prepare(&mut self, request: ClusterRequest<'_>) -> bool {
        self.can_draw(request)
    }
    /// Drains completed platform work; true invalidates provisional row ink.
    fn poll_changed(&mut self) -> bool {
        false
    }
    /// Non-frame probes can await asynchronous results without changing live scheduling.
    fn pending(&self) -> bool {
        false
    }
    /// Moves completed ink into the host atlas independently of current visibility.
    /// The placement retains the exact device geometry and original source identity.
    fn take_ready(&mut self) -> Option<(GlyphPlacement, ClusterGlyph)> {
        None
    }
    /// Reports whether the environment can draw the complete requested cluster.
    fn can_draw(&mut self, request: ClusterRequest<'_>) -> bool;
    /// Produces vector or raster ink without changing the supplied placement.
    fn prepare(
        &mut self,
        request: ClusterRequest<'_>,
        placement: &GlyphPlacement,
    ) -> Result<ClusterGlyph, String>;
}

/// One owner supplies both complete policies without synchronization.
pub mod synchronous;

/// Emoji placements use a reserved identity outside the loaded face vector.
pub const EMOJI_FACE: u32 = u32::MAX - 3;
/// System fallback placements cannot collide with bundled face identifiers.
pub const PLATFORM_FACE: u32 = u32::MAX - 2;
