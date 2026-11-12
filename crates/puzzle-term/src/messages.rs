//! Typed terminal messages above the generic bus (design §5.1, §5.7).
//!
//! Only payloads available in M1 become variants. Future protocol names stay
//! reserved in the catalogue, so later milestones add their real Rust types
//! without changing a public identity or sending an untyped built-in value.
/// Typed counters exist independently of the debug overlay.
mod frame_stats;
/// Completed construction data for opt-in host subscribers.
pub use frame_stats::FrameStats;
/// Counter wire validation rejects malformed external recordings.
mod stats_wire;

/// Stable wire names and reserved milestone ownership.
pub mod catalogue;
/// Bounded context values cross the public wire only on observation.
mod context_wire;
/// Typed clipboard source and serializer format at the public boundary.
mod copy_wire;
/// KeyEvent conversion keeps the Rust key identity separate from plain data.
mod key_wire;
/// Pointer events retain device coordinates and click counts on the wire.
mod mouse_wire;
/// Signed pointer samples retain outside distance at the wire boundary.
mod pointer_wire;
/// Composition offsets are validated separately from terminal coordinates.
mod preedit_wire;
/// Literal search requests use an exact discriminated map.
mod search_wire;
/// Selection requests and nullable range facts have checked cell endpoints.
mod selection_wire;
/// Validated maps for multi-field terminal facts and host requests.
mod terminal_wire;
/// Finite distances and discriminated absolute navigation maps.
mod viewport_wire;
/// Hardware wheel units and pointer coordinates survive recording.
mod wheel_wire;
/// Fallible conversion at the public protocol boundary.
mod wire;

/// Clipboard payloads share one policy kind regardless of MIME representation.
mod clipboard;
use crate::theme::Appearance;
pub use clipboard::ClipboardContent;
/// Plain payload tree exposed only to hosts crossing the wire boundary.
pub use puzzle_bus::value::Value;
/// Stable command conversion available to native recording hosts.
pub use puzzle_bus::wire::ToWire;
use puzzle_bus::{
    kinds::{KindDef, Visibility},
    message::Message,
    wire::Extension,
};
use puzzle_vt::input::KeyEvent;

/// Provisional IME text; its cursor is a UTF-8 byte offset, not a grid column.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PreeditState {
    /// Not sent to the application until the separate committed Text command.
    pub text: String,
    /// Candidate caret when the platform provides it; None means unknown.
    pub cursor: Option<usize>,
}

/// Final child status, reported only after every PTY output byte is parsed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExitInfo {
    /// Numeric status when the platform reported an ordinary exit.
    pub code: Option<i32>,
    /// Platform signal name when termination was caused by a signal.
    pub signal: Option<String>,
    /// Monotonic time from process spawn to the drained exit event.
    pub runtime_ms: u64,
}

/// A typed request that policy hooks may rewrite or veto.
#[non_exhaustive]
#[derive(Debug)]
pub enum Command {
    /// One physical key with modifiers, encoded under current VT modes.
    Key(KeyEvent),
    /// Relative local movement in rows; negative values move into history.
    Scroll(f64),
    /// Absolute local navigation without changing the application cursor.
    ScrollTo(crate::viewport::ScrollTarget),
    /// Replace, expand or clear the local tracked selection.
    Select(crate::selection::SelectionCommand),
    /// Start, clear or request the host literal search interface.
    Search(crate::search::SearchCommand),
    /// Copy a selection or command output through ClipboardWrite policy.
    Copy(crate::selection::CopyTarget),
    /// Hardware wheel delta before tracking/alternate/local routing.
    Wheel {
        /// OS pixels or notched rows; positive moves toward live output.
        delta: crate::viewport::ScrollInput,
        /// Hit and modifiers used if routing chooses application reports.
        pointer: puzzle_vt::input::MouseEvent,
    },
    /// Platform pointer report, encoded under the current tracking modes.
    Mouse(puzzle_vt::input::MouseEvent),
    /// Local selection retains signed coordinates independently of protocol bounds.
    Pointer(crate::pointer::PointerInput),
    /// A validated shortcut action with optional shared-store host routing.
    BindingAction {
        /// Concrete operation with a typed argument.
        action: crate::keybind::Action,
        /// Host fans out to sessions sharing the settings store.
        all: bool,
    },
    /// Text committed by an input method, distinct from physical keys.
    Text(String),
    /// Composition state is retained for the future M9 preedit overlay.
    Preedit(PreeditState),
    /// Input focus controls DECSET 1004 reports and hollow cursor geometry.
    Focus(bool),
    /// Pasted text follows sanitization and bracketed-paste policy.
    Paste(String),
    /// Policy-approved plain text or rich HTML with its plain alternative.
    ClipboardWrite(ClipboardContent),
    /// Internal policy-visible request; results never become a user paste.
    ClipboardRead,
    /// OSC 9/777 requests a bounded title and body notification.
    /// Internal typed OSC 99 request; follows the same policy lane as legacy notifications.
    DesktopNotification(crate::vt::notification::Request),
    /// Internal drop registration/feedback enters host policy with Terminal origin.
    Drop(crate::vt::dnd::Effect),
    /// Legacy title/body request, sharing OSC 99 admission policy.
    Notify {
        /// Bounded heading supplied by OSC 9 or 777.
        title: String,
        /// Bounded body text after VT sanitization.
        body: String,
    },
    /// OSC 22 asks the host to change its pointer affordance.
    SetPointerShape(String),
    /// Request to play an audible bell after host policy allows it.
    RingBell,
    /// Policy-approved hyperlink opening; never application-bound input.
    OpenLink(String),
    /// Platform reports a fully drained child process exit.
    ChildExited(ExitInfo),
    /// Host-reported light or dark appearance, independent of theme choice.
    SetSystemAppearance(Appearance),
    /// Platform fallback work is ready; retry provisional glyphs at the next refresh.
    GlyphRasterized,
    /// Typed built-in pointer operation; externally dispatched input cannot forge it.
    OverlayAction(puzzle_overlay::models::Action),
    /// Registered plugin request; erasure exists only at this seam.
    Ext(Extension),
}

/// A fact already produced by the terminal or a plugin.
#[non_exhaustive]
#[derive(Debug)]
pub enum Event {
    /// Successfully submitted frame construction, measured only while observed.
    FrameRendered(FrameStats),
    /// Visible title reported by the application.
    TitleChanged(String),
    /// Physical pixels above the live tail after accepted navigation.
    ViewportScrolled(f64),
    /// Expanded inclusive range changed or was invalidated by output.
    SelectionChanged(Option<crate::selection::SelectionRange>),
    /// Lines arrived while inspecting history, or following cleared the pill.
    NewOutputChanged(bool),
    /// The page should focus its own search interface.
    SearchRequested,
    /// OSC 0/1 changed the icon label independently of the title.
    IconNameChanged(String),
    /// OSC 7 announced a current working-directory URI.
    CwdChanged(String),
    /// Final active root-to-leaf context path, coalesced once per owner parser drain.
    ContextsChanged(Vec<crate::vt::context::Context>),
    /// A BEL control was received, regardless of audible policy.
    Bell,
    /// OSC 133 A marked the beginning of the prompt.
    PromptStarted,
    /// OSC 133 B marked the beginning of editable input.
    InputStarted,
    /// OSC 133 C began output; optional command label is at most 1 KiB UTF-8.
    CommandStarted(Option<String>),
    /// OSC 133 D ended a command with an optional exit status.
    CommandFinished(Option<i32>),
    /// VT resize changed the visible grid dimensions.
    GridResized {
        /// New terminal column count after clamping and reflow.
        columns: usize,
        /// New visible row count after clamping.
        rows: usize,
    },
    /// OSC 9;4 reported task progress to the host.
    Progress {
        /// ConEmu progress state code from OSC 9;4.
        state: u8,
        /// Completion percentage clamped to 0–100.
        percent: u8,
    },
    /// The child ended after its final visible output was parsed.
    SessionExited(ExitInfo),
    /// A key in an exited hold screen asks the host to close it.
    SessionCloseRequested,
    /// A resolved theme pair member became active.
    ThemeChanged {
        /// Stable built-in or custom selector.
        name: String,
        /// Effective appearance after explicit mode selection.
        appearance: Appearance,
    },
    /// Registered plugin fact with a checked wire identity.
    Ext(Extension),
}

/// The command lane supplies stable mask positions without coupling the bus.
impl Message for Command {
    /// The final command bit is reserved for all plugin requests.
    const EXT_INDEX: usize = 26;

    /// Names reserved today retain their mask positions as variants arrive.
    fn catalogue() -> &'static [KindDef] {
        catalogue::COMMANDS
    }

    /// Finds the fixed mask bit for the concrete request.
    fn kind_index(&self) -> usize {
        // These positions include reserved rows so activation in M7 or M8
        // cannot silently reassign an existing policy mask bit.
        match self {
            Self::Copy(_) => 4,
            Self::Key(_) => 0,
            Self::Scroll(_) => 6,
            Self::Select(_) => 11,
            Self::Search(_) => 12,
            Self::ScrollTo(_) => 7,
            Self::Mouse(_) => 5,
            Self::Wheel { .. } => 29,
            Self::Pointer(_) => 30,
            Self::BindingAction { .. } => 28,
            Self::Text(_) => 1,
            Self::Preedit(_) => 2,
            Self::Focus(_) => 9,
            Self::Paste(_) => 3,
            Self::ClipboardWrite(_) => 18,
            Self::ClipboardRead => 32,
            Self::DesktopNotification(_) => 33,
            Self::Drop(_) => 34,
            Self::Notify { .. } => 19,
            Self::SetPointerShape(_) => 20,
            Self::GlyphRasterized => 25,
            Self::OverlayAction(_) => 31,
            Self::RingBell => 21,
            Self::OpenLink(_) => 22,
            Self::ChildExited(_) => 27,
            Self::SetSystemAppearance(_) => 17,
            Self::Ext(_) => Self::EXT_INDEX,
        }
    }

    /// Only plugin commands carry an erased extension.
    fn extension(&self) -> Option<&Extension> {
        // Built-in requests never enter the plugin's erased channel.
        if let Self::Ext(extension) = self {
            Some(extension)
        } else {
            None
        }
    }

    /// Plugin policy adapters may rewrite their own concrete payload.
    fn extension_mut(&mut self) -> Option<&mut Extension> {
        if let Self::Ext(extension) = self {
            Some(extension)
        } else {
            None
        }
    }

    /// Restores the command lane after plugin validation.
    fn from_extension(extension: Extension) -> Self {
        Self::Ext(extension)
    }
}

/// The event lane shares bus machinery but owns different catalogue bits.
impl Message for Event {
    /// The final event bit is reserved for every plugin fact.
    const EXT_INDEX: usize = 20;

    /// Public and internal facts remain auditable in one table.
    fn catalogue() -> &'static [KindDef] {
        catalogue::EVENTS
    }

    /// Picks the observer mask bit for a concrete fact.
    fn kind_index(&self) -> usize {
        // Facts use a separate mask lane from requests; reserved positions
        // remain occupied even while no matching variant exists.
        match self {
            Self::FrameRendered(_) => 16,
            Self::TitleChanged(_) => 0,
            Self::ViewportScrolled(_) => 10,
            Self::SelectionChanged(_) => 11,
            Self::NewOutputChanged(_) => 25,
            Self::SearchRequested => 26,
            Self::IconNameChanged(_) => 1,
            Self::CwdChanged(_) => 2,
            Self::ContextsChanged(_) => 27,
            Self::Bell => 3,
            Self::PromptStarted => 4,
            Self::CommandStarted(_) => 5,
            Self::CommandFinished(_) => 6,
            Self::GridResized { .. } => 9,
            Self::InputStarted => 21,
            Self::Progress { .. } => 22,
            Self::SessionExited(_) => 23,
            Self::SessionCloseRequested => 24,
            Self::ThemeChanged { .. } => 14,
            Self::Ext(_) => Self::EXT_INDEX,
        }
    }

    /// Non-plugin facts never expose an erased payload.
    fn extension(&self) -> Option<&Extension> {
        if let Self::Ext(extension) = self {
            Some(extension)
        } else {
            None
        }
    }

    /// Observers cannot rewrite facts, but the trait supports both lanes.
    fn extension_mut(&mut self) -> Option<&mut Extension> {
        if let Self::Ext(extension) = self {
            Some(extension)
        } else {
            None
        }
    }

    /// Restores the event lane after plugin validation.
    fn from_extension(extension: Extension) -> Self {
        Self::Ext(extension)
    }
}

/// Domain visibility is determined by the corresponding catalogue row.
impl Command {
    /// Reveals whether this concrete request can cross the public wire.
    #[must_use]
    pub fn visibility(&self) -> Visibility {
        <Self as Message>::visibility(self)
    }
}

/// Facts follow the same public boundary as requests.
impl Event {
    /// Reveals whether external subscribers may receive this fact.
    #[must_use]
    pub fn visibility(&self) -> Visibility {
        <Self as Message>::visibility(self)
    }
}

#[cfg(test)]
/// Protocol tests include every active kind and hostile external data.
mod tests;

/// Binary alternatives retain their exact bytes at external dispatch boundaries.
mod clipboard_wire;

/// Bounded shell command facts use one public map in every host.
mod shell_wire;
