//! A private UI map cannot change which glyphs ordinary terminal text can find.
use super::*;

#[test]
fn named_private_icons_render_explicitly_and_never_enter_grid_fallback() -> Result<(), String> {
    let mut fonts = FontSet::new(bundled("JetBrainsMono-Variable.ttf"), None, None)?;
    assert_eq!(fonts.choose_face("\u{eb6e}", FontStyle::Regular)?, None);
    fonts.install_ui_icons(
        bundled("SymbolsNerdFontMono-Regular.ttf"),
        &[("fold-open", '\u{eb6e}'), ("fold-closed", '\u{eb70}')],
    )?;
    let (face, scalar) = fonts.ui_icon("fold-open").ok_or("UI mapping missing")?;
    assert_eq!(scalar, '\u{eb6e}');
    assert!(
        fonts
            .face(face)
            .ok_or("private UI face missing")?
            .glyph_id(scalar)?
            .is_some()
    );
    assert_eq!(fonts.choose_face("\u{eb6e}", FontStyle::Regular)?, None);
    assert_eq!(
        fonts.choose_face_from("\u{eb6e}", FontStyle::Regular, Some(face))?,
        Some(face)
    );
    // Replacing only mapping data changes the UI scalar without changing grid selection.
    fonts.install_ui_icons(
        bundled("SymbolsNerdFontMono-Regular.ttf"),
        &[("fold-open", '\u{eb70}')],
    )?;
    assert_eq!(
        fonts
            .ui_icon("fold-open")
            .ok_or("replacement UI mapping missing")?
            .1,
        '\u{eb70}'
    );
    assert_eq!(fonts.choose_face("\u{eb70}", FontStyle::Regular)?, None);
    Ok(())
}

#[test]
fn incomplete_ui_font_leaves_the_existing_mapping_intact() -> Result<(), String> {
    let mut fonts = FontSet::new(bundled("JetBrainsMono-Variable.ttf"), None, None)?;
    fonts.install_ui_icons(
        bundled("SymbolsNerdFontMono-Regular.ttf"),
        &[("fold", '\u{eb6e}')],
    )?;
    let before = fonts.ui_icon("fold");
    assert!(
        fonts
            .install_ui_icons(
                bundled("JetBrainsMono-Variable.ttf"),
                &[("fold", '\u{eb6e}')]
            )
            .is_err()
    );
    assert_eq!(fonts.ui_icon("fold"), before);
    Ok(())
}
