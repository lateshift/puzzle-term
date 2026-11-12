//! Scalar glyph identities are reversible, so Unicode floods cannot renumber cached ink.
//!
//! The high bit separates scalar keys from monotonic composed-cluster IDs. Remaining
//! bits retain the full scalar, style and presentation; exact device geometry still
//! lives in the normal placement suffix. No string or per-scalar registry is needed.
use crate::{
    cluster_source::ClusterRequest,
    font_set::FontStyle,
    glyph::{GlyphKey, HintMode},
    presentation::Presentation,
};

/// Composed-cluster IDs must stay below this reversible scalar namespace.
pub(super) const TAG: u32 = 1 << 31;

/// Encodes exactly one scalar without allocating or consulting evictable metadata.
pub(super) fn key(request: ClusterRequest<'_>, face: u32) -> Option<GlyphKey> {
    // Additional scalars require whole-cluster metadata, including selectors and marks.
    let mut chars = request.cluster.chars();
    let scalar = chars.next()? as u32;
    // Unicode ends at U+10FFFF, leaving appearance bits above all legal scalar bits.
    if chars.next().is_some() {
        return None;
    }
    Some(GlyphKey {
        // The face selects the policy owner; style and presentation disambiguate scalar ink.
        face,
        glyph_id: TAG
            | scalar
            | ((request.style as u32) << 21)
            | (u32::from(request.presentation == Presentation::Emoji) << 23),
        instance: request.style as u32,
        // Hinted geometry is appended by the existing layout path after source selection.
        hint_ppem: 0,
        hint_mode: HintMode::None,
    })
}

/// Reconstructs validated scalar UTF-8 in caller scratch for source preparation.
pub(super) fn request(id: u32, bytes: &mut [u8; 4]) -> Result<ClusterRequest<'_>, String> {
    // The caller separates this namespace from composed IDs before decoding.
    // Reject unused high bits and invalid scalars before passing untrusted keys to a source.
    if id & !(TAG | 0xffffff) != 0 {
        return Err("invalid scalar source identity".into());
    }
    let scalar = char::from_u32(id & 0x1fffff).ok_or("invalid source scalar")?;
    // Four style classes occupy two bits immediately above the complete scalar value.
    let style = match id >> 21 & 3 {
        0 => FontStyle::Regular,
        1 => FontStyle::Bold,
        2 => FontStyle::Italic,
        _ => FontStyle::BoldItalic,
    };
    Ok(ClusterRequest {
        // Stack UTF-8 survives the source call without registering or allocating a string.
        // Decoding retains the original appearance after unrelated registry entries retire.
        cluster: scalar.encode_utf8(bytes),
        style,
        presentation: if id & (1 << 23) == 0 {
            // Presentation remains independent of scalar width and face identity.
            Presentation::Text
        } else {
            Presentation::Emoji
        },
    })
}
