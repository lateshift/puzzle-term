//! Pixel-only sources receive effective sizing before transformed atlas emission.
use super::*;
use puzzle_text::cluster_source::{ClusterGlyph, ClusterRequest, PlatformFallback};
use puzzle_text::glyph::{BitmapKind, GlyphPlacement, RasterGlyph};
use std::sync::{Arc, Mutex};

/// Records source requests while returning authored mask coverage at that resolution.
struct Pixels(Arc<Mutex<Vec<f32>>>);
impl PlatformFallback for Pixels {
    fn can_draw(&mut self, _: ClusterRequest<'_>) -> bool {
        true
    }
    fn prepare(
        &mut self,
        _: ClusterRequest<'_>,
        p: &GlyphPlacement,
    ) -> Result<ClusterGlyph, String> {
        self.0.lock().expect("request journal").push(p.size_px);
        Ok(ClusterGlyph::Bitmap(RasterGlyph {
            kind: BitmapKind::Mask,
            width: p.advance_px,
            height: p.cell_height_px,
            offset_px: [0, -p.origin_px[1]],
            pixels: vec![255; (p.advance_px * p.cell_height_px) as usize],
        }))
    }
}

/// Cursor and selection reuse one source resolution and the exact same forward map.
#[test]
fn text_sizing_bitmap_uses_effective_resolution_and_parallel_tint() {
    for dpr in [1.0, 2.0] {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let mut loaded = fonts();
        loaded.set_platform_fallback(Box::new(Pixels(requests.clone())));
        let mut context = TextContext::new(loaded, 14.0, dpr).expect("context");
        let mut builder = FrameBuilder::new(&context);
        let mut terminal = Terminal::new(16, 6, 0, 0);
        terminal.advance("\x1b]66;s=3;漢\x07\x1b[2;2H".as_bytes());
        let mut frame = RenderFrame::default();
        builder
            .build(&mut context, &terminal, &mut frame)
            .expect("pixel heading");
        assert_eq!(requests.lock().expect("journal").len(), 1);
        assert!(requests.lock().expect("journal")[0] >= 2.5 * context.metrics().device_size);
        let row = &builder.rows[0];
        assert_eq!(row.rasters.len(), 1);
        assert_eq!(row.cursor_rasters.len(), 1);
        assert_ne!(
            row.rasters[0].flags & puzzle_frame::raster::RASTER_EFFECT,
            0
        );
        assert_eq!(row.rasters[0].rect, row.cursor_rasters[0].rect);
        assert_ne!(row.raster_selection_ink[0], u32::MAX);
        builder
            .build(&mut context, &terminal, &mut frame)
            .expect("warm pixels");
        assert_eq!(requests.lock().expect("journal").len(), 1);
    }
}
