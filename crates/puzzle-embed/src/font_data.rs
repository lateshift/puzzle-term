//! Native bundled font bytes for deterministic M3 probes (design §8.1).
//!
//! Harness and oracle scenes read the same vendored file as the web shell
//! fetches. This platform-owned I/O keeps the face out of neutral crate
//! binaries while retaining deterministic screenshot inputs. A read error
//! is reported before GPU scene preparation instead of falling back to a
//! different OS font and silently changing the reference image.
use puzzle_term::text::FontData;
use puzzle_term::text::font_set::FontSet;

/// Loads the approved regular JetBrains Mono face with collection index zero.
pub fn bundled_font() -> Result<FontData, String> {
    let path = "assets/fonts/JetBrainsMono-Variable.ttf";
    read(path)
}

/// Loads the three approved faces for live bold, italic and symbol fallback.
pub fn bundled_font_set() -> Result<FontSet, String> {
    // Platform I/O owns every font byte; puzzle-term only receives decoded
    // data and never embeds a TTF in the wasm or native core binary (P7).
    let regular = bundled_font()?;
    let italic = read("assets/fonts/JetBrainsMono-Italic-Variable.ttf")?;
    let symbols = read("assets/fonts/SymbolsNerdFontMono-Regular.ttf")?;
    let mut fonts = FontSet::new(regular, Some(italic), Some(symbols))?;
    install_ui_fonts(&mut fonts)?;
    Ok(fonts)
}

/// Converts one native font file into the shared owned byte representation.
fn read(path: &str) -> Result<FontData, String> {
    // Resolve repository assets from the crate location, including Cargo test working directories.
    let absolute = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path);
    std::fs::read(absolute)
        .map(|bytes| FontData::new(bytes, 0))
        .map_err(|error| format!("{path}: {error}"))
}

// UI-font extensions never modify the normal grid-face fallback order.
include!("font_data/optional_items.rs");
