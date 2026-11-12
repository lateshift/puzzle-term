//! Platform-owned cluster ink joins the existing Slug caches (D-28).
//!
//! Source glyph keys retain complete cluster/style identities in FontSet.
//! Prepared handles therefore outlive the transient borrowed system font bytes.
//! Color graphs share the normal paint table and device-restoration mirrors.
//! A transient path uses the same GlyphBackend entry point as a retained face.
//! The small PathSource transfers ownership on the first outline request, so
//! the backend can build bands without cloning hundreds of contour points.
//! Native synthesis flags live beside the outline descriptor and are consulted
//! again on a cached draw; actual face weight/slant need no synthetic transform.
//! Source paint graphs use white as the foreground-opacity reference, matching
//! bundled COLR. The live instance supplies text/selection/cursor tint afterward.
//! Both shape and paint generations must remain valid for a graph cache hit.
//! A stale paint address forces complete preparation before the graph is used.
use super::{PreparedColor, SlugBackend};
use crate::glyph::{GlyphKey, GlyphOutlineSource, GlyphPlacement, PreparedGlyph};

/// Moves a newly extracted outline into the backend without a second path copy.
struct PathSource {
    /// One cold outline consumed when Slug requests its geometry.
    path: kurbo::BezPath,
}
/// The same prepare method serves font faces and transient source outlines.
impl GlyphOutlineSource for PathSource {
    /// Takes owned geometry; the key was already validated by the cluster registry.
    fn outline(&mut self, _key: GlyphKey) -> Result<kurbo::BezPath, String> {
        Ok(std::mem::take(&mut self.path))
    }
}
/// Platform preparation preserves the atlas keys and restoration policy.
impl SlugBackend {
    /// Invalidates replaced platform identities while retaining unrelated ready font ink.
    pub fn invalidate_sources(&mut self) {
        let retained = |key: &GlyphKey| {
            key.face != crate::cluster_source::EMOJI_FACE
                && key.face != crate::cluster_source::PLATFORM_FACE
        };
        // Source IDs can be reused by the replacement registry. Forget their
        // descriptors, but leave texels to ordinary page LRU at a safe boundary.
        // Keeping the identity allocator monotonic prevents a new source shape
        // from colliding with the pack or any already collected row address.
        self.shapes.retain(|key, _| retained(key));
        self.vector_sizes.retain(|key, _| retained(key));
        self.source_styles.retain(|key, _| retained(key));
        self.colors.glyphs.retain(|(key, _), _| retained(key));
    }

    /// Publishes a source color graph, preserving real ink if any vector layer overflows.
    /// The backup is cold-only; cached draws borrow their retained graph without cloning.
    pub fn prepare_source_color_ink(
        &mut self,
        p: &GlyphPlacement,
        color: crate::color::ColorLayers,
    ) -> Result<Option<crate::glyph::RasterGlyph>, String> {
        // Vector preparation consumes its paths; keep the graph until publication succeeds.
        let layers = color.layers.clone();
        // End the borrowed descriptor before selecting the alternate bitmap owner.
        match self.prepare_source_color(p, color).map(|_| ()) {
            Ok(_) => Ok(None),
            Err(error) if self.atlas.needs_eviction() => {
                // Partial graph insertion never becomes a draw with missing layers.
                tracing::debug!(event = "text.color_bitmap_overflow", %error);
                self.rasterize_color(p, &layers).map(Some)
            }
            Err(error) => Err(error),
        }
    }

    /// Uses the existing oracle plug-in for a complete covered color graph.
    pub fn rasterize_color(
        &mut self,
        p: &GlyphPlacement,
        layers: &[crate::color::ColorLayer],
    ) -> Result<crate::glyph::RasterGlyph, String> {
        // Reuse one bounded scratch atlas rather than allocating a mirror for each layer.
        self.oracle
            .get_or_insert_with(Default::default)
            .color(p, layers)
    }
    /// Packs an owned platform outline only when its retained key is cold.
    pub fn prepare_path(
        &mut self,
        placement: &GlyphPlacement,
        path: kurbo::BezPath,
    ) -> Result<PreparedGlyph, String> {
        // Fitting uses the complete cluster bounds, identically for vector and bitmap ink.
        self.prepare_fitted(placement, &mut PathSource { path }, true, false, false)
    }
    /// Shared procedural parts use their complete cell transform, never per-digit fitting.
    pub fn prepare_procedural_path(
        &mut self,
        placement: &GlyphPlacement,
        path: kurbo::BezPath,
    ) -> Result<PreparedGlyph, String> {
        // The same rule applies when capacity sends an isolated hex digit to the oracle.
        // Fitting that tiny crop independently would enlarge it inside the shared box.
        self.prepare_fitted(placement, &mut PathSource { path }, false, false, false)
    }
    /// Records synthesis beside the retained outline, without retaining font bytes.
    pub fn prepare_styled_path(
        &mut self,
        p: &GlyphPlacement,
        path: kurbo::BezPath,
        bold: bool,
        italic: bool,
    ) -> Result<PreparedGlyph, String> {
        // The matching font's weight/slant policy must also reach the oracle fallback.
        let prepared = self.prepare_fitted(p, &mut PathSource { path }, true, italic, bold)?;
        // The outline cache bounds these tiny descriptors by the same glyph keys.
        self.source_styles.insert(p.key, (bold, italic));
        Ok(prepared)
    }
    /// Transfers prepared source geometry and retains its synthesis for cached rows.
    pub fn prepare_source_outline(
        &mut self,
        p: &GlyphPlacement,
        ink: crate::preparation::Outline,
        bold: bool,
        italic: bool,
    ) -> Result<PreparedGlyph, String> {
        // Geometry comes from the validated source, so no extraction is repeated here.
        let prepared = self.prepare_outline(p, ink, true, italic, bold)?;
        // Warm draws must use the source face's style policy rather than the primary face's.
        self.source_styles.insert(p.key, (bold, italic));
        Ok(prepared)
    }
    /// Returns the matching font's synthesis policy on a warm outline hit.
    pub fn source_style(&self, key: GlyphKey) -> (bool, bool) {
        // Metadata is borrowed only after the matching outline generation validates.
        self.source_styles
            .get(&key)
            .copied()
            .unwrap_or((false, false))
    }
    /// Borrows a valid source paint graph, checking both texture incarnations.
    pub fn source_color(&self, key: GlyphKey) -> Option<&PreparedColor> {
        // The white-foreground graph is shared by grid, selection and cursor tint.
        let color = self.colors.glyphs.get(&(key, 0xffffffff))?;
        // Page generations reject cleared addresses; restoration preserves live handles.
        color
            .layers
            .iter()
            .all(|l| {
                self.atlas.contains_handle(l.shape)
                    && (l.paint.index == 0 || self.colors.paints.contains(l.paint))
            })
            .then_some(color)
    }
    /// Publishes a complete source graph only after all layer/paint insertions succeed.
    pub fn prepare_source_color(
        &mut self,
        placement: &GlyphPlacement,
        color: crate::color::ColorLayers,
    ) -> Result<&PreparedColor, String> {
        // Foreground-independent vector metadata can serve selection and cursor redraws.
        let key = (placement.key, 0xffffffff);
        // Prune obsolete addresses before enforcing the descriptor allowance.
        let atlas = &self.atlas;
        let paints = &self.colors.paints;
        self.colors.glyphs.retain(|_, glyph| {
            glyph.layers.iter().all(|l| {
                atlas.contains_handle(l.shape) && (l.paint.index == 0 || paints.contains(l.paint))
            })
        });
        // Source warnings are coalesced by the desktop font identity before this seam.
        if self.colors.glyphs.len() >= 4096 && !self.colors.glyphs.contains_key(&key) {
            return Err("COLR source descriptor budget reached".into());
        }
        let prepared = self.color_layers(color.layers)?;
        self.colors.glyphs.insert(key, prepared);
        self.colors
            .glyphs
            .get(&key)
            .ok_or("source paint graph not retained".into())
    }
}
