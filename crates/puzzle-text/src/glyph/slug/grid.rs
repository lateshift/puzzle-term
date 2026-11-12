//! D-47 chooses storage bytes without changing the Slug outline or cell placement.
//!
//! Only a desktop host enables this policy. Effects, synthetic shear and color
//! graphs remain vectors; ordinary masks are keyed separately by device geometry.
//! The comparison includes the raster shelf's transparent gutter and deduplicated
//! Slug band lists, rather than assuming that every glyph has CJK complexity.
//! Outline identity and bitmap identity have different lifetimes. The font key
//! names em-space geometry; the raster key additionally names the fitted device
//! size, baseline, span and DPR. A foreground change never changes either outline.
//! Coverage is retained without RGB so cursor and selection recoloring remain cheap.
//! A shear introduced by missing italic is still a transform and selects vectors.
//! A real italic face is just another outline and may use the ordinary mask cache.
//! Procedural components remain vectors because their independent cell-edge geometry
//! must not be fitted part by part. COLR graphs keep their existing layer ordering.
//! Oversized crops decline mask storage before touching a fixed raster shelf.
//! The byte comparison counts the Slug contour's shared endpoints rather than two
//! complete texels per curve; disconnected contours need their own final endpoint.
//! Identical band lists share data while header records still exist for each band.
//! Padding differs by format: vector rows obey C1/B1/B3 and masks have a one-texel gutter.
use crate::{
    glyph::{GlyphPlacement, RendererChoice},
    preparation::Outline,
};

/// Explicit host opt-in keeps browser admission and packs on their existing path.
#[derive(Default)]
pub(super) struct Policy {
    /// None means that this context has no desktop bitmap selection owner.
    choice: Option<RendererChoice>,
}

impl Policy {
    /// Whether this context has the desktop storage policy rather than web admission.
    pub(super) fn enabled(&self) -> bool {
        self.choice.is_some()
    }
    /// Explicit Slug selection cannot borrow masks left by an earlier Auto setting.
    pub(super) fn masks_enabled(&self) -> bool {
        matches!(
            self.choice,
            Some(RendererChoice::Auto | RendererChoice::Atlas)
        )
    }
    /// Reports whether this untransformed glyph consumes fewer bytes as coverage.
    pub(super) fn bitmap(&self, p: &GlyphPlacement, ink: &Outline, style: [bool; 3]) -> bool {
        self.bitmap_bytes(p, ink.shape.bbox, vector_bytes(ink), style)
    }
    /// Cached vector metadata permits a later smaller size to choose coverage without rebanding.
    fn bitmap_bytes(
        &self,
        p: &GlyphPlacement,
        bbox: [f32; 4],
        bytes: usize,
        style: [bool; 3],
    ) -> bool {
        if p.effect
            || style[1]
            || p.color_glyph
            || matches!(
                p.key.face,
                crate::builtin::BUILTIN_FACE
                    | crate::builtin::TOFU_FACE
                    | crate::builtin::TOFU_COMPONENT_FACE
            )
        {
            // A synthesized shear is a transform, even without an overlay effect flag.
            return false;
        }
        if !matches!(
            self.choice,
            Some(RendererChoice::Auto | RendererChoice::Atlas)
        ) {
            return false;
        }
        let Ok(grid) = crate::glyph::oracle::geometry::SampleGrid::grid(p, bbox, style) else {
            return false;
        };
        // Permanent nearest-only grid crops need no interpolation gutter.
        let bitmap = grid.width as usize * grid.height as usize;
        // Very large masks fall back to vectors before asking a fixed 2048² page to fit.
        self.choice == Some(RendererChoice::Atlas) || bitmap < bytes
    }
}

/// Packed size from an empty block, including C1/B1/B3 row padding and deduplication.
pub(super) fn vector_bytes(ink: &Outline) -> usize {
    // One shared endpoint per connected curve is sufficient; contour ends add a texel.
    let mut curves = 0;
    for contour in &ink.shape.contours {
        for index in contour.clone() {
            // C1 prevents the second texture read from wrapping at column 4095.
            if curves % 4096 == 4095 {
                curves += 1;
            }
            // A contour-end point cannot be shared with the following disconnected contour.
            let connected = index + 1 < contour.end && curves % 4096 + 1 < 4095;
            curves += if connected { 1 } else { 2 };
        }
    }
    // B1 emits one header even for an empty or duplicate band.
    let lists = ink.bands.horizontal.iter().chain(&ink.bands.vertical);
    let mut bands = ink.bands.horizontal.len() + ink.bands.vertical.len();
    for (index, list) in lists.clone().enumerate() {
        // Identical lists share one address, exactly as the Slug band packer does.
        if list.is_empty() || lists.clone().take(index).any(|prior| prior == list) {
            continue;
        }
        // B3 keeps each unique list inside a single texture row.
        if bands % 4096 + list.len() > 4096 {
            bands = bands.div_ceil(4096) * 4096;
        }
        bands += list.len();
    }
    // Rgba32Float and Rg16Uint have different byte strides.
    curves * 16 + bands * 4
}

impl super::SlugBackend {
    /// Identifies native grid cache policy for source-validation ownership.
    pub fn grid_cache_enabled(&self) -> bool {
        self.grid.enabled()
    }
    /// Whether an ordinary request can use retained outline coverage at this host setting.
    pub fn grid_masks_enabled(&self) -> bool {
        self.grid.masks_enabled()
    }
    /// Selects grid storage; callers invalidate cached rows when changing this policy.
    pub fn set_grid_renderer(&mut self, choice: RendererChoice) {
        // This setting affects future ordinary preparations, never free-text vector keys.
        self.grid.choice = Some(choice);
    }
}

impl super::SlugBackend {
    /// A cached outline remains reusable only when its bytes beat this grid crop.
    pub fn get_for_placement(
        &mut self,
        p: &GlyphPlacement,
        style: [bool; 3],
    ) -> Option<puzzle_slug::atlas::ShapeHandle> {
        let result = self.get(p.key).filter(|handle| {
            // Byte comparisons only select an optional grid mask. Vector-only contexts
            // used to hash the same glyph a second time for an unusable size descriptor,
            // once per cell on every full rebuild. A miss needs no descriptor either.
            if !self.grid.masks_enabled() {
                return true;
            }
            // Pack/procedural entries lack descriptors and keep their vector handles.
            let bytes = self.vector_sizes.get(&p.key).copied().unwrap_or(0);
            !self.grid.bitmap_bytes(p, handle.bbox, bytes, style)
        });
        // Count usable placement handles rather than a vector declined by storage policy.
        crate::cache_metrics::CacheMetrics::lookup(&mut self.metrics, result.is_some(), 0);
        result
    }
}
