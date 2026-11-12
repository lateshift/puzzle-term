//! Property checks against the public word format and the production WGSL reader.
use super::*;
use proptest::prelude::*;

proptest! {
    /// Every valid band maximum and flag byte survives without exchanging axes.
    #[test]
    fn band_words_round_trip(x: u8, y: u8, flags: u8) {
        let word = encode_band_info(x, y, flags);
        prop_assert_eq!(decode_band_info(word), (x, y, flags));
        // Independent arithmetic models WGSL's masks and the high-half flag shift.
        prop_assert_eq!(word % 256, u32::from(x));
        prop_assert_eq!(word / 65_536 % 256, u32::from(y));
        prop_assert_eq!(word / 16_777_216, u32::from(flags));
        prop_assert_eq!(word / 256 % 256, 0);
    }

    /// Both coordinate halves are lossless over the full representable address range.
    #[test]
    fn locations_round_trip(x: u16, y: u16) {
        let word = encode_glyph_loc([x, y]);
        prop_assert_eq!(decode_glyph_loc(word), [x, y]);
        prop_assert_eq!(word % 65_536, u32::from(x));
        prop_assert_eq!(word / 65_536, u32::from(y));
    }
}

/// Pin the actual WGSL unpacking expressions, including reference E/W positions.
#[test]
fn encoding_matches_production_wgsl_reader() {
    let wgsl = include_str!("../../../puzzle-render/src/shaders/slug.wgsl");
    for expression in [
        "glyph_loc & 0xffffu",
        "glyph_loc >> 16u",
        "band_info & 0xffffu",
        "band_info >> 16u",
        "input.band_info & 0xffu",
        "(input.band_info >> 16u) & 0xffu",
        "flags & 0x1000",
        "flags & 0x2000",
    ] {
        assert!(
            wgsl.contains(expression),
            "WGSL layout changed: {expression}"
        );
    }
    assert_eq!(encode_band_info(7, 13, EVEN_ODD | WEIGHT), 0x300d_0007);
    assert_eq!(encode_glyph_loc([0x0123, 0x4567]), 0x4567_0123);
}
