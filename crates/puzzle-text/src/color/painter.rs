//! Scoped affine/clip state while skrifa walks an OpenType paint graph.
//!
//! Callback errors are captured because ColorPainter's drawing methods return
//! no Result. A failed graph publishes no partial layers to the glyph cache.
use super::{ColorLayer, ColorLayers, brush};
use kurbo::{Affine, BezPath, Shape};
use skrifa::{
    FontRef, GlyphId, MetadataProvider,
    color::{Brush, ColorPainter, CompositeMode, Transform},
    instance::{Location, Size},
    metrics::BoundingBox,
    outline::DrawSettings,
};
/// A font traversal's temporary owners; none survive a successful cache insert.
pub(super) struct Painter<'a> {
    /// Validated font tables, borrowed only while preparing a cold glyph.
    font: &'a FontRef<'a>,
    /// Chosen variable-font coordinates shared with the text shaper.
    location: &'a Location,
    /// Conversion from font units into the root em coordinate system.
    upem: f64,
    /// Current affine in font units, before normalization.
    transform: Affine,
    /// Saved parent transforms for balanced pushes and pops.
    transforms: Vec<Affine>,
    /// Clip paths already transformed to root em space.
    clips: Vec<BezPath>,
    /// Consecutive fills emitted by the traversal.
    layers: Vec<ColorLayer>,
    /// Packed straight sRGB terminal foreground for palette index 0xffff.
    foreground: u32,
    /// Captures the first failure from a void painter callback.
    error: Option<String>,
    /// Composite approximation is reported to the font owner once.
    unsupported: bool,
}
impl<'a> Painter<'a> {
    /// Validates em scaling before any outline or brush callback occurs.
    pub(super) fn new(
        font: &'a FontRef<'a>,
        location: &'a Location,
        foreground: u32,
    ) -> Result<Self, String> {
        // Font units differ between families; normalizing once aligns every
        // layer with the backend-neutral placement size rather than CPAL data.
        let upem = f64::from(font.metrics(Size::unscaled(), location).units_per_em);
        // Division by zero would contaminate both outline bounds and brushes.
        if upem == 0.0 {
            return Err("COLR font has zero units per em".into());
        }
        // Each traversal starts at the glyph root; no state from a previous
        // color graph can leak through its transform or clipping stack.
        Ok(Self {
            font,
            location,
            upem,
            transform: Affine::IDENTITY,
            transforms: Vec::new(),
            clips: Vec::new(),
            layers: Vec::new(),
            foreground,
            error: None,
            unsupported: false,
        })
    }
    /// Publishes complete traversal output or the first callback error.
    pub(super) fn finish(self) -> Result<ColorLayers, String> {
        // Void callbacks cannot stop skrifa, but partial output is discarded
        // transactionally here rather than being cached as a complete glyph.
        if let Some(error) = self.error {
            return Err(error);
        }
        // The one-time log belongs to the owning font set, which knows whether
        // several style instances share these same underlying font bytes.
        Ok(ColorLayers {
            layers: self.layers,
            unsupported_composite: self.unsupported,
        })
    }
    /// Converts a font-unit transform into an em-to-em affine.
    fn em_transform(&self) -> Affine {
        // Conjugating by UPEM preserves linear terms and normalizes translation.
        Affine::scale(1.0 / self.upem) * self.transform * Affine::scale(self.upem)
    }
    /// Builds an unhinted outline and captures it at its clip push transform.
    fn glyph_path(&self, glyph: GlyphId) -> Result<BezPath, String> {
        // The clip references the glyph outline, never another COLR graph.
        // Recursive color glyph references are traversed and bounded by skrifa.
        let outline = self
            .font
            .outline_glyphs()
            .get(glyph)
            .ok_or("COLR clip has no outline")?;
        // Glyph clipping is size independent: no 1× hinting adjustment may
        // shift one layer relative to the color line or its other layers.
        let mut pen = crate::probe::EmPen {
            path: BezPath::new(),
            scale: 1.0 / self.upem,
        };
        outline
            .draw(
                DrawSettings::unhinted(Size::unscaled(), self.location),
                &mut pen,
            )
            .map_err(|e| format!("COLR clip outline: {e}"))?;
        // Freeze the path now. A later brush transform changes the paint
        // coordinate system without retroactively moving this clip outline.
        Ok(self.em_transform() * pen.path)
    }
    /// Saves a fill with the innermost outline and all enclosing clip scopes.
    fn layer(&mut self, brush: Brush<'_>) -> Result<(), String> {
        // A finite fill limit bounds retained paths from hostile paint graphs.
        if self.layers.len() >= 256 {
            return Err("COLR glyph exceeds 256 fills".into());
        }
        // Unbounded fills have no finite Slug quad; a glyph must supply a
        // glyph or box clip before a brush can produce drawable ink.
        let Some(path) = self.clips.last() else {
            return Err("COLR fill has no bounded clip".into());
        };
        // Palette references are resolved before any GPU-facing allocation.
        let (paint, foreground) = brush::convert(
            self.font,
            brush,
            self.foreground,
            self.em_transform(),
            self.upem,
        )?;
        // Empty clips intentionally suppress painting while keeping graph order.
        if path.elements().is_empty() {
            return Ok(());
        }
        // Enclosing scopes intersect the leaf, in the same root em space.
        // Preserving traversal order keeps source-over composition correct.
        self.layers.push(ColorLayer {
            path: path.clone(),
            clips: self.clips[..self.clips.len() - 1].to_vec(),
            paint,
            foreground,
        });
        Ok(())
    }
}
/// Executes skrifa's scoped drawing operations without altering Slug math.
impl ColorPainter for Painter<'_> {
    /// Pushes an affine; a nonfinite/singular brush map fails at conversion.
    fn push_transform(&mut self, transform: Transform) {
        // Composition is parent * child, as specified by OpenType PaintTransform.
        self.transforms.push(self.transform);
        self.transform *= Affine::new(transform.elements().map(f64::from));
    }
    /// Restores the preceding transform independently of captured clip paths.
    fn pop_transform(&mut self) {
        // Skrifa balances valid graphs; identity also makes an invalid pop safe.
        self.transform = self.transforms.pop().unwrap_or(Affine::IDENTITY);
    }
    /// Draws the font's clip glyph at the current root-em transform.
    fn push_clip_glyph(&mut self, glyph: GlyphId) {
        // Keep an empty placeholder on failure so later pops remain balanced.
        match self.glyph_path(glyph) {
            Ok(path) => self.clips.push(path),
            Err(error) => {
                self.error.get_or_insert(error);
                self.clips.push(BezPath::new());
            }
        }
    }
    /// Captures even transformed clip boxes as ordinary Slug rectangle paths.
    fn push_clip_box(&mut self, bbox: BoundingBox) {
        // ClipBox corners are font units, independent of the outline UPEM pen.
        let rect = kurbo::Rect::new(
            f64::from(bbox.x_min) / self.upem,
            f64::from(bbox.y_min) / self.upem,
            f64::from(bbox.x_max) / self.upem,
            f64::from(bbox.y_max) / self.upem,
        );
        self.clips.push(self.em_transform() * rect.to_path(0.0));
    }
    /// Removes only the innermost clip when its COLR scope ends.
    fn pop_clip(&mut self) {
        // Ancestor clips stay captured until their own traversal scope closes.
        self.clips.pop();
    }
    /// Records a fill while preserving any earlier callback error.
    fn fill(&mut self, brush: Brush<'_>) {
        // Preserve the original diagnostic; later callbacks cannot overwrite it.
        if self.error.is_none()
            && let Err(error) = self.layer(brush)
        {
            self.error = Some(error);
        }
    }
    /// Marks non-source-over modes; all fills still keep painter order.
    fn push_layer(&mut self, mode: CompositeMode) {
        // The spec chooses a visible source-over approximation for other modes.
        self.unsupported |= mode != CompositeMode::SrcOver;
    }
}
