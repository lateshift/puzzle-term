//! Configured cursor defaults and application precedence (§14.2).
//!
//! VT stores the last DECSCUSR request so DECRQSS can answer it unchanged.
//! Rendering chooses an effective shape separately. This distinction lets
//! `allow_app_style=false` hold a user's preferred caret while still telling
//! the application which style it requested. The same decision controls
//! drawing and blink scheduling, avoiding a hidden cursor timer for a
//! setting that has made the caret steady.

/// Shape chosen in user settings or after an application style request.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CursorStyle {
    /// Filled cell, with its glyph redrawn in the cursor text color.
    #[default]
    Block,
    /// Thin leading-edge vertical caret.
    Bar,
    /// Thin bottom-edge horizontal caret.
    Underline,
    /// Outlined cell, including while the window still has focus.
    BlockHollow,
}

/// User control over the blink source, above VT mode 12 and DECSCUSR.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CursorBlink {
    /// Follow VT mode 12 and an application's steady-style choice.
    #[default]
    Auto,
    /// Blink even after an application chose a steady DECSCUSR code.
    On,
    /// Never hide the caret in a blink phase.
    Off,
}

/// Cursor paint when OSC 12 has no active application override.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CursorColor {
    /// Use the theme's cursor and cursorText pair.
    #[default]
    Theme,
    /// Use the resolved cell foreground and background as a legible pair.
    Inverse,
    /// Draw the cursor with this exact sRGB color.
    Fixed([u8; 3]),
}

impl CursorColor {
    /// Parses only the three §14.2 color forms and rejects malformed input.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "theme" => Ok(Self::Theme),
            "inverse" => Ok(Self::Inverse),
            _ if value.len() == 7 && value.starts_with('#') => {
                let byte = |range| {
                    u8::from_str_radix(&value[range], 16)
                        .map_err(|_| "cursor color must be #rrggbb".to_owned())
                };
                Ok(Self::Fixed([byte(1..3)?, byte(3..5)?, byte(5..7)?]))
            }
            _ => Err("cursor color must be theme, inverse or #rrggbb".into()),
        }
    }
}

/// User settings applied live without mutating the terminal's VT request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CursorSettings {
    /// Shape used when DECSCUSR is zero or app styles are disallowed.
    pub style: CursorStyle,
    /// Forced or automatic blink rule.
    pub blink: CursorBlink,
    /// Whether nonzero DECSCUSR shapes can override the configured shape.
    pub allow_app_style: bool,
    /// Theme, cell-inverse or literal cursor RGB.
    pub color: CursorColor,
}

impl Default for CursorSettings {
    /// Allows application style changes until the user fixes the caret.
    fn default() -> Self {
        Self {
            style: CursorStyle::Block,
            blink: CursorBlink::Auto,
            allow_app_style: true,
            color: CursorColor::Theme,
        }
    }
}

/// Shape and blink result shared by the frame builder and Session timer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CursorPresentation {
    /// Effective on-screen geometry.
    pub style: CursorStyle,
    /// Whether the 530-ms phase can hide this caret.
    pub blinks: bool,
}

impl CursorSettings {
    /// Resolves DECSCUSR 0–6 and mode 12 without changing VT state.
    #[must_use]
    pub fn presentation(self, app_style: u8, mode_blink: bool) -> CursorPresentation {
        let use_app = self.allow_app_style && (1..=6).contains(&app_style);
        let style = if use_app {
            match app_style {
                3 | 4 => CursorStyle::Underline,
                5 | 6 => CursorStyle::Bar,
                _ => CursorStyle::Block,
            }
        } else {
            self.style
        };
        // Even DECSCUSR codes request a steady caret. An ignored app style
        // cannot change either geometry or blink when allowAppStyle is off.
        let steady_app = use_app && app_style.is_multiple_of(2);
        let blinks = match self.blink {
            CursorBlink::On => true,
            CursorBlink::Off => false,
            CursorBlink::Auto => mode_blink && !steady_app,
        };
        CursorPresentation { style, blinks }
    }
}

#[cfg(test)]
/// Table-driven shape, blink and input validation regressions.
mod tests;
