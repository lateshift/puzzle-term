//! Theme sources, inheritance and resolved terminal colors (§14.6).
//!
//! Terminal colors in generated seeds are byte-exact copies of pinned
//! upstream files. UI colors absent from those sources are derived only
//! after inheritance, so a custom background recolors its overlays too.
//! No parser or filesystem reaches this neutral crate; platforms convert
//! their config object to the bus Value tree before registration.
use puzzle_bus::Value;
use std::collections::BTreeMap;
/// sRGB, linear-light, OKLab and WCAG contrast operations.
pub mod color;
/// Names the OKLab rules for every UI token absent from source palettes.
mod derive;
/// Rust constants regenerated from reviewed JSON assets.
mod generated;
/// Validates custom themes and resolves inheritance without recursion loops.
mod registry;
/// Interpolates resolved colors for an explicitly configured appearance fade.
mod transition;
pub use registry::ThemeRegistry;

/// Host appearance used for theme-pair selection and mode-2031 replies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Appearance {
    /// White-backed terminal and system light controls.
    Light,
    /// Dark-backed terminal and system dark controls.
    Dark,
}

impl Appearance {
    /// Returns the VT 2031 mode reply suffix for this appearance.
    pub const fn report(self) -> &'static [u8] {
        crate::vt::Terminal::appearance_report(matches!(self, Self::Dark))
    }

    /// Returns the shell's traditional foreground/background hint.
    pub const fn colorfgbg(self) -> &'static str {
        match self {
            Self::Dark => "15;0",
            Self::Light => "0;15",
        }
    }
}

/// Exact terminal colors from one reviewed source document.
#[derive(Clone, Copy)]
struct ThemeSeed {
    /// Stable setting and catalogue name.
    name: &'static str,
    /// Human-facing gallery label.
    display_name: &'static str,
    /// Source-declared light or dark variant.
    appearance: &'static str,
    /// Upstream repository, pinned commit and verified license.
    credit: &'static str,
    /// Default terminal glyph color.
    foreground: [u8; 3],
    /// Default terminal clear and cell background.
    background: [u8; 3],
    /// Cursor block color unless OSC 12 overrides it.
    cursor: Option<[u8; 3]>,
    /// Redrawn glyph inside a block cursor.
    cursor_text: Option<[u8; 3]>,
    /// Selection surface, copied from source when available.
    selection_background: Option<[u8; 3]>,
    /// Source selection ink, or a derived contrast choice.
    selection_foreground: Option<[u8; 3]>,
    /// ANSI slots 0–15 in their source order.
    palette: [[u8; 3]; 16],
}

/// Named UI colors derived through the §14.6 OKLab table.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UiTokens {
    /// Accent for controls and focused outlines.
    pub accent: [u8; 3],
    /// Thumb over a terminal-adjacent scrollbar.
    pub scrollbar_thumb: [u8; 4],
    /// Scrollbar track continuous with the surface.
    pub scrollbar_track: [u8; 3],
    /// Ordinary search match highlight.
    pub search_match: [u8; 3],
    /// Active search match highlight.
    pub search_current: [u8; 3],
    /// Link decoration and hover color.
    pub link: [u8; 3],
    /// Success status tint.
    pub zone_ok: [u8; 3],
    /// Error status tint.
    pub zone_error: [u8; 3],
    /// Badge background.
    pub badge_background: [u8; 3],
    /// Badge text.
    pub badge_foreground: [u8; 3],
    /// Tooltip background.
    pub tooltip_background: [u8; 3],
    /// Tooltip text.
    pub tooltip_foreground: [u8; 3],
    /// Subtle pill surface.
    pub pill_background: [u8; 3],
    /// Pill text.
    pub pill_foreground: [u8; 3],
    /// BEL flash overlay.
    pub bell_flash: [u8; 3],
    /// Ghost text that recedes behind ordinary foreground.
    pub ghost_text: [u8; 3],
    /// Input-method preedit underline.
    pub preedit_underline: [u8; 3],
    /// Side gutter next to terminal cells.
    pub gutter: [u8; 3],
}

/// One fully resolved theme, ready for frame color lookup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Theme {
    /// Stable selector name.
    pub name: String,
    /// Gallery label.
    pub display_name: String,
    /// Declared light or dark appearance.
    pub appearance: Appearance,
    /// Source or custom-theme attribution.
    pub credit: String,
    /// Default glyph ink.
    pub foreground: [u8; 3],
    /// Default clear and cell background.
    pub background: [u8; 3],
    /// Default cursor block.
    pub cursor: [u8; 3],
    /// Default glyph ink under a cursor block.
    pub cursor_text: [u8; 3],
    /// Selection surface.
    pub selection_background: [u8; 3],
    /// Selection glyph ink.
    pub selection_foreground: Option<[u8; 3]>,
    /// ANSI slots that remain overrideable by OSC 4.
    pub palette: [[u8; 3]; 16],
    /// Derived colors for UI and overlays.
    pub ui: UiTokens,
}

impl Theme {
    /// Creates a resolved theme from one byte-exact generated seed.
    fn from_seed(seed: ThemeSeed) -> Self {
        let appearance = if seed.appearance == "light" {
            Appearance::Light
        } else {
            Appearance::Dark
        };
        // Only optional selection ink is derived; all other terminal
        // bytes above came directly from the approved source document.
        let cursor = seed
            .cursor
            .unwrap_or_else(|| color::mix(seed.background, seed.foreground, 0.85));
        let cursor_text = seed
            .cursor_text
            .unwrap_or_else(|| color::higher_contrast(cursor, seed.background, seed.foreground));
        let selection_background = seed
            .selection_background
            .unwrap_or_else(|| color::mix(seed.background, seed.foreground, 0.25));
        let selection_foreground = seed.selection_foreground;
        let mut theme = Self {
            name: seed.name.into(),
            display_name: seed.display_name.into(),
            appearance,
            credit: seed.credit.into(),
            foreground: seed.foreground,
            background: seed.background,
            cursor,
            cursor_text,
            selection_background,
            selection_foreground,
            palette: seed.palette,
            ui: UiTokens::default(),
        };
        theme.ui = derive::ui(&theme);
        theme
    }

    /// Resolves a named built-in without reading JSON at runtime.
    #[must_use]
    pub fn builtin(name: &str) -> Option<Self> {
        generated::BUILTINS
            .iter()
            .find(|seed| seed.name == name)
            .copied()
            .map(Self::from_seed)
    }

    /// Lists the exact stable selectors in generated source order.
    #[must_use]
    pub fn builtin_names() -> Vec<&'static str> {
        generated::BUILTINS.iter().map(|seed| seed.name).collect()
    }

    /// Recomputes absent UI values after a custom override changes colors.
    fn rederive(&mut self) {
        self.ui = derive::ui(self);
    }
}

/// Custom definitions arrive as the same neutral map used by settings.
type CustomMap = BTreeMap<String, Value>;

#[cfg(test)]
/// Theme source, derivation and inheritance tests.
mod tests;

/// Shared installation of authoritative terminal colour defaults.
mod terminal_colors;
