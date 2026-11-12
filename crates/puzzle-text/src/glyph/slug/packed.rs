//! Pack handles join ordinary shape identity and page lifetime before fonts load.
//!
//! Pinning unused pack glyphs prevents permanent missing ink during a slow font
//! download. A successful validated takeover releases the pins, after which
//! evicted glyphs are re-imported from the retained validated pack data.
//!
//! The full font supplies shaping tables, but never replaces pack-covered ink.
//! A styled placement uses the primary face slot as its default variation ID.
//! Normalization removes D-27 size/hint fields only for that exact variation.
//! Explicit nondefault variations must retain their own geometry and identity.
//! Recovery copies precomputed band membership and exact f32 curves; it does
//! not parse OpenType, extract outlines or run candidate-band selection.
//! Ordinary insertion keeps current-frame addresses protected. If every page
//! is protected, the same recovered geometry feeds the oracle bitmap backend.
use super::{GlyphKey, ShapeKey, SlugAtlas, SlugBackend};
use std::{collections::HashMap, sync::Arc};

/// One validated style pack retains exact geometry until the text resource dies.
pub(super) struct Source {
    /// Immutable CPU source survives page eviction without another font extraction.
    pub(super) pack: Arc<crate::pack::FontPack>,
    /// Glyph identity lookup also handles cmap aliases and shaped placements.
    glyphs: HashMap<u32, crate::pack::PackGlyph>,
}

/// Pack startup preserves ready-to-upload addresses, advances and glyph IDs.
impl SlugBackend {
    /// Seeds regular unhinted keys from the already validated first-frame pack.
    /// Its small directory joins ordinary shape keys without waiting for fonts.
    pub fn from_pack(pack: &crate::pack::FontPack) -> Result<Self, String> {
        Self::from_pack_with_budget(pack, puzzle_slug::atlas::DEFAULT_BUDGET_BYTES)
    }

    /// Uses a host's atlas budget, including a small budget for pressure tests.
    /// Every packed outline is registered once even when cmap aliases share it.
    /// The decoder already checked the source hash, texel extents and band headers.
    /// No glyph address changes during this registration, preserving first uploads.
    pub fn from_pack_with_budget(
        pack: &crate::pack::FontPack,
        budget: usize,
    ) -> Result<Self, String> {
        let mut backend = Self {
            // The regular startup plane already follows the packer's texture layout.
            atlas: SlugAtlas::from_texels_with_budget(&pack.curves, &pack.bands, budget)
                .map_err(|error| error.to_string())?,
            ..Self::default()
        };
        // Scalar aliases must resolve to one stable glyph identity, otherwise
        // last-use stamps could retain one alias while evicting another's ink.
        for glyph in pack.glyphs.values() {
            // Whitespace retains design advances without a shader-visible contour.
            let Some(handle) = glyph.handle else {
                continue;
            };
            let key = GlyphKey {
                face: 0,
                glyph_id: glyph.glyph_id,
                instance: 0,
                hint_ppem: 0,
                hint_mode: super::super::HintMode::None,
            };
            if backend.shapes.contains_key(&key) {
                continue;
            }
            // IDs use the same allocator as future font and procedural preparations.
            backend.next_shape = backend
                .next_shape
                .checked_add(1)
                .ok_or("Slug glyph identity space exhausted")?;
            let shape = ShapeKey(backend.next_shape);
            // Validate page ownership without decoding or retaining duplicate geometry.
            // The backend clock represents startup here; pins protect unused entries
            // until drawing supplies real row-batch stamps and takeover succeeds.
            backend
                .atlas
                .register_packed(shape, handle, backend.colors.frame)
                .map_err(|error| error.to_string())?;
            backend.shapes.insert(key, shape);
        }
        // Keep the validated source recoverable after pins are released (§8.8).
        backend.remember_pack(0, Arc::new(pack.clone()));
        Ok(backend)
    }

    /// Installs another approved style pack before startup's first presentation.
    /// Prepared bands transfer directly; no OpenType parsing or band search runs.
    pub fn add_pack(&mut self, face: u32, pack: crate::pack::FontPack) -> Result<(), String> {
        // Share the recovery owner with the glyph-ID index instead of retaining
        // a second independent copy for each startup placement or cmap alias.
        let pack = Arc::new(pack);
        self.remember_pack(face, pack.clone());
        // Spaces retain advances in the cmap but have no texture data to import.
        for glyph in pack.glyphs.values().filter(|g| g.handle.is_some()) {
            // These slots match FontSet's primary regular/bold/italic/bold-italic
            // variations, allowing Harfrust output to find the same packed ink.
            let key = GlyphKey {
                face,
                glyph_id: glyph.glyph_id,
                instance: face,
                hint_ppem: 0,
                hint_mode: super::super::HintMode::None,
            };
            // Multiple scalars can share a glyph ID; the atlas stores it only once.
            if self.get(key).is_some() {
                continue;
            }
            // The validated directory supplies all geometry and sorted band lists.
            let ink = pack.prepared(glyph)?.ok_or("pack ink absent")?;
            // Use the backend's common identity space so pack and runtime shapes
            // cannot collide when platform sources are replaced after takeover.
            let shape = ShapeKey(
                self.next_shape
                    .checked_add(1)
                    .ok_or("Slug glyph identity space exhausted")?,
            );
            // Rebase precomputed texels into a page pair without changing curves.
            let handle = self
                .atlas
                .prepare_quadratics(
                    shape,
                    ink.shape,
                    ink.bands,
                    puzzle_slug::atlas::FillRule::NonZero,
                    self.colors.frame,
                )
                .map_err(|e| e.to_string())?;
            // Registration pins even an already allocated style shape; no unused
            // style may lose its only ink source before full-font takeover.
            self.atlas
                .register_packed(shape, handle, self.colors.frame)
                .map_err(|e| e.to_string())?;
            // Publish the identity only after both allocation and pinning succeeded.
            self.next_shape = shape.0;
            self.shapes.insert(key, shape);
        }
        Ok(())
    }

    /// Validates every imported style against the exact full-font variation.
    pub fn validate_packs(&self, fonts: &crate::font_set::FontSet) -> Result<(), String> {
        for (face, source) in &self.packs {
            // Stable face slots alone cannot prove the selected font variation matches.
            let font = fonts.face(*face).ok_or("pack face absent")?;
            if source.pack.weight != font.weight() || !source.pack.matches_source(&font.data()) {
                return Err("font does not match style pack".into());
            }
        }
        Ok(())
    }

    /// Startup scalar mapping selects the same exact style as the eventual full font.
    pub fn packed_glyph(&self, face: u32, scalar: char) -> Option<crate::pack::PackGlyph> {
        // A missing optional style keeps the existing regular startup fallback.
        self.packs
            .get(&face)?
            .pack
            .glyphs
            .get(&(scalar as u32))
            .copied()
    }

    /// Normalizes only keys actually covered by the selected style's pack.
    pub(super) fn pack_key(&self, mut key: GlyphKey) -> GlyphKey {
        // A full-font hinted key still names the pack's size-independent outline.
        // The guard avoids borrowing pack ink for an explicit custom variation.
        if key.instance == key.face
            && self
                .packs
                .get(&key.face)
                .is_some_and(|s| s.glyphs.contains_key(&key.glyph_id))
        {
            // Preserve face/glyph/variation while removing only size-dependent fields.
            key.hint_ppem = 0;
            key.hint_mode = super::super::HintMode::None;
        }
        key
    }

    /// An evicted pack shape is re-imported, with oracle overflow if this frame is full.
    pub(super) fn recover_pack(
        &mut self,
        p: &super::GlyphPlacement,
        fitting: bool,
        italic: bool,
        bold: bool,
    ) -> Option<Result<super::PreparedGlyph, String>> {
        // An explicit variation identity cannot borrow a different pack instance.
        if p.key.instance != p.key.face {
            return None;
        }
        let key = self.pack_key(p.key);
        let source = self.packs.get(&key.face)?;
        let glyph = source.glyphs.get(&key.glyph_id)?;
        // Copy owned geometry before mutating the atlas, ending the pack-map borrow.
        let ink = source.pack.prepared(glyph);
        Some(ink.and_then(|ink| match ink {
            Some(ink) => {
                let mut p = *p;
                p.key = key;
                self.prepare_outline(&p, ink, fitting, italic, bold)
            }
            None => Ok(super::PreparedGlyph::Empty),
        }))
    }

    /// Indexes one immutable pack once, rather than scanning the cmap per draw.
    fn remember_pack(&mut self, face: u32, pack: Arc<crate::pack::FontPack>) {
        // The scalar directory remains authoritative for startup cmap placement;
        // this companion index handles glyph IDs returned by full-font shaping.
        let glyphs = pack.glyphs.values().map(|g| (g.glyph_id, *g)).collect();
        self.packs.insert(face, Source { pack, glyphs });
    }

    /// Releases startup-only protection after replacement tables and metrics validate.
    pub fn release_pack_pins(&mut self) {
        self.atlas.release_pack_pins();
    }
}
