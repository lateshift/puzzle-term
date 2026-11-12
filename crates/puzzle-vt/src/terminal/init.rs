//! Terminal construction keeps cold defaults separate from parser entry points.
//! One write clock belongs to the complete Terminal, not either screen or feed.
//!
//! Construction allocates grid and bounded parser storage once. Reset cannot
//! simply call this constructor: vte may currently own the parser during RIS,
//! and replacing it would lose the bytes following that reset in the same feed.
//! In-place reset instead preserves the write clock and reusable allocations.
//!
//! Native and web callers supply the same dimensions/history bounds. There is
//! no host environment, time source, font context or platform policy here.
//! Empty optional transaction owners remain lazy until their first protocol
//! use. These defaults establish VT state; Session applies user settings before
//! application output starts, including the configured grapheme-width policy.
//!
//! Startup choices (the corresponding fields are documented on Terminal):
//! Clipboard/notification/drop storage must not allocate at startup.
//! UAPI context lifetime is session-scoped, separate from grid resets.
//! The native launcher may select the documented terminfo fallback.
//! Operation attribution is opt-in; ordinary feeds own no observer.
//! Zero denotes unwritten rows; both screen documents share writes.
//! Only the primary screen can retain history after scrolling.
//! A later out-and-back screen switch must still change the epoch.
//! Markers allocate only on demand, then share all movement rules.
//! Default colors stay unresolved until the theme-aware frame build.
//! Every color protocol writes this one table, never a parallel palette.
//! Hosts replace provisional cell pixels after their first text metrics.
//! Reuse existing query scratch; this capacity is not its wire limit.
//! D-31 starts clustering enabled; settings may change that default.
//! Default stops begin at column eight, excluding the initial cursor.
//! DECSC and kitty stacks keep primary/alternate saved state independent.
//! Cached mode flags avoid mode-table probes in each scalar store.
//! Only primary soft-wrapped text reflows; alternate contents truncate.
//! ISO-2022 starts all four G-sets as ASCII, with GL invoking G0.
use super::*;

/// Both screens start with identical, valid geometry and empty source state.
impl Terminal {
    /// Allocates both grids and their bounded, reusable rows.
    #[must_use]
    pub fn new(
        columns: usize,
        rows: usize,
        scrollback_lines: usize,
        scrollback_bytes: usize,
    ) -> Self {
        // A one-column screen cannot hold a wide character or its spacer.
        // Both bounds describe one valid grid: a wide-cell column pair and a row.
        let (columns, rows) = (columns.max(2), rows.max(1));
        Self::new_sized((columns, rows), (scrollback_lines, scrollback_bytes))
    }

    /// Initializes already-clamped geometry; both screens share one content clock.
    fn new_sized(grid: (usize, usize), history: (usize, usize)) -> Self {
        let (columns, rows) = grid;
        let (scrollback_lines, scrollback_bytes) = history;
        Self {
            host_protocols: Default::default(),
            contexts: Vec::new(),
            clipboard_grant_epoch: 0,
            term_name: "xterm-puzzle",
            operation_metrics: None,
            write_clock: crate::WriteClock::default(),
            primary: Screen::new(columns, rows, scrollback_lines, scrollback_bytes),
            alternate: Screen::new(columns, rows, 0, 0),
            active: ActiveScreen::Primary,
            screen_epoch: 0,
            reset_epoch: 0,
            parsing: Default::default(),
            cursor: Cursor::default(),
            cursor_style: 0,
            tracked: TrackedPositions::default(),
            pen: Cell::erased(PackedColor::DEFAULT),
            events: Vec::new(),
            title: String::new(),
            icon_name: String::new(),
            title_stack: Vec::new(),
            cwd: None,
            colors: crate::color_table::ColorTable::default(),
            links: LinkTable::default(),
            zones: Zones::default(),
            pointer_shape: None,
            mouse_shift_capture: None,
            appearance_dark: true,
            cell_pixels: (8, 16),
            dcs_kind: crate::perform::dcs::DcsKind::None,
            dcs_payload: Vec::with_capacity(128),
            dcs_truncated: false,
            replies: Vec::new(),
            modes: Modes::default(),
            grapheme_width_unicode: true,
            alternate_scroll_default: true,
            kitty_keys: KittyModes::default(),
            margins: Margins::full(columns, rows),
            tabs: initial_tabs(columns),
            grapheme: Default::default(),
            last_printed: None,
            saved: [None, None],
            current_link: None,
            underline_color: None,
            autowrap: true,
            insert_mode: false,
            lrmm_enabled: false,
            lnm: false,
            ambiguous_width_two: false,
            reflow_enabled: true,
            reflow_generation: 0,
            change_log: crate::view::ChangeLog::default(),
            charsets: [Charset::Ascii; 4],
            gl: 0,
            single_shift: None,
        }
    }
}
