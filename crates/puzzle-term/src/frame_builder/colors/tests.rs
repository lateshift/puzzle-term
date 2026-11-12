//! Theme and OSC color ordering regressions for §10.5.
//!
//! Colors remain tagged in VT cells, so a palette override or policy
//! change must affect the next frame without a parser-side rewrite.
use super::*;
use puzzle_vt::terminal::Terminal;

/// Palette promotion, inverse, DECSCNM and dim run in the specified order.
#[test]
fn indexed_and_inverse_color_rules() {
    let mut term = Terminal::new(2, 1, 0, 0);
    let mut policy = ColorPolicy::default();
    policy.set_options(true, 1.0);
    let fg = PackedColor::palette(1);
    let bg = PackedColor::rgb(10, 20, 30);
    let (bright, background) =
        policy.resolve(&PaintState::from_terminal(&term), fg, bg, Attrs::BOLD);
    // ANSI indices now use the generated Puzzle Dark palette rather than
    // xterm's hard-coded red; the promotion itself is the invariant.
    assert_eq!(bright, policy.palette[9]);
    assert_eq!(background, [10, 20, 30]);
    let swapped = policy.resolve(
        &PaintState::from_terminal(&term),
        fg,
        bg,
        Attrs::BOLD | Attrs::INVERSE,
    );
    assert_eq!(swapped, (background, bright));
    term.advance(b"\x1b[?5h");
    let restored = policy.resolve(
        &PaintState::from_terminal(&term),
        fg,
        bg,
        Attrs::BOLD | Attrs::INVERSE,
    );
    assert_eq!(restored, (bright, background));
    term.advance(b"\x1b[?5l");
    let dim = policy
        .resolve(&PaintState::from_terminal(&term), fg, bg, Attrs::DIM)
        .0;
    let base = policy.palette[1];
    assert_eq!(
        dim,
        [
            mix(10, base[0], 0.5),
            mix(20, base[1], 0.5),
            mix(30, base[2], 0.5),
        ]
    );
}

/// OSC palette and default-background overrides recolor existing cells.
#[test]
fn osc_overrides_reach_resolver_and_clear() {
    let mut term = Terminal::new(2, 1, 0, 0);
    let policy = ColorPolicy::default();
    term.advance(b"\x1b]4;1;#123456\x07\x1b]11;#202428\x07");
    let (fg, bg) = policy.resolve(
        &PaintState::from_terminal(&term),
        PackedColor::palette(1),
        PackedColor::default(),
        Attrs::empty(),
    );
    assert_eq!(fg, [0x12, 0x34, 0x56]);
    assert_eq!(bg, [0x20, 0x24, 0x28]);
    assert_eq!(
        policy.clear(&PaintState::from_terminal(&term))[0],
        32.0 / 255.0
    );
}

/// Contrast correction cannot silently darken a nearly black foreground.
#[test]
fn minimum_contrast_meets_requested_ratio() {
    let term = Terminal::new(2, 1, 0, 0);
    let mut policy = ColorPolicy::default();
    policy.set_options(false, 7.0);
    let (fg, bg) = policy.resolve(
        &PaintState::from_terminal(&term),
        PackedColor::rgb(32, 32, 32),
        PackedColor::rgb(30, 30, 30),
        Attrs::empty(),
    );
    let ratio =
        (luminance(fg).max(luminance(bg)) + 0.05) / (luminance(fg).min(luminance(bg)) + 0.05);
    assert!(ratio >= 7.0);
}

/// A sealed frame keeps its colours when later output changes every live role.
#[test]
fn sealed_table_drives_cursor_text_and_selection_independently() {
    let mut term = Terminal::new(2, 1, 0, 0);
    let policy = ColorPolicy::default();
    term.advance(b"\x1b]21;cursor=#123456;cursor_text=#abcdef;selection_background=#203040;selection_foreground=#fedcba\x07");
    let sealed = PaintState::from_terminal(&term);
    term.advance(b"\x1b]21;cursor=#ffffff;cursor_text=#000000;selection_background=#ffffff;selection_foreground=#000000\x07");
    let cell = Cell::default();
    assert_eq!(
        policy.cursor_pair(&sealed, CursorColor::Theme, cell),
        ([0x12, 0x34, 0x56], [0xab, 0xcd, 0xef])
    );
    assert_eq!(
        policy.selection_background(&sealed),
        rgba([0x20, 0x30, 0x40])
    );
    assert_eq!(
        policy.selection_ink(&sealed, rgba([99; 3])),
        rgba([0xfe, 0xdc, 0xba])
    );
    assert_ne!(sealed.colors, term.colors);
}
