//! Pack tests protect the first-frame no-font path and hostile-byte boundary.

use super::FontPack;
use crate::FontData;
use proptest::prelude::*;

/// Reads the approved test face only for pack generation, never for loading.
fn test_data() -> Result<FontData, String> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/JetBrainsMono-Variable.ttf"
    );
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    Ok(FontData::new(bytes, 0))
}

/// A decoded pack retains ASCII handles without retaining the font bytes.
#[test]
fn round_trip_has_ready_slug_texels() -> Result<(), String> {
    let data = test_data()?;
    let pack = FontPack::build(data.clone(), 400)?;
    let bytes = pack.encode();
    let loaded = FontPack::decode(&bytes)?;
    assert!(loaded.matches_source(&data));
    assert!(
        loaded
            .glyphs
            .get(&('B' as u32))
            .and_then(|glyph| glyph.handle)
            .is_some()
    );
    assert!(loaded.glyphs.contains_key(&(' ' as u32)));
    assert!(!loaded.curves.is_empty() && !loaded.bands.is_empty());
    // Moving the buffer by one byte exercises the alignment-copy branch.
    let mut unaligned = vec![0];
    unaligned.extend_from_slice(&bytes);
    assert_eq!(
        FontPack::decode(&unaligned[1..])?.glyphs.len(),
        loaded.glyphs.len()
    );
    Ok(())
}

proptest! {
    /// Arbitrary external bytes must return a report instead of panicking.
    #[test]
    fn hostile_pack_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = FontPack::decode(&bytes);
    }
}
