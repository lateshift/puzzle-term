//! Frozen M8 model data for public built-in providers (design §11.6, D-34).
//!
//! Session remains the model owner. These reused snapshots carry no navigation
//! logic and give render-side views immutable source data. Session-owned pointer
//! callbacks return typed actions that only Session applies to the live M8 models.
use crate::geometry::Rect;
/// One cached marker interval; semantic tokens preserve live theme changes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Marker {
    /// Fractional start in the complete retained document.
    pub start: f64,
    /// Fractional end, including the final source row.
    pub end: f64,
    /// Theme token selected from the source kind and command exit status.
    pub token: &'static str,
}
/// Search presentation sampled from Session's input mode and search model.
#[derive(Default)]
pub struct SearchBar {
    /// Desktop opens the field; web hosts retain their own DOM UI.
    pub open: bool,
    /// Committed query text, copied with capacity reuse on actual changes.
    pub query: String,
    /// UTF-8 byte boundary for the field's insertion caret.
    pub cursor: usize,
    /// Uncommitted input-method candidate, never searched or sent to the PTY.
    pub preedit: String,
    /// Number of discovered matches from the incremental M8 engine.
    pub count: usize,
    /// Current zero-based match, if navigation selected one.
    pub current: Option<usize>,
    /// Partial queries display a progress indicator beside their count.
    pub complete: bool,
}
/// Copy-only geometry and reused metadata at the late source boundary.
#[derive(Default)]
pub struct Models {
    /// Host UI in logical points; absent surfaces retain the original placement path.
    pub occluded_rects: Option<std::sync::Arc<[Rect]>>,
    /// Debug observation is absent until a host explicitly shows the HUD.
    pub hud: hud::Hud,
    /// Harness-only admitted toasts; absent and allocation-free on other hosts.
    pub toasts: Option<std::sync::Arc<[toast::Toast]>>,
    /// Visible OSC output intervals; the view never extracts retained history.
    pub blocks: Vec<blocks::Block>,
    /// Validated current hover source, tied to presented row contents.
    pub link: link::Link,
    /// Effective shortcut help and the Session-owned filter mode.
    pub help: help::Help,
    /// Logical track rectangle converted once from M8 device geometry.
    pub track: Rect,
    /// Logical thumb rectangle, with M8's exact position and extent.
    pub thumb: Rect,
    /// Source-driven marker list; providers never scan hidden terminal rows.
    pub markers: Vec<Marker>,
    /// Monotonic source stamp for actual viewport movement, not redraw frequency.
    pub scroll_activity: u64,
    /// M8 new-output state, without a second counter or following flag.
    pub new_output: bool,
    /// The two query fields are Session modes; this is only their render view.
    pub search: SearchBar,
    /// Transient cursor and application status data, independent of grid cells.
    pub visuals: visuals::Visuals,
}
/// One pointer callback can request one operation on a Session-owned model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    /// Copy one retained completed command through normal clipboard policy.
    CopyZoneOutput(u64),
    /// Request opening the owner's current validated hovered target.
    OpenHoveredLink,
    /// Move the thumb through its available travel; zero is oldest, one live.
    DragThumb(f64),
    /// Page toward a point in the M8 track's device coordinate space.
    PageTrack(f64),
    /// Resume following live output and clear the new-output state.
    Bottom,
    /// Navigate the existing search result set by signed steps.
    SearchStep(i32),
    /// Close the desktop query field and clear its results.
    SearchClose,
    /// Activate or dismiss an admitted notification using its presented identity.
    Notification {
        /// Presented generation of the admitted notification.
        serial: u64,
        /// True selects the default action; false dismisses.
        activate: bool,
    },
}

/// Frozen source models retain vector/string storage between dirty publications.
impl Models {
    /// Copies source values without replacing warmed heap buffers (P1).
    pub fn copy_from(&mut self, source: &Self) {
        self.occluded_rects.clone_from(&source.occluded_rects);
        self.hud = source.hud;
        self.toasts.clone_from(&source.toasts);
        self.blocks.clone_from(&source.blocks);
        self.link.clone_from(&source.link);
        self.visuals.copy_from(&source.visuals);
        self.help.copy_from(&source.help);
        self.track = source.track;
        self.thumb = source.thumb;
        self.markers.clone_from(&source.markers);
        self.scroll_activity = source.scroll_activity;
        self.new_output = source.new_output;
        // Query text and composition are copied only into this slot's owned capacity.
        self.search.open = source.search.open;
        self.search.query.clone_from(&source.search.query);
        self.search.preedit.clone_from(&source.search.preedit);
        self.search.cursor = source.search.cursor;
        self.search.count = source.search.count;
        self.search.current = source.search.current;
        self.search.complete = source.search.complete;
    }
}

/// Immutable composition, suggestion and status sources for public painters.
pub mod visuals;

/// Immutable effective shortcut table and leader captions.
pub mod help;

/// Frozen link metadata with capacity-reusing snapshot copies.
pub mod link;

/// Frozen visible command intervals, without clock or clipboard ownership.
pub mod blocks;

/// Frozen optional frame statistics for the debug painter.
pub mod hud;

/// Source-only notification data, with no clocks or host handles.
pub mod toast;
