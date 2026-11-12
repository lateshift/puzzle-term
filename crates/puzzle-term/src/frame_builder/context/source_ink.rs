//! Owned source results enter ordinary vector or device-sized raster identities.
//!
//! Validation geometry is the drawn unhinted geometry, never a discarded probe.
//! Hinted drawing retains its separate ppem key. Capacity overflow samples the
//! covered shape through the oracle while preserving fitting and style policy.
//! D-47 grid results can now transfer prepared geometry to a bitmap owner.
//! These results still use the ordinary source identity and complete fitting key.
//! The hinted drawing is sufficient after successful structural validation;
//! native grid publication need not retain a duplicate unhinted validation shape.
//! A later transform obtains its unhinted vector from the same native source seam.
//! Web completion publication keeps its previous ownership and admission policy.
//! Raster population is shared by sessions using the same text configuration.
//! Coverage does not depend on foreground and can be recolored by cursor/selection.
//! No pending grid batch may leave the atlas before uploads are collected.
use super::TextContext;
use puzzle_text::{cluster_source::ClusterGlyph, glyph::GlyphPlacement, raster::RasterKey};

/// Publication and visible dispatch share cache identities across row rewrites.
impl TextContext {
    /// Keeps the same vector/style/color/raster identities as visible source dispatch.
    pub(super) fn cache_source_ink(
        &mut self,
        p: &GlyphPlacement,
        ink: ClusterGlyph,
    ) -> Result<(), String> {
        match ink {
            ClusterGlyph::ValidatedOutline {
                unhinted,
                hinted,
                bold,
                italic,
            } => {
                // Both keys retain the already prepared validation and drawing shapes.
                self.cache_validated(p, unhinted, hinted, bold, italic)?;
            }
            ClusterGlyph::PreparedOutline { ink, bold, italic } => {
                // Converted curves and bands have already been paid for by the source call.
                let prepared = self.backend.prepare_source_outline(p, ink, bold, italic)?;
                self.cache_overflow(p, prepared)?;
            }
            ClusterGlyph::Outline(path) => {
                // Preserve the reversible source glyph identity across metadata retirement.
                // Device size matters only if vector pressure selects oracle pixels.
                let prepared = self.backend.prepare_path(p, path)?;
                self.cache_overflow(p, prepared)?;
            }
            ClusterGlyph::StyledOutline { path, bold, italic } => {
                // Synthetic style belongs to the owned result, not the current grid row.
                // A completion may arrive after that row has already been overwritten.
                let prepared = self.backend.prepare_styled_path(p, path, bold, italic)?;
                self.cache_overflow(p, prepared)?;
            }
            ClusterGlyph::Color(color) => {
                // Keep a complete layered glyph together when capacity requires raster ink.
                if let Some(samples) = self.backend.prepare_source_color_ink(p, color)? {
                    self.cache_overflow(p, puzzle_text::glyph::PreparedGlyph::Rasterized(samples))?;
                }
            }
            ClusterGlyph::Bitmap(samples) => {
                // Effective transform sizes already travel in the producer placement.
                // DPR still belongs to this context, as in grid and free-text lookups.
                // Raster publication owns exact samples and marks their upload pending until presentation.
                self.raster.prepare(
                    RasterKey {
                        // Face, glyph and hint identity distinguish geometrically different sources.
                        glyph: p.key,
                        // Exact floating-point device size participates in the crop identity.
                        size: p.size_px.to_bits(),
                        // Fitting span and baseline prevent reuse of an incompatible sampled crop.
                        fit: [p.ink_limit_px, p.cell_height_px, p.baseline_px.to_bits()],
                        // This text context owns the physical scale for both bitmap backends.
                        dpr: self.dpr.to_bits(),
                    },
                    samples,
                )?;
            }
            // A producer may decline; provisional markers never enter a final ink key.
            ClusterGlyph::Pending(_) | ClusterGlyph::Missing(_) => {}
        }
        Ok(())
    }

    /// Covered outlines retain oracle ink when vector capacity is exhausted (D-39).
    pub(super) fn cache_overflow(
        &mut self,
        p: &GlyphPlacement,
        prepared: puzzle_text::glyph::PreparedGlyph,
    ) -> Result<(), String> {
        // Vector success needs no raster entry; only overflow transfers sampled pixels.
        if let puzzle_text::glyph::PreparedGlyph::GridOutline {
            ink,
            placement,
            style,
        } = prepared
        {
            self.raster.prepare_grid(
                RasterKey {
                    glyph: p.key,
                    size: p.size_px.to_bits(),
                    fit: [p.ink_limit_px, p.cell_height_px, p.baseline_px.to_bits()],
                    dpr: self.dpr.to_bits(),
                },
                &placement,
                *ink,
                style,
            )?;
        } else if let puzzle_text::glyph::PreparedGlyph::Rasterized(samples) = prepared {
            // CPU-oracle masks enter the same atlas as platform fallback bitmaps.
            // The placement key includes fitting so taller sources cannot reuse a bad crop.
            self.raster.prepare_outline_mask(
                RasterKey {
                    glyph: p.key,
                    size: p.size_px.to_bits(),
                    fit: [p.ink_limit_px, p.cell_height_px, p.baseline_px.to_bits()],
                    dpr: self.dpr.to_bits(),
                },
                samples,
            )?;
        }
        // Raster ownership is complete before row caches can look up the final key.
        Ok(())
    }
    /// Retains both unhinted validation and the single prepared hinted drawing.
    fn cache_validated(
        &mut self,
        p: &GlyphPlacement,
        unhinted: puzzle_text::preparation::Outline,
        hinted: puzzle_text::preparation::Outline,
        bold: bool,
        italic: bool,
    ) -> Result<(), String> {
        if self.backend.grid_cache_enabled() && !p.effect {
            // A native grid request retains only its drawn hinted mask, avoiding a second cache.
            let prepared = self
                .backend
                .prepare_source_outline(p, hinted, bold, italic)?;
            return self.cache_overflow(p, prepared);
        }
        // The unhinted validation shape uses the same ordinary identity as effect drawing.
        let mut validation = *p;
        // Unhinted outlines are size-independent; device ppem must not fragment this key.
        validation.key.hint_ppem = 0;
        // Clear the hint mode together with ppem so both fields describe one outline.
        validation.key.hint_mode = puzzle_text::glyph::HintMode::None;
        // Validation and drawn shapes use the ordinary key space, not a
        // second geometry cache. Either may need device-sized oracle overflow.
        let prepared = self
            .backend
            .prepare_source_outline(&validation, unhinted, bold, italic)?;
        // A validated outline retains real oracle ink if page capacity rejects its vector.
        self.cache_overflow(&validation, prepared)?;
        // The hinted drawn shape is prepared once by its source and transferred intact.
        let prepared = self
            .backend
            .prepare_source_outline(p, hinted, bold, italic)?;
        // Covered current-frame overflow must resolve to pixels instead of a tofu key.
        self.cache_overflow(p, prepared)?;
        Ok(())
    }
}
