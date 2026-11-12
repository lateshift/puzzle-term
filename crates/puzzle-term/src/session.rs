//! One platform-neutral terminal session and its bounded feed loop (§4.5).
//!
//! The session owns VT state, a typed bus, pending transport bytes and host
//! service traits. Application output runs through puzzle-vt directly; the
//! bus carries coarse input commands and terminal facts only (P8).
//! A 64 KiB slice is the largest parser call. After each slice the injected
//! clock decides whether the caller's time allowance has expired, leaving
//! every remaining byte in order for a later pump. No byte is dropped.
//!
//! Replies are written after each slice, before parsing later bytes. An
//! application querying cursor state between two large output chunks sees
//! its response in protocol order. Input commands drain synchronously in
//! `pump`, so a key reaches Transport::write without waiting for a frame.
//! The host’s FrameBuilder reuses row geometry between presentations; the
//! scheduler keeps its cursor and synchronized-output timers outside VT.
//! The host provides `Instant` and services. VT stays free of clocks and I/O.
//!
//! The pending vector is append-only during a feed call, then drops one
//! parsed prefix. This guarantees parser state sees the same byte order
//! whether a transport delivers a sequence in one packet or several.
//! An oversized backlog warns at 64 MiB but is never truncated (§16):
//! dropping the middle of an escape sequence would corrupt the screen.
//! The transport owns actual backpressure outside this synchronous core.
//! `feed` invokes `pump` after moving VT facts into the bus, matching the
//! drain points in §5.3. A host can call `pump` separately immediately
//! after an input event, with no pending output and no render request.
//! The initial blank frame lets a newly created surface present its theme
//! background once. Subsequent idle calls return None and schedule nothing.
use crate::{
    messages::{Command, Event, ExitInfo},
    platform::Services,
    vt::Terminal,
};
use puzzle_bus::{Bus, Origin};
use std::time::Duration;
use web_time::Instant;
/// Explicit host acknowledgements bound background output notifications.
mod activity;
/// Shortcut state and host action handoff.
mod bindings;
/// Typed input, host policy and child-exit command handling.
mod commands;
/// Cursor preferences and the effective application-style decision (§14.2).
pub mod cursor_settings;
/// Declarative definition ownership and source lifetime validation.
pub mod decorations;
/// Native drag observations use the geometry of the last presented frame.
mod dnd;
/// Coarse facts produced by VT output.
mod events;
/// Program-exit policy and direct held-screen notice (§4.7).
mod exit;
/// Budgeted parser feeding and post-parse service work.
mod feed;
/// Font, DPR and grid changes that cross the host geometry seam.
mod geometry;
/// Typed deferred host work and native textarea ownership after bus policy.
mod host;
/// Effective default sRGB observations for replaceable host chrome.
mod host_colors;
/// Owned results for native event channels and single-threaded web callbacks.
pub mod host_events;
/// Small input and animation facts keep native owners free of synchronous queries.
pub mod host_state;
/// Optional kitty protocol and ordered asynchronous decode completion.
mod images;
/// Value messages accepted by the singular owner, on every target (D-50).
pub mod message;
/// M8 model snapshots, provider actions and desktop query input mode.
mod model_overlays;
/// Refresh-coalesced physical motion uses current owner-side protocol policy.
mod motion;
/// UI notification feedback and immutable toast-source publication.
mod notifications;
/// Native UI exclusion metadata never changes grid or PTY dimensions.
mod occlusion;
/// Fixed scalar VT operation counts attribute work within a Session-owner turn.
mod operation_metrics;
/// Host preference for the two physical macOS Option keys.
mod option_key;
/// Registered OSC dispatch yields preserve source-order lifecycle and replies.
mod osc;
/// Provider ownership and copy-only desktop handoff.
mod overlays;
/// Shared burst-settling rule for host display clocks.
pub mod presentation;
/// Damage publication fills a caller-owned slot without touching glyph resources.
pub mod publication;
/// Nonblocking program-reply credit and ordered large-response continuation.
pub(crate) mod replies;
mod schedule;
/// Bounded query slices and cached match presentation.
mod search;
/// Selection queries and source reconciliation.
mod selection;
/// Applies validated settings to the live terminal and frame policy.
mod settings;
/// Host appearance and resolved color transactions (§14.7).
mod theme;
/// Anchored viewport and source-driven scrollbar updates.
mod viewport;
/// Fractional wheel reports remain owned by the same Session as input modes.
mod wheel;
pub use option_key::OptionAsAlt;

/// Host preference for a normally terminated program.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OnExit {
    /// Close the host surface after the final output has been parsed.
    Close,
    /// Retain the grid with an exit notice until the next key.
    Hold,
}

/// Initial grid and parser budget before the settings schema arrives.
#[derive(Clone, Copy, Debug)]
pub struct SessionConfig {
    /// TERM advertised to the child, also returned by XTGETTCAP TN.
    pub term_name: &'static str,
    /// Visible terminal columns at creation.
    pub columns: usize,
    /// Visible terminal rows at creation.
    pub rows: usize,
    /// Maximum retained primary scrollback lines.
    pub scrollback_lines: usize,
    /// Maximum retained primary scrollback bytes.
    pub scrollback_bytes: usize,
    /// Maximum parser time per feed call.
    pub parse_budget: Duration,
    /// Initial mode 2027 state until M6.5 wires unicode.graphemeWidth.
    pub grapheme_width_unicode: bool,
    /// Which macOS Option sides report terminal Alt instead of layout text.
    pub option_as_alt: OptionAsAlt,
    /// Minimum age of the first output in an idle burst before its snapshot.
    pub render_settle: Duration,
    /// Normal child-exit action until the settings system supplies it.
    pub on_exit: OnExit,
    /// Early abnormal exits remain visible even under Close.
    pub abnormal_exit_ms: u64,
    /// Cursor defaults before the settings schema supplies live values.
    pub cursor: cursor_settings::CursorSettings,
}

impl Default for SessionConfig {
    /// Uses the §6.3 history limits and §14.5 eight-millisecond budget.
    fn default() -> Self {
        Self {
            term_name: "xterm-puzzle",
            columns: 80,
            rows: 24,
            scrollback_lines: 10_000,
            scrollback_bytes: 32 * 1024 * 1024,
            parse_budget: Duration::from_millis(8),
            grapheme_width_unicode: true,
            option_as_alt: OptionAsAlt::None,
            render_settle: Duration::from_millis(8),
            on_exit: OnExit::Close,
            abnormal_exit_ms: 250,
            cursor: cursor_settings::CursorSettings::default(),
        }
    }
}

/// Work completed by one bounded output feed call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FeedOutcome {
    /// Bytes parsed in this call, including any earlier pending prefix.
    pub parsed_bytes: usize,
    /// Bytes retained for the next feed or pump.
    pub pending_bytes: usize,
    /// Parser slices completed before the budget expired.
    pub slices: usize,
}

/// All state for one terminal, excluding GPU and platform handles.
pub struct Session {
    /// Public extension callbacks return here between frame builds.
    overlays: puzzle_overlay::OverlayRegistry,
    /// Model contexts stay with interaction even while the renderer builds older state.
    overlay_source: overlays::source::Source,
    /// Built-ins own presentation only; this state retains the Session query mode.
    model_overlays: model_overlays::State,
    /// Annotation definitions and rewrite witnesses belong to this Session.
    decorations: decorations::Store,
    /// Invalidations accumulate while the desktop owns external callbacks.
    overlay_events: puzzle_overlay::provider::EventKinds,
    /// Stable injected origin for animation seconds.
    overlay_epoch: Option<Instant>,
    /// Accessibility state also applies to provider animations.
    overlay_reduced_motion: bool,
    /// Pure byte parser and both terminal screens.
    terminal: Terminal,
    /// Optional protocols own their state on the same owner as VT.
    osc: crate::osc::Registry,
    /// Native MIME permissions and correlation stay on the singular Session owner.
    clipboard: crate::clipboard::State,
    /// Notification correlations and policy are lazy, and owned exclusively here.
    notifications: crate::notification::State,
    /// Native hosts opt in; browser sessions retain their unsupported-APC path.
    graphics: Option<Box<images::State>>,
    /// Local scrollback position follows tracked content through output.
    viewport: crate::viewport::Viewport,
    /// Tracked selection and copy preferences belong to this document.
    selection: crate::selection::SelectionState,
    /// Local drag state follows tracked press cells through reflow and output.
    pointer: crate::pointer::Gesture,
    /// Protocol duplicate suppression belongs with the modes it samples.
    motion: motion::Motion,
    /// Incremental query state is lazy and belongs to this terminal.
    search: crate::search::SearchModel,
    /// Last local pointer cell supplies semantic command selection.
    selection_pointer: Option<crate::vt::selection::SelectPoint>,
    /// Sub-row application wheel input never crosses destination changes.
    wheel: wheel::Steps,
    /// Bounded bell locations for the future scrollbar overlay.
    bells: Vec<crate::vt::tracked::TrackedPos>,
    /// Resolved bell identities reuse capacity when marker sources change.
    bell_lines: Vec<crate::vt::tracked::LineId>,
    /// Provisional composition is UI state, never application input.
    preedit: crate::messages::PreeditState,
    /// Portable shortcut interceptor and pending leaders.
    keybindings: crate::keybind::Keybindings,
    /// Clock origin for deterministic sequence timeout milliseconds.
    binding_epoch: Option<Instant>,
    /// Shared user preference resolved against the live XTSHIFTESCAPE request.
    mouse_shift_capture: crate::settings::ShiftCapture,
    /// Host settings actions wait for the platform to apply them after pump.
    binding_actions: Vec<(crate::keybind::Action, bool)>,
    /// Host results remain owned here until the platform drains them.
    host_events: std::collections::VecDeque<host_events::HostEvent>,
    /// Opt-in bus observation keeps parser-only sessions free of host event copies.
    host_fact_hook: host_events::FactHook,
    /// The host receives one close result even when exit policy needs no held key.
    host_close_sent: bool,
    /// Output stays unread until the host explicitly acknowledges this session.
    host_activity_pending: bool,
    /// Last input fact permits change-only delivery during continuous parsing.
    host_input: Option<host_state::InputState>,
    /// Last delivered default paper/ink; unchanged output needs no colour event.
    host_colors: Option<([u8; 3], [u8; 3])>,
    /// Native UI ownership is independent of platform/security keybinding policy.
    search_ui: crate::search::SearchUi,
    /// Last delivered query progress lets native hosts maintain counts without querying Session.
    host_search: Option<crate::search::SearchState>,
    /// Last animation edge informs a host refresh source without a live query.
    host_animation: Option<bool>,
    /// Latest injected host turn supplies animation time without a platform clock read.
    host_time: Option<Instant>,
    /// Typed policy and fact lanes, drained only at defined points.
    bus: Bus<Command, Event>,
    /// The platform-owned capabilities used at the session seam.
    services: Services,
    /// Unparsed suffix retained in transport order across feed calls.
    pending: Vec<u8>,
    /// Every programmatic reply shares one completion-accounted writer lane.
    replies: replies::Lane,
    /// Lifetime consumed bytes include suffixes drained during publication and EOF.
    parsed_total: u64,
    /// Opt-in nested feed phases, owned by the neutral Session and read by its host.
    parse_feed_phases: Option<[Duration; 6]>,
    /// Resize report waits until the host sends the matching PTY SIGWINCH.
    pending_resize_reply: Vec<u8>,
    /// Per-call parse limit supplied by validated settings later in M6.
    parse_budget: Duration,
    /// A requested presentation is consumed by publication once.
    needs_frame: bool,
    /// Mutable drawing preferences remain with the Session owner.
    frame_policy: Option<crate::frame_builder::policy::FramePolicy>,
    /// Last captured visible blinking ink, independent of any render cache.
    blinking_ink: bool,
    /// Last capture inputs; failures reset this to revise every circulating slot.
    publication_key: Option<(u64, u64, u64)>,
    /// Copy epoch belongs to the Session rather than any renderer's row cache.
    publication_epoch: u64,
    /// First time mode 2026 withheld a changed frame.
    sync_started: Option<Instant>,
    /// Next cursor/text blink transition while activity is recent.
    blink_deadline: Option<Instant>,
    /// Blink stops fifteen seconds after its last input or appearance reset.
    last_blink_activity: Option<Instant>,
    /// Status exists only after the platform drained PTY output.
    exited: Option<ExitInfo>,
    /// A close policy or a key on the held screen asks the host to exit.
    close_requested: bool,
    /// Selected policy for ordinary process completion.
    on_exit: OnExit,
    /// Short unsuccessful starts override Close with a held notice.
    abnormal_exit_ms: u64,
    /// Effective cursor preferences independent of VT's stored request.
    cursor_settings: cursor_settings::CursorSettings,
    /// Active light/dark pair and custom theme registry.
    theme_state: theme::ThemeState,
}

impl Session {
    /// Creates one terminal without opening a PTY or selecting a GPU.
    #[must_use]
    pub fn new(mut config: SessionConfig, services: Services) -> Self {
        Self {
            overlays: puzzle_overlay::OverlayRegistry::default(),
            overlay_source: overlays::source::Source::default(),
            model_overlays: model_overlays::State::default(),
            decorations: decorations::Store::default(),
            overlay_events: puzzle_overlay::provider::EventKinds::ALL,
            overlay_epoch: None,
            overlay_reduced_motion: false,
            terminal: terminal_from_config(&mut config),
            osc: crate::osc::Registry::default(),
            clipboard: crate::clipboard::State::default(),
            notifications: crate::notification::State::default(),
            graphics: None,
            viewport: crate::viewport::Viewport::default(),
            selection: crate::selection::SelectionState::default(),
            selection_pointer: None,
            pointer: crate::pointer::Gesture::default(),
            motion: motion::Motion::default(),
            search: crate::search::SearchModel::default(),
            wheel: wheel::Steps::default(),
            bells: Vec::new(),
            bell_lines: Vec::new(),
            preedit: crate::messages::PreeditState::default(),
            keybindings: crate::keybind::Keybindings::default(),
            binding_epoch: None,
            mouse_shift_capture: crate::settings::ShiftCapture::False,
            binding_actions: Vec::new(),
            host_events: Default::default(),
            host_fact_hook: None,
            host_close_sent: false,
            host_activity_pending: false,
            host_input: None,
            host_colors: None,
            host_search: None,
            search_ui: crate::search::SearchUi::BuiltIn,
            host_animation: None,
            host_time: None,
            bus: Bus::default(),
            services,
            // Capacity grows only with backlog, then is reused across
            // later transport callbacks instead of allocating per slice.
            pending: Vec::new(),
            replies: replies::Lane::default(),
            parsed_total: 0,
            parse_feed_phases: None,
            pending_resize_reply: Vec::new(),
            parse_budget: config.parse_budget,
            needs_frame: true,
            frame_policy: None,
            blinking_ink: false,
            publication_key: None,
            publication_epoch: 0,
            sync_started: None,
            blink_deadline: None,
            last_blink_activity: None,
            exited: None,
            close_requested: false,
            on_exit: config.on_exit,
            abnormal_exit_ms: config.abnormal_exit_ms,
            cursor_settings: config.cursor,
            theme_state: theme::ThemeState::default(),
        }
    }

    /// Enqueues a policy-controlled command with a host-assigned origin.
    pub fn submit(&mut self, command: Command, origin: Origin) {
        // Revoke before interceptors: consumed input is still a user action (§16).
        if origin == Origin::User {
            self.clipboard.revoke_paste_grant();
        }
        self.bus.submit(command, origin);
    }

    /// Allows typed policy and observer registration by a host.
    pub fn bus_mut(&mut self) -> &mut Bus<Command, Event> {
        &mut self.bus
    }

    /// Gives the frame builder direct read-only access to VT state.
    #[must_use]
    pub fn terminal(&self) -> &Terminal {
        &self.terminal
    }
}

#[cfg(test)]
/// Manual transport and clock checks for chunking, replies and idle work.
mod tests;

/// Constructs VT before Session state, preserving the configured width and terminfo policy.
fn terminal_from_config(config: &mut SessionConfig) -> Terminal {
    let mut terminal = Terminal::new(
        config.columns,
        config.rows,
        config.scrollback_lines,
        config.scrollback_bytes,
    );
    terminal.enable_reply_streams();
    terminal.set_grapheme_width_default(config.grapheme_width_unicode);
    // The name moves once; neither the Session config nor VT retains a duplicate string.
    terminal.term_name = std::mem::take(&mut config.term_name);
    terminal
}

/// Per-surface query UI ownership; the underlying search model is shared.
mod search_ui;
