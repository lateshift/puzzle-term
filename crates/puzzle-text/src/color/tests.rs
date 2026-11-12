//! Real approved COLRv1 font traversal, foreground and malformed-graph guards.
#![allow(
    clippy::unwrap_used,
    reason = "vendored test font must produce exact authored cases"
)]
use puzzle_slug::paint::Brush;
/// This fixture is embedded only in test targets, never distributed binaries.
fn face() -> crate::face::FontFace {
    crate::face::FontFace::new(
        crate::FontData::new(
            include_bytes!("../../../../assets/fonts/test/test_glyphs-glyf_colr_1.ttf").to_vec(),
            0,
        ),
        400,
    )
    .unwrap()
}
#[test]
fn colr_brush_and_clip_contract() {
    let font = face();
    let mut seen = [false; 4];
    for id in [8, 12, 54, 156, 180, 205] {
        let glyph = font.color_layers(id, 0xff_00_ff_00).unwrap().unwrap();
        assert!(!glyph.layers.is_empty());
        for layer in glyph.layers {
            assert!(!layer.path.elements().is_empty());
            let kind = match layer.paint.brush {
                Brush::Solid(_) => 0,
                Brush::Linear { .. } => 1,
                Brush::Radial { .. } => 2,
                Brush::Sweep { .. } => 3,
            };
            seen[kind] = true;
            assert!(layer.paint.texels().is_ok());
        }
    }
    assert_eq!(seen, [true; 4]);
    // The upstream fixture intentionally contains these two recursion cycles.
    for id in [178, 179] {
        assert!(font.color_layers(id, 0xffffffff).is_err());
    }
    assert!(font.color_layers(0, 0xffffffff).unwrap().is_none());
}
#[test]
fn foreground_only_palette_contract() {
    let font = face();
    let mut changed = 0;
    let mut unchanged = 0;
    for id in 0..221 {
        let (Ok(Some(red)), Ok(Some(green))) = (
            font.color_layers(id, 0xff_00_00_ff),
            font.color_layers(id, 0xff_00_ff_00),
        ) else {
            continue;
        };
        for (a, b) in red.layers.iter().zip(&green.layers) {
            if a.foreground {
                changed += 1;
                assert_ne!(a.paint.texels().unwrap(), b.paint.texels().unwrap());
            } else {
                unchanged += 1;
                assert_eq!(a.paint.texels().unwrap(), b.paint.texels().unwrap());
            }
        }
    }
    assert!(changed > 0 && unchanged > 0);
}

/// COLRv0 uses the same ordered fill surface, including its foreground sentinel.
#[test]
fn colrv0_order_and_foreground_contract() {
    // Reuse the approved outlines/CPAL; only a tiny COLR table is authored here.
    // The vendored file is immutable and this synthetic table is test-only.
    let mut bytes =
        include_bytes!("../../../../assets/fonts/test/test_glyphs-glyf_colr_1.ttf").to_vec();
    let count = u16::from_be_bytes([bytes[4], bytes[5]]) as usize;
    let record = (0..count)
        .map(|i| 12 + i * 16)
        .find(|&i| &bytes[i..i + 4] == b"COLR")
        .unwrap();
    let offset = u32::from_be_bytes(bytes[record + 8..record + 12].try_into().unwrap()) as usize;
    // Header: one base glyph, two layers; layer order is palette then foreground.
    let table: [u8; 28] = [
        0, 0, 0, 1, 0, 0, 0, 14, 0, 0, 0, 20, 0, 2, 0, 8, 0, 0, 0, 2, 0, 2, 0, 0, 0, 3, 255, 255,
    ];
    bytes[offset..offset + 28].copy_from_slice(&table);
    bytes[record + 12..record + 16].copy_from_slice(&28u32.to_be_bytes());
    let font = crate::face::FontFace::new(crate::FontData::new(bytes, 0), 400).unwrap();
    let red = font.color_layers(8, 0xff0000ff).unwrap().unwrap();
    let green = font.color_layers(8, 0xff00ff00).unwrap().unwrap();
    assert_eq!(red.layers.len(), 2);
    assert!(!red.layers[0].foreground);
    assert!(red.layers[1].foreground);
    assert_eq!(
        red.layers[0].paint.sample([0.0; 2]),
        green.layers[0].paint.sample([0.0; 2])
    );
    assert_ne!(
        red.layers[1].paint.sample([0.0; 2]),
        green.layers[1].paint.sample([0.0; 2])
    );
    assert!(red.layers.iter().all(|l| !l.path.elements().is_empty()));
}
