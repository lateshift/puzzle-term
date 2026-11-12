//! Shaping tests use the bundled face and its real OpenType substitutions.

use super::{ShapingOptions, TextShaper};
use crate::{FontData, face::FontFace};

/// Loads the approved test face from the repository's asset directory.
fn face() -> Result<FontFace, String> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/JetBrainsMono-Variable.ttf"
    );
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    FontFace::new(FontData::new(bytes, 0), 400)
}

/// Ordinary ASCII can bypass the shaper when substitutions are disabled.
#[test]
fn ascii_direct_path_preserves_cluster_bytes() -> Result<(), String> {
    let face = face()?;
    let off = ShapingOptions {
        ligatures: false,
        features: &[],
    };
    assert!(TextShaper::ascii_fast_path(&face, "ab->", off)?);
    let mut shaper = TextShaper::default();
    let glyphs = shaper.shape(0, &face, "ab->", off)?;
    assert_eq!(
        glyphs.iter().map(|glyph| glyph.cluster).collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
    Ok(())
}

/// Disabling ligatures changes the font's coding substitutions at the cursor.
#[test]
fn ligatures_use_harfrust_and_cache_the_run() -> Result<(), String> {
    let face = face()?;
    // The approved JetBrains face advertises coding features. If their
    // GSUB records disappear, the direct path would silently lose arrows.
    assert!(face.has_ascii_ligatures());
    let on = ShapingOptions {
        ligatures: true,
        features: &[],
    };
    let off = ShapingOptions {
        ligatures: false,
        features: &[],
    };
    let mut shaper = TextShaper::default();
    let enabled = shaper.shape(0, &face, "-> !=", on)?.to_vec();
    let disabled = shaper.shape(0, &face, "-> !=", off)?.to_vec();
    assert_ne!(enabled, disabled);
    assert_eq!(enabled, shaper.shape(0, &face, "-> !=", on)?);
    Ok(())
}
