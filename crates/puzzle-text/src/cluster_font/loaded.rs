//! Persistent system-face parsing state, separate from its mapped-byte owner.
//! A platform retains this metadata alongside the mapping that supplied it.
//! Table views borrow that mapping briefly; shaping plans, cmap selection and
//! variations are initialized on the worker and reused without a byte copy.
use super::*;

/// Parsed metadata for one matched face and variation instance.
pub struct Face {
    /// Collection index is validated when the mapping first becomes resident.
    index: u32,
    /// Cmap selection avoids repeating first-use subtable discovery inline.
    cmap: skrifa::charmap::MappingIndex,
    /// Owned shaping tables survive short borrowed FontRef views.
    data: ShaperData,
    /// Shaping and drawing use exactly the same native variation coordinates.
    instance: ShaperInstance,
    /// Outline drawing reuses its normalized location.
    location: skrifa::instance::Location,
    /// Authored images/paint graphs have unpredictable decode/traversal cost.
    outline_only: bool,
}
/// A face owns parsing metadata; callers continue owning and bounding its mapped bytes.
impl Face {
    /// Initializes all parsing metadata off-frame without retaining borrowed tables.
    pub fn new(
        bytes: &[u8],
        index: u32,
        request: ClusterRequest<'_>,
        variations: &[([u8; 4], f32)],
    ) -> Result<Self, String> {
        // Validate the collection offset before deriving any table or variation view.
        let font = FontRef::from_index(bytes, index).map_err(|e| e.to_string())?;
        // Weight and slant must match the policy that selected this resident face.
        let settings = settings(request.style, variations);
        // These owned indices/plans refer to offsets; the host keeps the bytes mapped.
        Ok(Self {
            index,
            // MappingIndex stores table selection, not a borrow into the mapping.
            cmap: skrifa::charmap::MappingIndex::new(&font),
            data: ShaperData::new(&font),
            // Reuse the shaping instance instead of reconstructing variation state per job.
            instance: ShaperInstance::from_variations(
                &font,
                settings
                    .iter()
                    .map(|(tag, value)| (harfrust::Tag::new(tag), *value)),
            ),
            // The outline location uses the same axes, preserving validation/draw identity.
            location: font.axes().location(
                settings
                    .iter()
                    .map(|(tag, value)| (skrifa::Tag::new(tag), *value)),
            ),
            // Color-table discovery is paid once; decode cost is never assumed bounded.
            outline_only: !has_color_tables(bytes, index),
        })
    }
    /// Inline work excludes first hinter setup and complex joined shaping.
    /// Multi-scalar clusters and authored color always use background preparation.
    pub fn inline_safe(&self, request: ClusterRequest<'_>, p: &GlyphPlacement) -> bool {
        // A single unhinted scalar needs neither joined shaping nor first hinter setup.
        self.outline_only && p.key.hint_ppem == 0 && request.cluster.chars().count() == 1
    }
    /// Validates coverage from the already selected cmap without producing discarded ink.
    pub fn covers(&self, bytes: &[u8], cluster: &str) -> bool {
        let Ok(font) = FontRef::from_index(bytes, self.index) else {
            return false;
        };
        // A short borrowed view reads the already resident bytes without remapping them.
        let cmap = self.cmap.charmap(&font);
        cluster
            .chars()
            // Selectors and joiners influence a cluster but require no separate drawable ink.
            .filter(|c| !crate::font_set::is_default_ignorable(*c))
            // Glyph zero is the font's missing-character entry, not successful coverage.
            .all(|c| cmap.map(c).is_some_and(|g| g.to_u32() != 0))
    }
    /// Extracts the same validated ink that is transferred into the draw store.
    pub fn prepare(
        &self,
        bytes: &[u8],
        request: ClusterRequest<'_>,
        p: &GlyphPlacement,
        decoder: &mut dyn crate::bitmap::PngDecoder,
        baseline: f32,
    ) -> Result<ClusterGlyph, String> {
        let font = FontRef::from_index(bytes, self.index).map_err(|e| e.to_string())?;
        // Shaping borrows the persistent tables and instance, returning owned positions.
        let glyphs = shaped(&font, request.cluster, &self.data, &self.instance)?;
        if !self.outline_only && glyphs.len() == 1 {
            // Structural validation preceded this draw-size decode; no image is decoded twice.
            let gid = glyphs[0].0;
            if let Some(image) =
                crate::bitmap::load_bytes(bytes, self.index, gid, p.size_px, decoder)?
            {
                return crate::bitmap::fitted(image, p, baseline).map(ClusterGlyph::Bitmap);
            }
            // A color graph becomes the drawn graph; validation does not discard it.
            if let Some(color) =
                crate::color::borrowed_layers(&font, &self.location, gid, 0xffffffff)?
            {
                return Ok(ClusterGlyph::Color(color));
            }
        }
        // Unhinted em-space geometry remains reusable at every later device size.
        outlines(&font, &self.location, &glyphs, p).map(ClusterGlyph::Outline)
    }
}
