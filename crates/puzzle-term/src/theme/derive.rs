//! The §14.6 derivation table for UI tokens absent from source palettes.
//!
//! Every source file carries only terminal colors. Derived overlays are
//! local Puzzle design choices, never attributed to Catppuccin or another
//! palette author. Mixing is in OKLab and changes after inheritance.
use super::{Theme, UiTokens, color};

/// Applies the documented OKLab ratios to one resolved terminal palette.
pub(super) fn ui(theme: &Theme) -> UiTokens {
    let bg = theme.background;
    let fg = theme.foreground;
    let accent = theme.palette[4];
    // Search yellow and error red use terminal slots 3 and 1, so switching
    // from dark to light retains the original scheme's hue choices.
    let search = color::mix(bg, theme.palette[3], 0.40);
    let error = color::mix(bg, theme.palette[1], 0.55);
    let ok = color::mix(bg, theme.palette[2], 0.55);
    // The blue ANSI slot is a portable source accent; pack authors chose
    // that hue, while Puzzle chooses how an overlay uses it.
    // Alpha belongs only to the thumb, allowing host backgrounds to remain
    // visible when the scrollbar is placed outside the cell grid.
    UiTokens {
        accent,
        scrollbar_thumb: [fg[0], fg[1], fg[2], 89],
        scrollbar_track: color::mix(bg, fg, 0.08),
        // Two strengths distinguish a search hit from the active match
        // without introducing a new hue absent from the source palette.
        search_match: search,
        search_current: color::mix(bg, theme.palette[3], 0.65),
        link: theme.palette[6],
        zone_ok: ok,
        zone_error: error,
        // A badge needs stronger fill than a pill; its ink is selected by
        // WCAG contrast after the OKLab blend, avoiding a low-contrast label.
        badge_background: color::mix(bg, accent, 0.35),
        badge_foreground: color::higher_contrast(color::mix(bg, accent, 0.35), fg, bg),
        tooltip_background: color::mix(bg, fg, 0.12),
        tooltip_foreground: fg,
        // Pills and gutters recede into the default background. Keeping
        // their ratios small prevents a theme switch from flashing a panel.
        pill_background: color::mix(bg, fg, 0.10),
        pill_foreground: fg,
        bell_flash: color::mix(bg, theme.palette[3], 0.35),
        // Ghost text sits halfway to normal ink; preedit uses the accent
        // so composition remains identifiable on either appearance.
        ghost_text: color::mix(bg, fg, 0.55),
        preedit_underline: accent,
        gutter: color::mix(bg, fg, 0.04),
    }
}
