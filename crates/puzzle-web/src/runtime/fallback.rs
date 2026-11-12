//! Browser font fallback uses whole-cluster Canvas2D ink (M11.3, D-7).
//!
//! The source stores only Rust strings and coverage facts, keeping the
//! platform-neutral Send seam intact. Its shared platform text resource owns
//! one readback canvas. Device size comes from immutable placement;
//! browser advances never influence terminal width or later glyph origins.
//! White text yields a reusable mask. Authored emoji colors remain RGBA.
//! Pixel inspection chooses that representation, independent of font names.
//! The browser's fallback engine sees the complete UTF-8 grapheme, including
//! variation selectors, joiners, regional indicators and skin-tone modifiers.
//! Calling fillText per scalar would destroy emoji ligatures and flag pairs.
//! Source coverage and preparation always receive the same style/presentation.
//!
//! Coverage probes compare against the browser's own unsupported-scalar ink.
//! A visible missing-glyph box is not accepted as proof of font coverage.
//! Permanent scalar facts prevent repeated validation inside composed clusters.
//! Default-ignorables stay in shaping and never receive individual probes.
//! Failed Canvas operations report no coverage and leave tofu available.
//!
//! Font shorthand preserves configured fallback-family order. Names are
//! quoted with escaped quotes and backslashes, and line breaks become spaces.
//! Weight and italic are separate style facts, not guessed from family names.
//! Emoji presentation prioritizes system emoji families after configured ones.
//! Text presentation prioritizes system UI ink before the emoji fallback.
//!
//! Measurement uses the exact device font size chosen by the text context.
//! Actual ink bounds exclude advances and side bearings from fitting geometry.
//! Canvas ascent is positive upward, while destination y grows downward.
//! The shared fit converts those bounds around the primary row's baseline.
//! Emoji may enlarge into their inset span; ordinary text only shrinks.
//! Scaling the Canvas transform rasterizes at the fitted size directly,
//! avoiding a second bitmap resampling pass that would blur narrow strokes.
//!
//! The temporary canvas is limited to the glyph's allocated cell rectangle.
//! Foreign overhang cannot corrupt adjacent rows or terminal padding.
//! Cropping removes transparent margins after the fitted draw has completed.
//! The crop offset reconstructs the original cell position in frame assembly.
//! Its pixels remain owned Rust bytes after the reusable canvas is cleared.
//!
//! ImageData is straight-alpha color. The pixel helper compares premultiplied
//! color to white coverage before choosing a mask or authored-color bitmap.
//! Masks store only alpha and can serve multiple colors without preparation.
//! Color crops keep their own RGB and alpha rather than inheriting text tint.
//! Neither trait implementation retains a DOM or offscreen rendering handle.
//! This keeps both source objects Send without any handwritten unsafe wrapper.
use puzzle_term::text::{
    cluster_source::{ClusterGlyph, ClusterRequest, EmojiSource, PlatformFallback},
    font_set::FontStyle,
    glyph::GlyphPlacement,
    presentation::Presentation,
};
use wasm_bindgen::JsCast;
use web_sys::{OffscreenCanvas, OffscreenCanvasRenderingContext2d};
/// Per-call work evidence distinguishes draw and synchronous readback costs.
mod operations;
/// Cropping and color classification operate on readback samples.
mod pixels;
/// Device-sized validation keeps the drawn bitmap and permanent scalar facts.
mod validation;

/// A platform source owns font policy, never persistent JavaScript objects.
pub(super) struct BrowserFallback {
    /// Canvas operation totals include coverage and final device-sized drawing.
    operations: operations::Counts,
    /// Existing shared text identity owns the JS canvas outside the Send source.
    resource: Option<String>,
    /// Only unsupported-marker ink is retained; covered ink transfers directly.
    markers: validation::Markers,
    /// Last call's validation and final-draw durations remain separate in evidence.
    preparation_costs: [std::time::Duration; 2],
    /// Configured family order precedes the system fonts in both presentation modes.
    families: String,
    /// Two bits per scalar/style/presentation retain coverage facts through cluster-cache eviction.
    scalars: Vec<u8>,
    /// Cumulative idle coverage work and unique indexed scalar/policy slots.
    index_us: usize,
    /// Permanent scalar facts cannot be lost when the whole-cluster cache retires an entry.
    indexed_slots: usize,
}
/// The browser implementation is valid behind both cluster-source traits.
impl BrowserFallback {
    /// Escapes CSS names without allowing a family to inject size or style declarations.
    pub(super) fn new(families: &[String]) -> Self {
        // Treat each user entry as one quoted CSS family, not font shorthand.
        let families = families
            .iter()
            .map(|family| {
                format!(
                    "\"{}\"",
                    family
                        .replace('\\', "\\\\")
                        .replace('"', "\\\"")
                        .replace(['\n', '\r'], " ")
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        Self {
            operations: Default::default(),
            resource: None,
            markers: Default::default(),
            preparation_costs: [std::time::Duration::ZERO; 2],
            families,
            // Unicode has 0x110000 codepoint slots. Four styles and two presentations
            // need eight independent facts; four two-bit facts fit in each byte.
            // This permanent table answers negatives after whole-cluster eviction.
            scalars: vec![0; 0x110000 * 8 / 4],
            index_us: 0,
            indexed_slots: 0,
        }
    }
    /// Builds the CSS font shorthand at the exact device font size.
    fn font(&self, request: ClusterRequest<'_>, size: f32) -> String {
        // Primary-style facts select real browser weight and italic faces.
        let italic = matches!(request.style, FontStyle::Italic | FontStyle::BoldItalic);
        let bold = matches!(request.style, FontStyle::Bold | FontStyle::BoldItalic);
        // Selectors change ordering without changing the grapheme given to fillText.
        let system = if request.presentation == Presentation::Emoji {
            "\"Apple Color Emoji\",\"Segoe UI Emoji\",\"Noto Color Emoji\",system-ui,sans-serif"
        } else {
            "system-ui,sans-serif,\"Apple Color Emoji\",\"Segoe UI Emoji\",\"Noto Color Emoji\""
        };
        let comma = if self.families.is_empty() { "" } else { "," };
        format!(
            "{} {} {}px {}{comma}{system}",
            if italic { "italic" } else { "normal" },
            if bold { 700 } else { 400 },
            size,
            self.families
        )
    }
    /// Connects a Send source to the already-owned platform text resource.
    fn for_resource(families: &[String], name: &str) -> Self {
        let mut source = Self::new(families);
        source.resource = Some(name.into());
        source
    }
    /// Coverage-only queries read known facts; device-sized preparation discovers ink.
    fn covers(&mut self, request: ClusterRequest<'_>) -> bool {
        request
            .cluster
            .chars()
            .filter(|c| !puzzle_term::text::font_set::is_default_ignorable(*c))
            .all(|scalar| {
                let slot = (request.style as usize * 2
                    + usize::from(request.presentation == Presentation::Emoji))
                    * 0x110000
                    + scalar as usize;
                (self.scalars[slot / 4] >> ((slot % 4) * 2)) & 3 == 2
            })
    }

    /// Fits whole-cluster ink into its existing cells, retaining the text baseline.
    fn draw(
        &self,
        request: ClusterRequest<'_>,
        placement: &GlyphPlacement,
    ) -> Result<ClusterGlyph, String> {
        let width = placement.ink_limit_px;
        let height = placement.cell_height_px;
        // The cell stage bounds memory and clips foreign overhang to allocated cells.
        if width == 0
            || height == 0
            || width > 2046
            || height > 2046
            || !placement.size_px.is_finite()
            || placement.size_px <= 0.0
        {
            return Err("browser raster geometry exceeds page".into());
        }
        let canvas = self.context(width, height)?;
        canvas.set_font(&self.font(request, placement.size_px));
        let fit = self.fit(&canvas, request, placement)?;
        // Scale coordinates around the shared baseline; do not resample a bitmap.
        canvas
            .set_transform(
                f64::from(fit.scale),
                0.0,
                0.0,
                f64::from(fit.scale),
                f64::from(fit.offset[0]),
                f64::from(placement.baseline_px + fit.offset[1]),
            )
            .map_err(js_error)?;
        self.operations.draw(&canvas, request.cluster, 0.0, 0.0)?;
        // Own the clipped pixels before the shared stage serves a different glyph.
        let data = self.operations.read(&canvas, width, height)?;
        Ok(ClusterGlyph::Bitmap(pixels::crop(
            &data,
            width,
            height,
            placement.baseline_px as i32,
        )?))
    }
    /// Measures foreign ink in y-up pixels before shared baseline-preserving fitting.
    fn fit(
        &self,
        canvas: &OffscreenCanvasRenderingContext2d,
        request: ClusterRequest<'_>,
        placement: &GlyphPlacement,
    ) -> Result<puzzle_term::text::fitting::InkFit, String> {
        let metrics = canvas.measure_text(request.cluster).map_err(js_error)?;
        // Canvas metrics are device pixels with ascent positive upward from baseline.
        let bounds = [
            -metrics.actual_bounding_box_left() as f32,
            -metrics.actual_bounding_box_descent() as f32,
            metrics.actual_bounding_box_right() as f32,
            metrics.actual_bounding_box_ascent() as f32,
        ];
        // Uniform cell fitting uses ink bounds, never the browser advance width.
        let fit = puzzle_term::text::fitting::fit(
            placement,
            bounds,
            request.presentation == Presentation::Emoji,
            placement.baseline_px,
        );
        Ok(fit)
    }
    /// Resolve the existing resource canvas without retaining a JS value in core.
    fn context(
        &self,
        width: u32,
        height: u32,
    ) -> Result<OffscreenCanvasRenderingContext2d, String> {
        if let Some(name) = self.resource.as_deref()
            && let Some((canvas, created)) =
                super::resources::readback::context(name, width, height)?
        {
            if created {
                self.operations.created();
            }
            return Ok(canvas);
        }
        // Standalone source fixtures have no shared text owner and stay self-contained.
        self.operations.created();
        context(width, height)
    }
}
/// Emoji presentation prioritizes this source ahead of bundled monochrome faces.
impl EmojiSource for BrowserFallback {
    /// Tests whole-cluster coverage without changing the terminal placement.
    fn can_draw(&mut self, request: ClusterRequest<'_>) -> bool {
        request.presentation == Presentation::Emoji && self.covers(request)
    }
    /// Returns fitted samples whose crop origin refers to the primary baseline.
    fn prepare(
        &mut self,
        request: ClusterRequest<'_>,
        placement: &GlyphPlacement,
    ) -> Result<ClusterGlyph, String> {
        self.draw(request, placement)
    }
}
/// CJK and rare symbols reach the same source after the bundled fallback faces.
impl PlatformFallback for BrowserFallback {
    /// Tests whole-cluster coverage without changing the terminal placement.
    fn can_draw(&mut self, request: ClusterRequest<'_>) -> bool {
        self.covers(request)
    }
    /// Returns fitted samples whose crop origin refers to the primary baseline.
    fn prepare(
        &mut self,
        request: ClusterRequest<'_>,
        placement: &GlyphPlacement,
    ) -> Result<ClusterGlyph, String> {
        self.draw(request, placement)
    }
}
/// Allocates a temporary white-ink surface; JavaScript values never enter FontSet.
pub(super) fn context(
    width: u32,
    height: u32,
) -> Result<OffscreenCanvasRenderingContext2d, String> {
    context_with_readback_hint(width, height, true)
}
/// The measurement witness compares identical reusable surfaces with either hint.
fn context_with_readback_hint(
    width: u32,
    height: u32,
    frequent: bool,
) -> Result<OffscreenCanvasRenderingContext2d, String> {
    // Canvas dimensions are bounded by the caller before crossing this JS seam.
    let canvas = OffscreenCanvas::new(width, height).map_err(js_error)?;
    let options = js_sys::Object::new();
    // Frequent readback chooses a CPU backing instead of synchronizing a GPU surface.
    js_sys::Reflect::set(&options, &"willReadFrequently".into(), &frequent.into())
        .map_err(js_error)?;
    let context = canvas
        .get_context_with_context_options("2d", &options)
        .map_err(js_error)?
        .ok_or("OffscreenCanvas 2d unavailable")?
        .dyn_into::<OffscreenCanvasRenderingContext2d>()
        .map_err(|error| js_error(error.into()))?;
    // White is the neutral coverage probe; system color fonts retain their palette.
    context.set_fill_style_str("#ffffff");
    Ok(context)
}
/// Keeps recoverable browser failures at the neutral String error boundary.
pub(super) fn js_error(error: wasm_bindgen::JsValue) -> String {
    format!("browser raster: {error:?}")
}

#[cfg(test)]
/// Pixel representation and unsupported-cluster behavior without mocked fonts.
mod tests;

/// Budgeted Canvas discovery outside frame building and presentation.
pub(super) mod idle;

/// Loaded faces share the browser's bounded, event-driven preparation seam.
pub(super) mod loaded;
