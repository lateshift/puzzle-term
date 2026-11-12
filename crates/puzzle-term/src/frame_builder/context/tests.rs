//! Pack-first frames protect web startup without embedding fonts in core.
//! The fixture is loaded by tests only; production hosts supply fetched data.
#![allow(
    clippy::unwrap_used,
    reason = "fixture setup failures must fail these tests"
)]
use super::*;
use crate::frame_builder::FrameBuilder;
use puzzle_frame::RenderFrame;
use puzzle_text::{FontData, pack::FontPack};
use puzzle_vt::terminal::Terminal;

/// Settings changes retire shared addresses without losing covered pack ink.
mod budget;
/// Tiny page budgets exercise takeover and cached rows without an OS font.
mod eviction;

/// Admission refusal cannot erase both the real glyph and its diagnostic.
mod deferred;

/// Source installation cannot discard ready pack/loaded glyphs during takeover.
#[test]
fn source_installation_preserves_ready_font_ink() {
    let pack = FontPack::decode(&fixture("fontpacks/jetbrains-regular.pzfp")).unwrap();
    let mut context = TextContext::from_pack(pack, 14.0, 2.0).unwrap();
    let mut builder = FrameBuilder::new(&context);
    let mut vt = Terminal::new(192, 2, 0, 0);
    let ascii: String = ('!'..='~').flat_map(|ch| [ch, ' ']).collect();
    vt.advance(ascii.as_bytes());
    let mut frame = RenderFrame::default();
    builder.build(&mut context, &vt, &mut frame).unwrap();
    let before: Vec<_> = frame
        .slugs
        .iter()
        .map(|s| (s.glyph_loc, s.band_info, s.bbox, s.m, s.t))
        .collect();
    let count = context.prepared_glyphs();
    context
        .load_fonts(
            FontData::new(fixture("fonts/JetBrainsMono-Variable.ttf"), 0),
            None,
            None,
        )
        .unwrap();
    context
        .set_cluster_sources(Box::new(Pixels(50)), Box::new(Pixels(50)))
        .unwrap();
    assert_eq!(
        context.prepared_glyphs(),
        count,
        "source installation discarded ready font ink"
    );
    builder.build(&mut context, &vt, &mut frame).unwrap();
    assert_eq!(
        frame
            .slugs
            .iter()
            .map(|s| (s.glyph_loc, s.band_info, s.bbox, s.m, s.t))
            .collect::<Vec<_>>(),
        before,
        "takeover changed packed ASCII addresses or ink"
    );
}

/// Every startup style uses its pack even when admission declines full-font work.
#[test]
fn all_style_packs_survive_takeover_without_preparation() {
    use puzzle_text::font_set::FontStyle;
    let pack = FontPack::decode(&fixture("fontpacks/jetbrains-regular.pzfp")).unwrap();
    let mut context = TextContext::from_pack(pack, 14.0, 1.0).unwrap();
    for (name, style) in [
        ("bold", FontStyle::Bold),
        ("italic", FontStyle::Italic),
        ("bold-italic", FontStyle::BoldItalic),
    ] {
        context
            .add_font_pack(
                FontPack::decode(&fixture(&format!("fontpacks/jetbrains-{name}.pzfp"))).unwrap(),
                style,
            )
            .unwrap();
    }
    let mut builder = FrameBuilder::new(&context);
    let mut vt = Terminal::new(32, 5, 0, 0);
    vt.advance(b"alpha beta alpha\r\n\x1b[1malpha beta alpha\r\n\x1b[3malpha beta alpha\r\n\x1b[22malpha beta alpha");
    let mut frame = RenderFrame::default();
    builder.build(&mut context, &vt, &mut frame).unwrap();
    let before: Vec<_> = frame
        .slugs
        .iter()
        .map(|s| (s.glyph_loc, s.band_info, s.bbox, s.m, s.t))
        .collect();
    context
        .load_fonts(
            FontData::new(fixture("fonts/JetBrainsMono-Variable.ttf"), 0),
            Some(FontData::new(
                fixture("fonts/JetBrainsMono-Italic-Variable.ttf"),
                0,
            )),
            None,
        )
        .unwrap();
    let requests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    context
        .set_preparation_driver(Box::new(deferred::Decline(requests.clone())))
        .unwrap();
    builder.build(&mut context, &vt, &mut frame).unwrap();
    assert_eq!(requests.load(std::sync::atomic::Ordering::Relaxed), 0);
    assert_eq!(
        before,
        frame
            .slugs
            .iter()
            .map(|s| (s.glyph_loc, s.band_info, s.bbox, s.m, s.t))
            .collect::<Vec<_>>(),
        "takeover changed styled pack ink"
    );
}

/// Reads the same external bytes used by the browser's default pack.
fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../../assets/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

/// Pack geometry survives takeover, reset and shared row cache invalidation.
#[test]
fn pack_first_frame_and_font_takeover() {
    for dpr in [1.0, 2.0] {
        let pack = FontPack::decode(&fixture("fontpacks/jetbrains-regular.pzfp")).unwrap();
        let mut context = TextContext::from_pack(pack, 14.0, dpr).unwrap();
        let geometry = context.metrics();
        let mut builder = FrameBuilder::new(&context);
        let mut vt = Terminal::new(12, 2, 0, 0);
        vt.advance(b"Hello pack");
        let mut frame = RenderFrame::default();
        builder.build(&mut context, &vt, &mut frame).unwrap();
        assert!(!frame.slugs.is_empty());
        assert!(!frame.uploads.is_empty());
        assert!(
            context.fonts.is_none(),
            "first frame has never parsed a TTF"
        );
        let count = context.prepared_glyphs();
        context.reset_atlas();
        builder.build(&mut context, &vt, &mut frame).unwrap();
        assert!(!frame.slugs.is_empty());
        assert!(!frame.uploads.is_empty(), "reset restores the pack planes");
        let data = FontData::new(fixture("fonts/JetBrainsMono-Variable.ttf"), 0);
        context.load_fonts(data, None, None).unwrap();
        let after = context.metrics();
        assert_eq!(geometry.width_px, after.width_px);
        assert_eq!(geometry.height_px, after.height_px);
        assert_eq!(geometry.baseline_px, after.baseline_px);
        builder.build(&mut context, &vt, &mut frame).unwrap();
        assert!(!frame.slugs.is_empty());
        assert_eq!(
            context.prepared_glyphs(),
            count,
            "§8.8 pack keys reused at every DPR"
        );
    }
}

/// A mismatched fetch cannot reinterpret existing packed glyph identities.
#[test]
fn takeover_rejects_wrong_font_transactionally() {
    let pack = FontPack::decode(&fixture("fontpacks/jetbrains-regular.pzfp")).unwrap();
    let mut context = TextContext::from_pack(pack, 14.0, 1.0).unwrap();
    let old = context.atlas_generation();
    let wrong = FontData::new(fixture("fonts/JetBrainsMono-Italic-Variable.ttf"), 0);
    assert!(context.load_fonts(wrong, None, None).is_err());
    assert!(context.fonts.is_none());
    assert_eq!(context.atlas_generation(), old);
}

/// A source ID reused after replacement must not retrieve the previous bitmap.
#[test]
fn source_replacement_invalidates_prepared_ink() {
    let mut context = TextContext::new(
        FontSet::new(
            FontData::new(fixture("fonts/JetBrainsMono-Variable.ttf"), 0),
            None,
            None,
        )
        .unwrap(),
        14.0,
        1.0,
    )
    .unwrap();
    let mut builder = FrameBuilder::new(&context);
    let mut vt = Terminal::new(8, 2, 0, 0);
    vt.advance("漢".as_bytes());
    let mut frame = RenderFrame::default();
    for value in [50u8, 200] {
        context
            .set_cluster_sources(Box::new(Pixels(value)), Box::new(Pixels(value)))
            .unwrap();
        builder.build(&mut context, &vt, &mut frame).unwrap();
        assert_eq!(context.raster.allocated_bytes(), 4 * 2048 * 2048);
        assert!(frame.uploads.iter().any(|u|matches!(u,puzzle_frame::Upload::Color {pixels,..} if pixels.chunks_exact(4).any(|p|p[0]==value))));
    }
}
/// Tiny authored-color source exposes descriptor reuse without an OS font.
struct Pixels(
    /// Distinguishes the old source's red channel from its replacement.
    u8,
);
/// Deliberately covers missing text with one diagnostic authored pixel.
impl puzzle_text::cluster_source::PlatformFallback for Pixels {
    /// Any non-bundled cluster exercises the source path.
    fn can_draw(&mut self, _: puzzle_text::cluster_source::ClusterRequest<'_>) -> bool {
        true
    }
    /// Returns owned diagnostic ink without parsing any font.
    fn prepare(
        &mut self,
        _: puzzle_text::cluster_source::ClusterRequest<'_>,
        _: &puzzle_text::glyph::GlyphPlacement,
    ) -> Result<puzzle_text::cluster_source::ClusterGlyph, String> {
        Ok(puzzle_text::cluster_source::ClusterGlyph::Bitmap(
            puzzle_text::glyph::RasterGlyph {
                kind: puzzle_text::glyph::BitmapKind::Color,
                width: 1,
                height: 1,
                offset_px: [0, 0],
                pixels: vec![self.0, 0, 0, 255],
            },
        ))
    }
}
/// Keeps emoji selection out of the source-replacement witness.
impl puzzle_text::cluster_source::EmojiSource for Pixels {
    /// This fixture is a text fallback only.
    fn can_draw(&mut self, _: puzzle_text::cluster_source::ClusterRequest<'_>) -> bool {
        false
    }
    /// Returns owned diagnostic ink without parsing any font.
    fn prepare(
        &mut self,
        r: puzzle_text::cluster_source::ClusterRequest<'_>,
        p: &puzzle_text::glyph::GlyphPlacement,
    ) -> Result<puzzle_text::cluster_source::ClusterGlyph, String> {
        puzzle_text::cluster_source::PlatformFallback::prepare(self, r, p)
    }
}

/// D-30: reapplying the same policy after size/display events preserves prepared ink.
#[test]
fn identical_source_policy_preserves_bitmap_and_vector_handles() {
    let fonts = FontSet::new(
        FontData::new(fixture("fonts/JetBrainsMono-Variable.ttf"), 0),
        None,
        None,
    )
    .unwrap();
    let mut context = TextContext::new(fonts, 15.0, 2.0).unwrap();
    context
        .set_synchronous_sources(Box::new(CompletePixels))
        .unwrap();
    context.configure_synchronous_sources(&[]).unwrap();
    let mut builder = FrameBuilder::new(&context);
    let mut vt = Terminal::new(8, 2, 0, 0);
    vt.advance("A漢".as_bytes());
    let mut frame = RenderFrame::default();
    builder.build(&mut context, &vt, &mut frame).unwrap();
    let generation = context.atlas_generation();
    let entries = context.grid_cache_metrics();
    assert!(entries[0] > 0);
    let prepared = context.prepared_glyphs();
    for _ in 0..3 {
        context.reconfigure(15.0, 2.0).unwrap();
        context.configure_synchronous_sources(&[]).unwrap();
        assert_eq!(context.atlas_generation(), generation);
        assert_eq!(context.grid_cache_metrics(), entries);
        assert_eq!(context.prepared_glyphs(), prepared);
        builder.build(&mut context, &vt, &mut frame).unwrap();
        assert_eq!(context.prepared_glyphs(), prepared);
    }
}
/// Complete bitmap fixture does not depend on installed system fonts.
struct CompletePixels;
impl puzzle_text::cluster_source::synchronous::SynchronousSources for CompletePixels {
    fn configure(&mut self, _: &[String], _: puzzle_text::metrics::CellMetrics) {}
    fn eligible(
        &mut self,
        _: puzzle_text::cluster_source::ClusterRequest<'_>,
        _: puzzle_text::cluster_source::synchronous::SourceKind,
    ) -> bool {
        true
    }
    fn prepare(
        &mut self,
        r: puzzle_text::cluster_source::ClusterRequest<'_>,
        p: &puzzle_text::glyph::GlyphPlacement,
        _: puzzle_text::cluster_source::synchronous::SourceKind,
    ) -> Result<Option<puzzle_text::cluster_source::ClusterGlyph>, String> {
        puzzle_text::cluster_source::PlatformFallback::prepare(&mut Pixels(50), r, p).map(Some)
    }
}

/// Geometry-keyed negatives survive a round trip, while a new family retries ink.
#[test]
fn source_negatives_survive_geometry_and_unchanged_policy() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let fonts = FontSet::new(
        FontData::new(fixture("fonts/JetBrainsMono-Variable.ttf"), 0),
        None,
        None,
    )
    .unwrap();
    let mut context = TextContext::new(fonts, 15.0, 2.0).unwrap();
    context
        .set_synchronous_sources(Box::new(NoInk(calls.clone())))
        .unwrap();
    context.configure_synchronous_sources(&[]).unwrap();
    let mut builder = FrameBuilder::new(&context);
    let mut vt = Terminal::new(8, 2, 0, 0);
    vt.advance("漢".as_bytes());
    let mut frame = RenderFrame::default();
    for (dpr, expected) in [(2.0, 1), (1.0, 2), (2.0, 2)] {
        context.reconfigure(15.0, dpr).unwrap();
        context.configure_synchronous_sources(&[]).unwrap();
        builder.set_context(&context);
        builder.build(&mut context, &vt, &mut frame).unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), expected);
    }
    context
        .configure_synchronous_sources(&["different family".into()])
        .unwrap();
    builder.set_context(&context);
    builder.build(&mut context, &vt, &mut frame).unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 3);
}
/// Eligible cmap with no actual outline exercises the completed-negative cache.
struct NoInk(std::sync::Arc<std::sync::atomic::AtomicUsize>);
impl puzzle_text::cluster_source::synchronous::SynchronousSources for NoInk {
    fn configure(&mut self, _: &[String], _: puzzle_text::metrics::CellMetrics) {}
    fn eligible(
        &mut self,
        _: puzzle_text::cluster_source::ClusterRequest<'_>,
        _: puzzle_text::cluster_source::synchronous::SourceKind,
    ) -> bool {
        true
    }
    fn prepare(
        &mut self,
        _: puzzle_text::cluster_source::ClusterRequest<'_>,
        _: &puzzle_text::glyph::GlyphPlacement,
        _: puzzle_text::cluster_source::synchronous::SourceKind,
    ) -> Result<Option<puzzle_text::cluster_source::ClusterGlyph>, String> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(None)
    }
}
