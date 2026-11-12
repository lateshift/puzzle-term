//! Live memory-policy changes rebuild every session sharing one text context.
use super::*;
use crate::frame_builder::context::budget as policy;
use crate::settings::{Layer, SettingsStore};

/// Pack-only startup and already-cached rows must survive shrink and growth.
#[test]
fn runtime_reload_preserves_complete_shared_frames_and_defaults() {
    for dpr in [1.0, 2.0] {
        let pack = FontPack::decode(&fixture("fontpacks/jetbrains-regular.pzfp")).unwrap();
        let mut context = TextContext::from_pack(pack, 14.0, dpr).unwrap();
        assert_eq!(context.glyph_budget_bytes(), 48 * 1024 * 1024);
        assert_eq!(context.raster.page_limits(), [4, 2]);
        let mut builders = [FrameBuilder::new(&context), FrameBuilder::new(&context)];
        let mut vt = Terminal::new(96, 2, 0, 0);
        vt.advance(b"\x1b[?25l");
        let ascii: String = ('!'..='~').collect();
        vt.advance(ascii.as_bytes());
        let mut frame = RenderFrame::default();
        let mut store = SettingsStore::default();
        for mib in [48, policy::MIN_MIB, 28, 48] {
            let report = store.apply_key(
                "render.glyphMemoryMb",
                puzzle_bus::Value::Number(mib as f64),
                Layer::Session,
            );
            assert!(report.rejected.is_empty());
            context
                .set_glyph_memory_mb(store.effective().render_glyph_memory_mb)
                .unwrap();
            for builder in &mut builders {
                builder.build(&mut context, &vt, &mut frame).unwrap();
                // Every printable ASCII glyph has an actual prepared font shape.
                assert_eq!(frame.slugs.len(), ascii.len());
                assert!(
                    frame
                        .slugs
                        .iter()
                        .all(|instance| instance.bbox[0] < instance.bbox[2])
                );
            }
            assert!(context.vector_pressure().1 <= mib as usize * policy::MIB);
            assert!(context.raster_bytes() <= mib as usize * policy::MIB);
            let generation = context.atlas_generation();
            assert!(!context.set_glyph_memory_mb(mib).unwrap());
            assert_eq!(
                context.atlas_generation(),
                generation,
                "equal settings discarded warm ink"
            );
        }
    }
}

/// Validation clamps from physical page requirements, preserving the 48-MiB default.
#[test]
fn structural_minimum_is_validated_and_reported_every_time() {
    let mut store = SettingsStore::default();
    assert_eq!(store.effective().render_glyph_memory_mb, 48);
    let minimum = puzzle_text::raster::MIN_BUDGET_BYTES
        .max(puzzle_slug::atlas::MIN_BUDGET_BYTES)
        .div_ceil(policy::MIB);
    for _ in 0..3 {
        let report = store.apply_key(
            "render.glyphMemoryMb",
            puzzle_bus::Value::Number(0.0),
            Layer::User,
        );
        assert_eq!(report.clamped, ["render.glyphMemoryMb"]);
        assert_eq!(store.effective().render_glyph_memory_mb as usize, minimum);
    }
}
