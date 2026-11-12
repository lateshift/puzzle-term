//! Property-defined blanks retain columns without querying any ink source.
use super::*;
use crate::cluster_source::{ClusterGlyph, ClusterRequest, PlatformFallback};
use crate::glyph::GlyphPlacement;
use crate::layout::GridCell;
/// No property-defined blank may need a system outline.
struct Forbidden;
impl PlatformFallback for Forbidden {
    fn can_draw(&mut self, _: ClusterRequest<'_>) -> bool {
        panic!("blank initiated fallback search")
    }
    fn prepare(
        &mut self,
        _: ClusterRequest<'_>,
        _: &GlyphPlacement,
    ) -> Result<ClusterGlyph, String> {
        panic!("blank requested ink")
    }
}
/// Exhaustive property coverage includes spaces beyond the user's examples.
#[test]
fn property_only_blank_clusters_preserve_grid_width_without_fallback() -> Result<(), String> {
    let mut fonts = fonts()?;
    fonts.set_platform_fallback(Box::new(Forbidden));
    let mut shaper = TextShaper::default();
    let mut row = RowLayout::default();
    for ch in (0..=0x10ffff)
        .filter_map(char::from_u32)
        .filter(|ch| ch.is_whitespace() || crate::font_set::is_default_ignorable(*ch))
    {
        if ch.is_control() {
            continue;
        }
        let text = ch.to_string();
        let cells = [GridCell {
            byte: 0,
            end: text.len(),
            column: 0,
            span: 2,
        }];
        let placed = row.grid_line(&mut fonts, &mut shaper, &text, &cells, options(), |_| {})?;
        assert!(placed.glyphs.is_empty(), "{ch:?} produced ink");
        assert_eq!(placed.cells, 2);
    }
    // The union applies per scalar even if a stored cluster mixes both properties.
    let text = "\u{3000}\u{200d}";
    let cells = [GridCell {
        byte: 0,
        end: text.len(),
        column: 0,
        span: 2,
    }];
    let placed = row.grid_line(&mut fonts, &mut shaper, text, &cells, options(), |_| {})?;
    assert!(placed.glyphs.is_empty());
    assert_eq!(placed.cells, 2);
    Ok(())
}
