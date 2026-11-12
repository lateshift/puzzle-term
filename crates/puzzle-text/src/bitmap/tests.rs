//! M11.5 protects strike choice, both table formats, error fallback and alpha halos.
#![allow(
    clippy::expect_used,
    reason = "authored table fixtures must parse in these contracts"
)]
use super::*;

/// Authored PNG metadata is sufficient for the table reader's fake decoder.
struct FakeDecoder {
    /// A first rejection models a corrupt preferred source before a valid one.
    reject: bool,
}
/// The tests exercise the seam without putting PNG in the neutral crate.
impl PngDecoder for FakeDecoder {
    /// Produces a 2×2 source, rejecting the first request when directed.
    fn decode(&mut self, _: &[u8]) -> Result<DecodedBitmap, String> {
        if std::mem::take(&mut self.reject) {
            return Err("synthetic corrupt PNG".into());
        }
        Ok(DecodedBitmap {
            width: 2,
            height: 2,
            pixels: [255, 0, 0, 255].repeat(4),
        })
    }
}

#[test]
/// Unsorted strikes, fractional sizes and invalid entries obey the specified rule.
fn strike_selection_and_table_formats() {
    let sizes = [64.0, 16.0, 32.0, f32::NAN, 0.0];
    for (size, index) in [(8.0, 1), (16.0, 1), (16.1, 2), (32.0, 2), (65.0, 0)] {
        assert_eq!(strike_index(&sizes, size), Some(index));
    }
    assert_eq!(strike_index(&sizes, f32::INFINITY), None);
    assert_eq!(strike_index(&[], 16.0), None);
    for cbdt in [false, true] {
        let data = font(cbdt);
        let image = load(&data, 0, 15.0, &mut FakeDecoder { reject: false })
            .expect("table lookup")
            .expect("bitmap");
        assert_eq!(image.ppem, 16.0);
        assert_eq!(image.image.pixels, [255, 0, 0, 255].repeat(4));
        assert!(
            load(&data, 1, 16.0, &mut FakeDecoder { reject: false })
                .expect("absent glyph")
                .is_none()
        );
    }
}

#[test]
/// Failed decoding tries a second candidate and retains a once-per-font warning.
fn decoder_failure_tries_next_source() {
    let first_font = font(false);
    let second_font = font(true);
    let candidates = [(first_font, 0), (second_font, 0)];
    let mut warned = std::collections::HashSet::new();
    let placement = placement();
    let draw = first(
        &candidates,
        &placement,
        24.0,
        &mut FakeDecoder { reject: true },
        &mut warned,
    )
    .expect("second source");
    assert_eq!(warned.len(), 1);
    assert_eq!(draw.kind, BitmapKind::Color);
    assert!(draw.width <= placement.ink_limit_px && draw.height <= placement.cell_height_px);
    assert_eq!(placement.origin_px, [16, 24]);
    assert!(
        load(
            &FontData::new(vec![0; 8], 0),
            0,
            16.0,
            &mut FakeDecoder { reject: false }
        )
        .is_err()
    );
}

#[test]
/// Analytical integrals pin fractional boxes and hide colored transparent texels.
fn premultiplied_area_reference() {
    let source = DecodedBitmap {
        width: 3,
        height: 1,
        pixels: vec![255, 0, 0, 255, 0, 255, 0, 0, 0, 0, 255, 128],
    };
    // Each destination covers 1.5 source pixels: red/blue never mix with hidden green.
    assert_eq!(
        area_resize(&source, 2, 1).expect("fractional filter"),
        [255, 0, 0, 170, 0, 0, 255, 85]
    );
    let averaged = area_resize(&source, 1, 1).expect("whole box");
    assert_eq!(averaged, [170, 0, 85, 128]);
    let transparent = DecodedBitmap {
        width: 2,
        height: 2,
        pixels: [255, 127, 63, 0].repeat(4),
    };
    assert_eq!(area_resize(&transparent, 1, 1).expect("zero alpha"), [0; 4]);
    assert!(area_resize(&source, 0, 1).is_err());
    assert!(
        area_resize(
            &DecodedBitmap {
                width: u32::MAX,
                height: 2,
                pixels: vec![]
            },
            1,
            1
        )
        .is_err()
    );
}

/// Two cells are immutable; the filter receives only their ink allowance.
fn placement() -> GlyphPlacement {
    GlyphPlacement {
        cell_column: 0,
        key: crate::glyph::GlyphKey {
            face: 0,
            glyph_id: 0,
            instance: 0,
            hint_ppem: 0,
            hint_mode: crate::glyph::HintMode::None,
        },
        origin_px: [16, 24],
        baseline_px: 24.0,
        size_px: 16.0,
        advance_px: 32,
        ink_limit_px: 32,
        cell_height_px: 32,
        cell_span: 2,
        color: 0xffffffff,
        effect: false,
        color_glyph: true,
    }
}

/// A tiny authored sfnt directory needs no new vendored bitmap test font (R4).
fn font(cbdt: bool) -> FontData {
    let mut head = vec![0; 54];
    head[18..20].copy_from_slice(&1000u16.to_be_bytes());
    let maxp = vec![0, 1, 0, 0, 0, 1];
    let mut tables = vec![(*b"head", head), (*b"maxp", maxp)];
    if cbdt {
        tables.extend(cbdt_tables());
    } else {
        tables.push((*b"sbix", sbix_table()));
    }
    tables.sort_by_key(|(tag, _)| *tag);
    let mut bytes = vec![0; 12 + tables.len() * 16];
    bytes[0..4].copy_from_slice(&[0, 1, 0, 0]);
    bytes[4..6].copy_from_slice(&(tables.len() as u16).to_be_bytes());
    for (index, (tag, table)) in tables.into_iter().enumerate() {
        let at = 12 + index * 16;
        bytes[at..at + 4].copy_from_slice(&tag);
        let offset = bytes.len() as u32;
        bytes[at + 8..at + 12].copy_from_slice(&offset.to_be_bytes());
        bytes[at + 12..at + 16].copy_from_slice(&(table.len() as u32).to_be_bytes());
        bytes.extend(table);
        while !bytes.len().is_multiple_of(4) {
            bytes.push(0);
        }
    }
    FontData::new(bytes, 0)
}

/// PNG IHDR width/height locations are read by skrifa before codec validation.
fn png_header() -> Vec<u8> {
    let mut png = vec![0; 24];
    png[16..20].copy_from_slice(&2u32.to_be_bytes());
    png[20..24].copy_from_slice(&2u32.to_be_bytes());
    png
}

/// Sbix v1 has a strike offset and two glyph offsets for this one-glyph font.
fn sbix_table() -> Vec<u8> {
    let mut bytes = vec![0, 1, 0, 3, 0, 0, 0, 1, 0, 0, 0, 12, 0, 16, 0, 72];
    bytes.extend(12u32.to_be_bytes());
    bytes.extend(44u32.to_be_bytes());
    bytes.extend([0, 0, 0, 0]);
    bytes.extend(*b"png ");
    bytes.extend(png_header());
    bytes
}

/// CBLC's index-format 1 points to one CBDT image-format 17 PNG record.
fn cbdt_tables() -> Vec<([u8; 4], Vec<u8>)> {
    let mut location = vec![0; 56];
    location[0..8].copy_from_slice(&[0, 3, 0, 0, 0, 0, 0, 1]);
    location[8..12].copy_from_slice(&56u32.to_be_bytes());
    location[12..16].copy_from_slice(&24u32.to_be_bytes());
    location[16..20].copy_from_slice(&1u32.to_be_bytes());
    location[52..56].copy_from_slice(&[16, 16, 32, 1]);
    location.extend([0, 0, 0, 0, 0, 0, 0, 8]);
    location.extend([0, 1, 0, 17, 0, 0, 0, 4]);
    location.extend(0u32.to_be_bytes());
    location.extend(33u32.to_be_bytes());
    let mut data = vec![0, 3, 0, 0, 2, 2, 0, 2, 2];
    data.extend(24u32.to_be_bytes());
    data.extend(png_header());
    vec![(*b"CBLC", location), (*b"CBDT", data)]
}
