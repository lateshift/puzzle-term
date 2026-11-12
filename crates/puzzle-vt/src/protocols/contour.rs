//! Contour dark/light notifications and Unicode mode negotiation (D-58).
//! References: <https://github.com/contour-terminal/contour/blob/master/docs/vt-extensions/color-palette-update-notifications.md>
//! and <https://github.com/contour-terminal/terminal-unicode-core>.
//! Mode storage and Unicode clustering stay in the shared terminal model.
use crate::Terminal;

impl Terminal {
    /// Encodes Contour's mode-2031 appearance report for a theme or a query.
    /// It exposes no user data and allocates nothing.
    pub const fn appearance_report(dark: bool) -> &'static [u8] {
        if dark {
            b"\x1b[?997;1n"
        } else {
            b"\x1b[?997;2n"
        }
    }
}
/// Answers the explicit 996 query using the terminal's resolved appearance.
pub(crate) fn appearance(term: &mut Terminal) {
    term.replies
        .extend_from_slice(Terminal::appearance_report(term.appearance_dark));
}
