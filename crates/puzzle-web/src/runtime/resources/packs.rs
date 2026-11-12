//! Loads the existing four-face startup pack set required by §8.8.
//!
//! GPU initialization still runs in parallel. Style packs are installed before
//! publishing the text resource, so takeover never replaces ready styled ink
//! with a cold font job. These are the already approved and shipped assets.
use super::{TextContext, error, fetch};
use puzzle_term::text::{font_set::FontStyle, pack::FontPack};
use wasm_bindgen::JsValue;

/// Validates and imports each style under its stable FontSet face slot.
pub(super) async fn styles(context: &mut TextContext) -> Result<(), JsValue> {
    for (name, style) in [
        ("jetbrains-bold.pzfp", FontStyle::Bold),
        ("jetbrains-italic.pzfp", FontStyle::Italic),
        ("jetbrains-bold-italic.pzfp", FontStyle::BoldItalic),
    ] {
        // A failed derived cache takes the existing full-font startup fallback.
        let bytes = fetch(name).await?;
        let pack = FontPack::decode(&bytes).map_err(|e| error("FontLoadFailed", &e))?;
        context
            .add_font_pack(pack, style)
            .map_err(|e| error("FontLoadFailed", &e))?;
    }
    Ok(())
}
