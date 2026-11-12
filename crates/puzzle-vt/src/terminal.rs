//! Platform-neutral owner of two screens and the active cursor (§6.1).
//!
//! This initial model exposes only grid state. M5.2 adds a bounded parser;
//! later work-order tasks fill in editing, color and input semantics.
//! The two screens deliberately share a row representation: changing to an
//! alternate buffer must not reinterpret widths, styles or damage marks.
//! The active screen is selected once at access time, leaving the storage
//! borrowable without copying a row or exposing platform-specific state.
/// Auxiliary execution contexts survive resets of the screen.
mod contexts;
/// Lazy host-facing transactions keep native protocol storage out of screen state.
mod host_protocols;
/// Construction clamps grid geometry before allocating screen owners.
mod init;

use crate::{
    cell::{Cell, PackedColor},
    input::KittyModes,
    links::LinkTable,
    modes::Modes,
    screen::Screen,
    tracked::{LineId, TrackedPositions},
    zones::Zones,
};

/// User-requested screen edits keep the output parser intact.
mod actions;
/// Borrowed buffer, markers and frame-level change publication.
mod contents;
/// C0 cursor and charset controls shared with vte callbacks.
mod controls;
/// Cursor and margin-aware CSI movement (§6.5).
mod cursor;
/// Inclusive scroll regions share bounded full-grid initialization.
mod margins;
/// Sparse OSC 66 placement/editing uses only the active screen owner.
mod multicell;
/// Bounded parser turns can yield at a registered extension without reordering replies.
mod parsing;
pub use margins::Margins;
/// Erase, insertion and scrolling operations (§6.5).
mod editing;
/// Opt-in operation attribution contains only scalar counts, never clocks or locks.
mod operation_metrics;
/// Scalar printing, cluster joining and wide-cell repair.
mod print;
/// RIS and DECSTR reset the visible state without replacing the parser.
mod reset;
/// Resizing keeps cursor and tracked anchors on their logical text.
mod resize;
/// Opt-in source write receipts remain generic and synchronous.
mod watches;
/// Which grid receives output and appears in a future buffer view.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActiveScreen {
    /// Full-width main grid with bounded scrollback.
    Primary,
    /// Program-owned grid without scrollback.
    Alternate,
}

/// Top-origin cursor and delayed-autowrap state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Cursor {
    /// Visible row, never beyond the screen height.
    pub row: usize,
    /// Visible column, never beyond the screen width.
    pub column: usize,
    /// The next printable character wraps after a last-column write.
    pub pending_wrap: bool,
}

/// DECSC remembers graphic state as well as the visible position (§6.5).
#[derive(Clone, Copy, Debug)]
struct SavedCursor {
    /// Position and delayed-wrap state at the save point.
    cursor: Cursor,
    /// Pen attributes must return with the cursor for full-screen apps.
    pen: Cell,
    /// The four designated graphic sets are part of DECSC state.
    charsets: [Charset; 4],
    /// Active GL invocation is restored with designated sets.
    gl: usize,
    /// DECOM changes the coordinate space of the saved position.
    origin: bool,
}

/// Stateful terminal core, independent of wall time, I/O, fonts and buses.
pub struct Terminal {
    /// Mode resets revoke grants even when reset and re-enable share one parser slice.
    pub clipboard_grant_epoch: u64,
    /// Optional clipboard, notification and drag transactions share one lazy-storage owner.
    pub(crate) host_protocols: host_protocols::HostProtocols,
    /// Active context path allocates only after OSC 3008 is used.
    pub(crate) contexts: Vec<crate::context::Context>,
    /// Spawn TERM echoed by XTGETTCAP; the platform selects the fallback.
    pub term_name: &'static str,
    /// Optional operation counts used only by an explicitly observing host.
    operation_metrics: Option<[u64; 10]>,
    /// One sequence stamps content writes on both screens; RIS never resets it.
    pub(crate) write_clock: crate::WriteClock,
    /// Main grid and bounded scrollback.
    pub primary: Screen,
    /// Alternate grid without scrollback.
    pub alternate: Screen,
    /// The grid selected by a DEC private mode.
    pub active: ActiveScreen,
    /// Changes on every screen transition, even when a feed switches twice.
    pub screen_epoch: u64,
    /// Full resets invalidate optional extension state; soft resets and screen switches do not.
    pub reset_epoch: u64,
    /// Bounded parser and raw framing move together during callback borrowing.
    parsing: parsing::State,
    /// Current position and pending wrap on the active screen.
    pub cursor: Cursor,
    /// DECSCUSR style 0–6, interpreted by the future frame builder.
    pub cursor_style: u8,
    /// Shared handles for positions that must survive scrolling.
    pub tracked: TrackedPositions,
    /// Pen template copied by the print path before setting the scalar.
    pub pen: Cell,
    /// Pending facts drained by the session, never coupled to puzzle-bus.
    pub events: Vec<TerminalEvent>,
    /// OSC title and icon text, each limited to 1024 characters.
    pub title: String,
    /// OSC 0/1 icon label independent of the window title.
    pub icon_name: String,
    /// Bounded CSI 22/23 t snapshots of both title strings.
    pub title_stack: Vec<(String, String)>,
    /// OSC 7 working-directory URI, treated as untrusted text.
    pub cwd: Option<String>,
    /// Unified theme defaults and overrides, shared by xterm and kitty OSCs.
    pub colors: crate::color_table::ColorTable,
    /// OSC 8 targets with generation-checked, bounded handles.
    pub links: LinkTable,
    /// OSC 133/633 marks anchored by monotonic line identity.
    pub zones: Zones,
    /// OSC 22 cursor name selected from the safe CSS allowlist.
    pub pointer_shape: Option<String>,
    /// XTSHIFTESCAPE program preference; None defers to the user default.
    pub mouse_shift_capture: Option<bool>,
    /// Dark/light answer used by CSI ? 996 n until theme wiring in M6.
    pub appearance_dark: bool,
    /// Device-pixel cell size for CSI 14/16 t responses.
    pub cell_pixels: (u16, u16),
    /// Recognized DCS request kind during a bounded passthrough string.
    pub(crate) dcs_kind: crate::perform::dcs::DcsKind,
    /// At most 4096 DCS bytes retained for status or termcap requests.
    pub(crate) dcs_payload: Vec<u8>,
    /// A request beyond the DCS cap is discarded at termination.
    pub(crate) dcs_truncated: bool,
    /// Replies collected for the Session to send in parser order.
    pub replies: Vec<u8>,
    /// Supported ANSI and DEC private switches, including report state.
    pub modes: Modes,
    /// Configured startup value of mode 2027, restored by RIS and DECSTR.
    pub grapheme_width_unicode: bool,
    /// Configured alternate-scroll policy restored by RIS and DECSTR.
    pub(crate) alternate_scroll_default: bool,
    /// Kitty keyboard flags and bounded mode stacks remain screen-local.
    pub kitty_keys: KittyModes,
    /// Active scroll and horizontal regions.
    pub margins: Margins,
    /// Explicit tab stops; reset defaults are eight columns apart.
    pub tabs: Vec<bool>,
    /// Scalar facts and one validated joined suffix share the same cache lifetime.
    grapheme: print::GraphemeCache,
    /// The last scalar used by bounded REP.
    pub last_printed: Option<char>,
    /// Per-screen DECSC snapshots; an alternate-screen switch does not mix them.
    saved: [Option<SavedCursor>; 2],
    /// The current hyperlink and underline extras join new cells in M5.4.
    pub current_link: Option<u32>,
    /// SGR 58 color stored sparsely on newly printed cells.
    pub underline_color: Option<PackedColor>,
    /// Whether the next printable after the last column wraps first.
    pub autowrap: bool,
    /// Cached ANSI IRM flag so the ASCII print path does no mode-table lookup.
    pub insert_mode: bool,
    /// Cached DECLRMM flag for CR/HT in dense text streams.
    pub lrmm_enabled: bool,
    /// ANSI mode 20 makes LF, VT and FF also return to the left margin.
    pub lnm: bool,
    /// Ambiguous-width code points use two cells when CJK width is selected.
    pub ambiguous_width_two: bool,
    /// Primary soft-wrapped lines reflow when the width changes.
    pub reflow_enabled: bool,
    /// Counts primary width reflows so selections can distinguish remap from rewrite.
    pub reflow_generation: u64,
    /// One bounded entry per host frame for external line observers (§6.13).
    pub(crate) change_log: crate::view::ChangeLog,
    /// The four ISO-2022 designations selected by ESC (/)/*/+.
    charsets: [Charset; 4],
    /// G-set currently invoked into the printable GL range.
    gl: usize,
    /// One-print override from SS2 or SS3.
    single_shift: Option<usize>,
}

/// One designated graphic set for the 7-bit printable range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Charset {
    /// Unmodified ASCII printable characters.
    Ascii,
    /// DEC line-drawing and symbol replacements.
    DecSpecial,
    /// UK national replacement of `#` by `£`.
    Uk,
}

/// Facts emitted by byte processing for a later Session to publish.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminalEvent {
    /// BEL was received; sound and flash remain host policy decisions.
    Bell {
        /// Cursor line at the instant BEL was parsed.
        line: Option<LineId>,
    },
    /// OSC 0/2 changed the bounded window title.
    TitleChanged(String),
    /// OSC 0/1 changed the bounded icon label.
    IconNameChanged(String),
    /// OSC 7 announced an application working-directory URI.
    CwdChanged(String),
    /// Context path changed; consumers borrow the final path at the drain boundary.
    ContextsChanged,
    /// OSC 52 requested a policy-controlled clipboard write.
    ClipboardWrite(String),
    /// Complete MIME operation after bounded protocol validation.
    ClipboardRequest(crate::clipboard::Request),
    /// Kitty notification transaction; host policy and completion belong to Session.
    DesktopNotification(crate::notification::Request),
    /// Drop target lifecycle and acceptance, never a platform callback.
    Drop(crate::dnd::Effect),
    /// OSC 9 or 777 requested a bounded host notification.
    Notification {
        /// Short notification heading, empty for plain OSC 9.
        title: String,
        /// Bounded message text for host policy to display or suppress.
        body: String,
    },
    /// OSC 9;4 reported progress state and percentage.
    Progress {
        /// ConEmu state code (0–4).
        state: u8,
        /// Reported completion clamped to 0–100.
        percent: u8,
    },
    /// OSC 22 requested one permitted pointer shape.
    PointerShape(String),
    /// Clock-free identity for Session-owned OSC timing, before the legacy fact.
    ZoneBoundary {
        /// VT assigns identities independently of untrusted OSC parameters.
        id: u64,
        /// Only output and completion alter execution duration.
        kind: crate::zones::ZoneKind,
    },
    /// OSC 133/633 marked the start of a shell prompt.
    PromptStarted,
    /// OSC 133/633 marked the beginning of command input.
    InputStarted,
    /// OSC 133/633 output boundary with optional bounded kitty command metadata.
    OutputStarted(Option<String>),
    /// OSC 133/633 marked the end of a command, optionally with exit status.
    CommandFinished(Option<i32>),
    /// A grid resize completed after primary and alternate buffers changed.
    GridResized {
        /// New visible width in cells.
        columns: usize,
        /// New visible height in cells.
        rows: usize,
    },
}

impl Terminal {
    /// Selects the startup width model without changing the live mode.
    ///
    /// Settings apply this before output starts; DECSET/DECRST can then
    /// change the live value until RIS or DECSTR restores this choice.
    pub fn set_grapheme_width_default(&mut self, unicode: bool) {
        self.grapheme_width_unicode = unicode;
        self.modes.set(true, 2027, unicode);
    }

    /// Sets live mode 1007 and the policy restored after terminal resets.
    pub fn set_alternate_scroll_default(&mut self, enabled: bool) {
        self.alternate_scroll_default = enabled;
        self.modes.set(true, 1007, enabled);
    }

    /// Returns the visible grid's dimensions.
    #[must_use]
    pub fn size(&self) -> (usize, usize) {
        // Both screens start with the same geometry; use the active one so
        // later independent resize handling cannot leave a stale answer.
        let ring = &self.screen().ring;
        (ring.columns(), ring.visible_rows())
    }

    /// Returns the active grid without copying cells.
    #[must_use]
    pub fn screen(&self) -> &Screen {
        match self.active {
            ActiveScreen::Primary => &self.primary,
            ActiveScreen::Alternate => &self.alternate,
        }
    }

    /// Returns the active grid for in-place parser edits.
    pub fn screen_mut(&mut self) -> &mut Screen {
        match self.active {
            ActiveScreen::Primary => &mut self.primary,
            ActiveScreen::Alternate => &mut self.alternate,
        }
    }

    /// Splits the independent screen and write-sequence owners for one mutation.
    /// Every store receives the same Terminal clock, including inactive-screen work.
    pub(crate) fn screen_clock_mut(&mut self) -> (&mut Screen, &mut crate::WriteClock) {
        let screen = match self.active {
            ActiveScreen::Primary => &mut self.primary,
            ActiveScreen::Alternate => &mut self.alternate,
        };
        (screen, &mut self.write_clock)
    }

    /// Edits one visible row for a local host notice, recording one content write.
    /// This cold seam keeps direct host text out of the escape-sequence parser.
    pub fn edit_row(&mut self, index: usize, edit: impl FnOnce(&mut crate::Row)) {
        let (screen, clock) = self.screen_clock_mut();
        if let Some(row) = screen.row_mut(index) {
            edit(row);
            row.version = clock.written::<true>(row, 0..row.cells.len() + 1);
            screen.damage.mark(index);
        }
    }

    /// Latest global source stamp; unobserved rows keep local versions instead.
    /// Register observation before capturing a witness that compares this value.
    pub fn write_clock(&self) -> u64 {
        self.write_clock.latest()
    }

    /// Returns the active cursor's line identity.
    #[must_use]
    pub fn cursor_line(&self) -> Option<LineId> {
        // Future overlays need identity, not the row's current screen index.
        self.screen().row(self.cursor.row).map(|row| row.id)
    }

    /// Moves one line down, scrolling at the bottom of the visible grid.
    pub fn line_feed(&mut self) {
        // Test-only hard bounds catch non-progressing callers without a timed worker.
        #[cfg(test)]
        crate::scroll_operations::line_feed_step();
        let (columns, rows) = self.size();
        // LF is a cursor movement even when no visible scroll occurs.
        self.cursor.pending_wrap = false;
        let bottom = self.margins.bottom.min(rows - 1);
        if self.cursor.row != bottom {
            self.cursor.row = (self.cursor.row + 1).min(rows - 1);
            return;
        }
        // The ordinary full-width LF stays on the recycled-row fast path.
        // A partial region needs the more general scroll-region machinery.
        if self.margins.top == 0
            && bottom == rows - 1
            && self.margins.left == 0
            && self.margins.right == columns - 1
        {
            self.prepare_multicell_scroll(0, rows - 1, 1, true);
            self.note_operation(operation_metrics::Operation::ScrollUp, 1);
            let background = self.pen.bg;
            match self.active {
                ActiveScreen::Primary => {
                    self.primary
                        .scroll_up(&mut self.tracked, background, &mut self.write_clock)
                }
                ActiveScreen::Alternate => {
                    self.alternate
                        .scroll_up(&mut self.tracked, background, &mut self.write_clock)
                }
            }
        } else {
            self.scroll_region_up(1);
        }
    }
}

#[cfg(test)]
#[path = "terminal/tests.rs"]
mod tests;

/// Default tab stops every eight columns exclude the initial cursor position.
fn initial_tabs(columns: usize) -> Vec<bool> {
    (0..columns)
        .map(|column| column != 0 && column % 8 == 0)
        .collect()
}

/// Source consumers activate global ordering only for their registered lifetime.
mod observation;
/// Private source capacity is admitted before any owner or tracker grows.
mod source_capacity;
