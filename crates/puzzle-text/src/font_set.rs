//! Style faces and fallback selection for grid text (design §8.3).
//!
//! The platform supplies bytes for regular, italic and Symbols Nerd Font.
//! We expose regular/bold/italic/bold-italic slots even when a variable
//! font shares one file: each slot has its own variation location. A
//! missing italic file is represented by a synthetic-shear flag, not by
//! moving the cells or pretending the regular face has italic outlines.
//!
//! Fallback checks every drawable scalar in a cluster before selecting a face.
//! Variation selectors and joiners direct shaping but must never trigger a
//! missing-glyph fallback on their own (design §8.4, D-31).
//! A first-character cache accelerates common text, but each hit is still
//! validated for the rest of the cluster so combining marks cannot be
//! rendered from a face that only covers their base character.
//!
//! The four primary slots are separate style identities even when regular
//! and bold share one variable TTF. That lets a placement keep a stable
//! face ID and variation instance without carrying font bytes. Fallback
//! faces come after the primary style, with user files in config order
//! and the bundled symbols file last. Color emoji and platform raster
//! fallback enter in later milestones; absent scalars reach builtin tofu.
use crate::{
    FontData,
    builtin::{BUILTIN_FACE, BuiltinCell, TOFU_FACE},
    face::FontFace,
    glyph::{GlyphKey, GlyphOutlineSource, HintMode},
};
use rustc_hash::FxHashMap;

/// Four primary style classes used by terminal attributes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontStyle {
    /// Upright normal-weight text.
    Regular,
    /// Upright heavier text.
    Bold,
    /// Slanted normal-weight text.
    Italic,
    /// Slanted heavier text.
    BoldItalic,
}

/// Loaded faces with stable IDs for placements and fallback lookup.
pub struct FontSet {
    /// OpenType data and hinting caches addressed by `GlyphKey.face`.
    faces: Vec<FontFace>,
    /// Face ID for each primary style in enum order.
    primary: [u32; 4],
    /// User fallback faces followed by the bundled symbol face.
    fallback: Vec<u32>,
    /// Named UI icons select private faces; these never join the grid fallback chain.
    ui_icons: Vec<(String, u32, char)>,
    /// Whether the last fallback slot is the bundled symbol face.
    has_symbols: bool,
    /// True when italic positions need the 12-degree Slug shear.
    synthetic_italic: bool,
    /// Common cluster-first-scalar hits, verified before reuse.
    selection_cache: FxHashMap<(char, FontStyle, crate::presentation::Presentation), u32>,
    /// Passed-in sources and bounded identities for non-font cluster ink.
    sources: sources::Sources,
    /// Optional live-host preparation owner; deterministic headless callers stay synchronous.
    preparation: Option<crate::preparation::Pending>,
    /// Diagnostic selection hits, distinct from atlas-handle reuse.
    metrics: Option<crate::cache_metrics::CacheMetrics>,
}

/// Builds style slots without embedding font bytes in this neutral crate.
impl FontSet {
    /// Loads the approved bundled roles from platform-provided bytes.
    ///
    /// The regular face is required. Invalid optional faces are skipped with
    /// a warning, leaving a deterministic regular or tofu fallback.
    /// Variable weight gives bold its own location in the same source file;
    /// a separate italic file supplies true slanted outlines when present.
    pub fn new(
        regular: FontData,
        italic: Option<FontData>,
        symbols: Option<FontData>,
    ) -> Result<Self, String> {
        let mut faces = vec![FontFace::new(regular.clone(), 400)?];
        faces.push(FontFace::new(regular.clone(), 700)?);
        // A missing or rejected italic file keeps all four style IDs valid.
        // The drawing layer later shears only those synthetic italic slots.
        let (italic_data, italic_face, synthetic_italic) = match italic {
            Some(data) => match FontFace::new(data.clone(), 400) {
                Ok(face) => (data, face, false),
                Err(error) => {
                    tracing::warn!(event = "font.parse_failed", %error, "italic face skipped");
                    (regular.clone(), FontFace::new(regular.clone(), 400)?, true)
                }
            },
            None => (regular.clone(), FontFace::new(regular.clone(), 400)?, true),
        };
        let italic_start = u32::try_from(faces.len()).map_err(|_| "too many font faces")?;
        faces.push(italic_face);
        faces.push(FontFace::new(italic_data, 700)?);
        let mut fallback = Vec::new();
        let mut has_symbols = false;
        // A broken optional symbol file is logged and skipped. It must not
        // prevent the primary terminal font from producing its first frame.
        if let Some(data) = symbols {
            match FontFace::new(data, 400) {
                Ok(face) => {
                    let id = u32::try_from(faces.len()).map_err(|_| "too many font faces")?;
                    faces.push(face);
                    fallback.push(id);
                    has_symbols = true;
                }
                Err(error) => tracing::warn!(
                    event = "font.parse_failed",
                    %error,
                    "symbol fallback skipped"
                ),
            }
        }
        Ok(Self {
            faces,
            primary: [0, 1, italic_start, italic_start + 1],
            fallback,
            has_symbols,
            ui_icons: Vec::new(),
            synthetic_italic,
            selection_cache: FxHashMap::default(),
            sources: sources::Sources::default(),
            preparation: None,
            metrics: None,
        })
    }

    /// Adds an optional user fallback before the bundled symbol face.
    ///
    /// Insertion invalidates first-scalar hits because a newly earlier
    /// fallback could now cover a cluster that previously used symbols.
    pub fn add_fallback(&mut self, data: FontData) -> Result<u32, String> {
        let face = FontFace::new(data, 400)?;
        let id = u32::try_from(self.faces.len()).map_err(|_| "too many font faces")?;
        // Retain unaffected first-scalar decisions; only a newly covered
        // base could select this earlier face (§8.3, M11.1).
        let at = self.fallback.len() - usize::from(self.has_symbols);
        self.selection_cache
            .retain(|(first, _, presentation), old| {
                // An earlier text face still wins; emoji-first choices also remain.
                self.primary.contains(old)
                    || self.fallback[..at].contains(old)
                    || *presentation == crate::presentation::Presentation::Emoji
                        && *old == crate::cluster_source::EMOJI_FACE
                    || face.glyph_id(*first).is_ok_and(|glyph| glyph.is_none())
            });
        self.faces.push(face);
        // The final bundled symbol face remains last in the chain.
        // With no symbols, append in the user's original config order.
        let at = self.fallback.len() - usize::from(self.has_symbols);
        self.fallback.insert(at, id);
        Ok(id)
    }

    /// Chooses the first face covering an entire Unicode cluster.
    ///
    /// A cached first character is only a candidate. Marks and variation
    /// selectors can add scalars that the same face does not contain.
    pub fn choose_face(&mut self, cluster: &str, style: FontStyle) -> Result<Option<u32>, String> {
        // Unicode White_Space and Default_Ignorable_Code_Point require no ink
        // source. Standalone selectors keep the established no-face API result;
        // whitespace uses primary metrics without asking whether a font has ink.
        if is_blank_cluster(cluster) {
            return Ok(cluster
                .chars()
                .any(char::is_whitespace)
                .then_some(self.primary[style as usize]));
        }
        let request = crate::cluster_source::ClusterRequest {
            cluster,
            style,
            presentation: crate::presentation::presentation(cluster),
        };
        self.resolve(request)
    }

    /// Gives explicitly selected free-text faces first refusal, retaining normal fallback.
    pub fn choose_face_from(
        &mut self,
        cluster: &str,
        style: FontStyle,
        preferred: Option<u32>,
    ) -> Result<Option<u32>, String> {
        // The explicit face must cover the entire grapheme, including marks.
        if let Some(face) = preferred {
            if self.face(face).is_none() {
                return Err("requested free-text face is not loaded".into());
            }
            if self.covers(face, cluster)? {
                return Ok(Some(face));
            }
        }
        self.choose_face(cluster, style)
    }

    /// Chooses text or a passed-in source without looking up invisible controls.
    fn resolve(
        &mut self,
        request: crate::cluster_source::ClusterRequest<'_>,
    ) -> Result<Option<u32>, String> {
        use crate::{cluster_source::EMOJI_FACE, presentation::Presentation};
        let Some(first) = request
            .cluster
            .chars()
            .find(|ch| !is_default_ignorable(*ch))
        else {
            return Ok(None);
        };
        let cache_key = (first, request.style, request.presentation);
        // Emoji presentation outranks even a text face that covers the base.
        if request.presentation == Presentation::Emoji && self.sources.emoji_covers(request) {
            // Emoji eligibility is validated before reuse, but still has a selection identity.
            let hit = self.selection_cache.get(&cache_key) == Some(&EMOJI_FACE);
            crate::cache_metrics::CacheMetrics::lookup(&mut self.metrics, hit, 0);
            self.selection_cache.insert(cache_key, EMOJI_FACE);
            return Ok(Some(EMOJI_FACE));
        }
        // A first-scalar cache cannot prove that a longer ZWJ or combining
        // sequence is drawable: every candidate validates the whole cluster.
        if let Some(id) = self.selection_cache.get(&cache_key).copied() {
            let covered = match id {
                EMOJI_FACE => self.sources.emoji_covers(request),
                crate::cluster_source::PLATFORM_FACE => self.sources.platform_covers(request),
                _ => {
                    request.cluster.len() == first.len_utf8() || self.covers(id, request.cluster)?
                }
            };
            if covered {
                crate::cache_metrics::CacheMetrics::lookup(&mut self.metrics, true, 0);
                return Ok(Some(id));
            }
        }
        crate::cache_metrics::CacheMetrics::lookup(&mut self.metrics, false, 0);
        let primary = self.primary[request.style as usize];
        for id in std::iter::once(primary).chain(self.fallback.iter().copied()) {
            if self.covers(id, request.cluster)? {
                self.selection_cache.insert(cache_key, id);
                return Ok(Some(id));
            }
        }
        let chosen = self.sources.last_resort(request);
        if let Some(id) = chosen {
            self.selection_cache.insert(cache_key, id);
        }
        Ok(chosen)
    }

    /// Borrows a face by its stable placement ID.
    /// No clone of the platform-owned bytes is needed for repeated glyphs.
    pub fn face(&self, id: u32) -> Option<&FontFace> {
        self.faces.get(id as usize)
    }

    /// Returns the chosen primary style face without fallback.
    /// This is used for cell metrics, which must not vary between glyphs.
    pub fn primary_face(&self, style: FontStyle) -> u32 {
        self.primary[style as usize]
    }

    /// Reports whether italic needs an affine shear in this font set.
    /// It never changes the cell advance or origin selected by shaping.
    pub fn needs_synthetic_italic(&self, style: FontStyle) -> bool {
        self.synthetic_italic && matches!(style, FontStyle::Italic | FontStyle::BoldItalic)
    }

    /// Reports whether this style and face need a second offset ink layer.
    ///
    /// Variation-capable primary faces use their real 700-weight outlines;
    /// a fixed fallback face has no such axis and follows §8.3 instead.
    pub fn needs_synthetic_bold(&self, style: FontStyle, face: u32) -> bool {
        matches!(style, FontStyle::Bold | FontStyle::BoldItalic)
            && self.face(face).is_some_and(FontFace::needs_synthetic_bold)
    }

    /// Verifies complete scalar coverage without substituting `.notdef`.
    /// Returning false allows the next fallback to try the same cluster.
    fn covers(&self, id: u32, cluster: &str) -> Result<bool, String> {
        let face = self.face(id).ok_or("fallback face ID is invalid")?;
        for scalar in cluster.chars() {
            // Default-ignorable controls shape or select the neighbouring
            // glyph; their absence from a cmap is not a missing ink face.
            if is_default_ignorable(scalar) {
                continue;
            }
            if face.glyph_id(scalar)?.is_none() {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// Implements Unicode 17's Default_Ignorable_Code_Point intervals for text.
///
/// Source: DerivedCoreProperties.txt, Unicode 17.0.0. These scalars can
/// influence shaping, but cannot paint a separate fallback or tofu glyph.
pub fn is_default_ignorable(ch: char) -> bool {
    matches!(ch as u32,
        0x00ad | 0x034f | 0x061c | 0x115f..=0x1160 | 0x17b4..=0x17b5 |
        0x180b..=0x180f | 0x200b..=0x200f | 0x202a..=0x202e |
        0x2060..=0x206f | 0x3164 | 0xfe00..=0xfe0f | 0xfeff |
        0xffa0 | 0xfff0..=0xfff8 | 0x1bca0..=0x1bca3 |
        0x1d173..=0x1d17a | 0xe0000..=0xe0fff
    )
}

/// Supplies chosen unhinted or ppem-hinted paths to the Slug backend.
impl GlyphOutlineSource for FontSet {
    /// Live hosts admit cold loaded faces; headless callers retain deterministic preparation.
    fn prepared_outline(
        &mut self,
        p: &crate::glyph::GlyphPlacement,
        style: [bool; 3],
    ) -> Option<Result<Option<crate::preparation::Outline>, String>> {
        // The host-owned driver retains validated geometry instead of extracting
        // it again when the same glyph becomes visible in a later cached row.
        // Synthetic style accompanies that geometry through completion handoff.
        self.prepare_outline(p, style)
    }
    /// Rejects a stale face ID or inconsistent hint key with a report.
    /// Validated keys keep unhinted and ppem-specific outlines distinct.
    fn outline(&mut self, key: GlyphKey) -> Result<kurbo::BezPath, String> {
        if matches!(
            key.face,
            BUILTIN_FACE | TOFU_FACE | crate::builtin::TOFU_COMPONENT_FACE
        ) {
            // Procedural outlines are keyed by exact device cell geometry.
            // Their path still passes through the same Slug backend seam.
            let cell = BuiltinCell::unpack(key.instance)?;
            if key.face == crate::builtin::TOFU_COMPONENT_FACE {
                return Ok(cell.tofu_component(key.glyph_id)?.path);
            }
            if key.face == TOFU_FACE {
                return Ok(cell.tofu(key.glyph_id).path);
            }
            let ch = char::from_u32(key.glyph_id).ok_or("invalid builtin codepoint")?;
            return cell
                .ink(ch)
                .map(|ink| ink.path)
                .ok_or("builtin glyph is unsupported".into());
        }
        if (key.hint_ppem == 0) != (key.hint_mode == HintMode::None) {
            return Err("glyph hint mode and ppem disagree".into());
        }
        let face = self
            .faces
            .get_mut(key.face as usize)
            .ok_or("glyph face ID is invalid")?;
        face.outline(key.glyph_id, key.hint_ppem, key.hint_mode)
    }
}

#[cfg(test)]
mod tests;

/// Loaded-face cold jobs share source completion and frame allowance semantics.
mod preparation;
/// Complete-cluster identities and passed-in source preparation.
mod sources;
/// Direct ownership of complete environment preparation policies.
mod synchronous;

#[cfg(test)]
#[path = "font_set/presentation/tests.rs"]
mod presentation_tests;

/// True when every scalar is Unicode White_Space or Default_Ignorable_Code_Point.
/// Rust's Unicode White_Space property and the versioned table above define the
/// union; examples or individual fonts cannot decide whether a cell needs ink.
pub fn is_blank_cluster(cluster: &str) -> bool {
    cluster
        .chars()
        .all(|ch| ch.is_whitespace() || is_default_ignorable(ch))
}

/// Private UI faces and icon-name mappings are independent of terminal fallback.
mod ui_icons;
