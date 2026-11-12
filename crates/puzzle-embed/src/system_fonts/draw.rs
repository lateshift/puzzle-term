//! First-placement validation transfers every successful outline to its normal store.
//!
//! Unhinted outline and COLR preparation is size independent. Bitmap sources
//! decode once at the actual placement. D-27 alone needs an unhinted validation
//! shape plus one hinted draw shape; both become ordinary atlas entries, so
//! effects can reuse the validation ink without another extraction or side cache.
use super::NativeFonts;
use puzzle_term::text::{
    cluster_source::{ClusterGlyph, ClusterRequest},
    glyph::{GlyphPlacement, HintMode},
};

impl NativeFonts {
    /// Validates once, retaining owned preparation instead of returning only a boolean.
    pub(super) fn resolve_draw(
        &mut self,
        request: ClusterRequest<'_>,
        placement: &GlyphPlacement,
        validated: bool,
    ) -> Option<ClusterGlyph> {
        // An unhinted draw already is validation; only D-27 requires a second variant.
        if validated || placement.key.hint_ppem == 0 || placement.key.hint_mode == HintMode::None {
            return prepared(self.resolve(request, placement)?);
        }
        // Validation uses the reusable em-space key so effects can later draw it.
        let mut unhinted = *placement;
        unhinted.key.hint_ppem = 0;
        unhinted.key.hint_mode = HintMode::None;
        // Strikes and color graphs do not use D-27. Their first actual-size
        // preparation is also their drawn preparation, with no second decode.
        let validation = self.resolve(request, &unhinted)?;
        // Style synthesis accompanies the complete matched cluster, not the primary family.
        // Image and color sources already contain final ink and need no outline conversion.
        let (path, bold, italic) = match validation {
            ClusterGlyph::Outline(path) => (path, false, false),
            ClusterGlyph::StyledOutline { path, bold, italic } => (path, bold, italic),
            ink => return prepared(ink),
        };
        // Only hinted outlines repeat extraction; unhinted validation remains useful ink.
        let drawn = self.resolve(request, placement);
        let hinted = match drawn {
            Some(
                ClusterGlyph::Outline(hinted) | ClusterGlyph::StyledOutline { path: hinted, .. },
            ) => hinted,
            // A failed hinter must not erase already validated usable ink. The
            // unhinted shape becomes the draw if the optional fitting fails.
            _ => return prepared(ClusterGlyph::StyledOutline { path, bold, italic }),
        };
        // Both variants transfer prepared ownership to the ordinary atlas key space.
        Some(ClusterGlyph::ValidatedOutline {
            unhinted: outline(path)?,
            hinted: outline(hinted)?,
            bold,
            italic,
        })
    }
}

/// Band building belongs to the same admitted call as extraction, never later publication.
fn prepared(ink: ClusterGlyph) -> Option<ClusterGlyph> {
    // Image and color sources already contain final ink and need no outline conversion.
    let (path, bold, italic) = match ink {
        ClusterGlyph::Outline(path) => (path, false, false),
        ClusterGlyph::StyledOutline { path, bold, italic } => (path, bold, italic),
        ink => return Some(ink),
    };
    // Empty or malformed geometry cannot become confirmed ink. The caller may
    // continue its fallback chain; successful conversion transfers its sole owner.
    // Failed geometry is reported by outline and never cached as successful coverage.
    let ink = outline(path)?;
    Some(ClusterGlyph::PreparedOutline { ink, bold, italic })
}

/// Malformed font geometry is reported before trying another fallback candidate.
fn outline(
    path: puzzle_term::text::geometry::BezPath,
) -> Option<puzzle_term::text::preparation::Outline> {
    match puzzle_term::text::preparation::Outline::new(path) {
        // Empty contextual components retain layout without manufacturing a draw quad.
        Ok(ink) => ink,
        Err(error) => {
            // Invalid input is recoverable and must not panic or silently confirm ink.
            tracing::warn!(event = "font.parse_failed", %error);
            None
        }
    }
}
