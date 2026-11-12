//! Typed shortcut actions and their discoverable metadata (§14.9).
//!
//! Deferred actions retain a stable identity so configs can be shared with
//! later releases. Availability is explicit rather than a silent no-op.
use crate::settings::ThemeMode;

/// Closed action vocabulary; payloads are validated by the binding parser.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Copy the selection in the requested text, VT or HTML representation.
    Copy(String),
    /// Request a user-gesture clipboard paste through paste policy.
    Paste,
    /// Paste the desktop PRIMARY selection.
    PasteSelection,
    /// Select every available terminal line.
    SelectAll,
    /// Select the OSC 133 command output under the pointer.
    SelectCommandOutput,
    /// Copy the most recent command output.
    CopyCommandOutput,
    /// Send decoded Unicode text directly to the child.
    SendText(String),
    /// Send ESC [ followed by the supplied control sequence.
    Csi(String),
    /// Send ESC followed by the supplied sequence.
    Esc(String),
    /// Clear history and visible output while preserving the prompt row.
    ClearScreen,
    /// Remove saved history without changing visible cells.
    ClearScrollback,
    /// Reset the terminal as RIS does.
    Reset,
    /// Move the viewport by a signed number of rows.
    ScrollLines(i32),
    /// Move the viewport by a signed fractional page count.
    ScrollPages(f32),
    /// Show the oldest available history.
    ScrollTop,
    /// Resume following the live screen.
    ScrollBottom,
    /// Navigate between OSC 133 prompt marks.
    JumpToPrompt(i32),
    /// Open the host search interface.
    Search,
    /// Select the next search result.
    SearchNext,
    /// Select the previous search result.
    SearchPrev,
    /// Adjust the logical font size by a signed step count.
    Zoom(i32),
    /// Restore the configured logical font size.
    ZoomReset,
    /// Choose light, dark or automatic appearance.
    SetAppearance(ThemeMode),
    /// Switch to the opposite resolved appearance.
    ToggleAppearance,
    /// Activate a named settings override profile.
    Profile(String),
    /// Reload the settings file while retaining last-good values.
    ReloadSettings,
    /// Open the native configuration file in an editor.
    OpenSettings,
    /// Display the searchable shortcut overlay.
    ShowKeybindings,
    /// Toggle the accessible text mirror.
    ToggleScreenReaderMode,
    /// Toggle the native window fullscreen state.
    ToggleFullscreen,
    /// Ask the desktop host for another terminal window.
    NewWindow,
    /// Close this desktop terminal window.
    CloseWindow,
    /// Invoke a registered plugin or embedding-page action.
    Ext(String),
    /// Consume this shortcut without writing input.
    Ignore,
    /// Remove this trigger and restore ordinary key encoding.
    Unbind,
}

/// One public catalogue row used by CLI and generated settings docs.
#[derive(Clone, Copy, Debug)]
pub struct ActionInfo {
    /// Portable camelCase name accepted by the parser.
    pub name: &'static str,
    /// Human meaning, independent of platform implementation.
    pub description: &'static str,
    /// Validated argument category; none rejects a trailing argument.
    pub argument: &'static str,
    /// Earliest feature milestone, or now for implemented actions.
    pub available: &'static str,
}

/// Builds catalogue rows without repeating their field declarations.
macro_rules! info {
    ($name:literal, $doc:literal, $arg:literal, $when:literal) => {
        ActionInfo {
            name: $name,
            description: $doc,
            argument: $arg,
            available: $when,
        }
    };
}

/// All names, including future actions, remain visible to introspection.
#[rustfmt::skip]
pub const ACTIONS: &[ActionInfo] = &[
    // Copy the selection in the requested text, VT or HTML representation.
    info!("copy", "Copy the selection in the requested text, VT or HTML representation.", "text", "now"),
    // Request a user-gesture clipboard paste through paste policy.
    info!("paste", "Request a user-gesture clipboard paste through paste policy.", "none", "now"),
    // Paste the desktop PRIMARY selection.
    info!("pasteSelection", "Paste the desktop PRIMARY selection.", "none", "M13"),
    // Select every available terminal line.
    info!("selectAll", "Select every available terminal line.", "none", "now"),
    // Select the OSC 133 command output under the pointer.
    info!("selectCommandOutput", "Select the OSC 133 command output under the pointer.", "none", "now"),
    // Copy the most recent command output.
    info!("copyCommandOutput", "Copy the most recent command output.", "none", "now"),
    // Send decoded Unicode text directly to the child.
    info!("sendText", "Send decoded Unicode text directly to the child.", "text", "now"),
    // Send ESC [ followed by the supplied control sequence.
    info!("csi", "Send ESC [ followed by the supplied control sequence.", "text", "now"),
    // Send ESC followed by the supplied sequence.
    info!("esc", "Send ESC followed by the supplied sequence.", "text", "now"),
    // Clear history and visible output while preserving the prompt row.
    info!("clearScreen", "Clear history and visible output while preserving the prompt row.", "none", "now"),
    // Remove saved history without changing visible cells.
    info!("clearScrollback", "Remove saved history without changing visible cells.", "none", "now"),
    // Reset the terminal as RIS does.
    info!("reset", "Reset the terminal as RIS does.", "none", "now"),
    // Move the viewport by a signed number of rows.
    info!("scrollLines", "Move the viewport by a signed number of rows.", "integer", "now"),
    // Move the viewport by a signed fractional page count.
    info!("scrollPages", "Move the viewport by a signed fractional page count.", "number", "now"),
    // Show the oldest available history.
    info!("scrollTop", "Show the oldest available history.", "none", "now"),
    // Resume following the live screen.
    info!("scrollBottom", "Resume following the live screen.", "none", "now"),
    // Navigate between OSC 133 prompt marks.
    info!("jumpToPrompt", "Navigate between OSC 133 prompt marks.", "integer", "now"),
    // Open the host search interface.
    info!("search", "Open the host search interface.", "none", "now"),
    // Select the next search result.
    info!("searchNext", "Select the next search result.", "none", "now"),
    // Select the previous search result.
    info!("searchPrev", "Select the previous search result.", "none", "now"),
    // Adjust the logical font size by a signed step count.
    info!("zoom", "Adjust the logical font size by a signed step count.", "integer", "now"),
    // Restore the configured logical font size.
    info!("zoomReset", "Restore the configured logical font size.", "none", "now"),
    // Choose light, dark or automatic appearance.
    info!("setAppearance", "Choose light, dark or automatic appearance.", "appearance", "now"),
    // Switch to the opposite resolved appearance.
    info!("toggleAppearance", "Switch to the opposite resolved appearance.", "none", "now"),
    // Activate a named settings override profile.
    info!("profile", "Activate a named settings override profile.", "text", "now"),
    // Reload the settings file while retaining last-good values.
    info!("reloadSettings", "Reload the settings file while retaining last-good values.", "none", "now"),
    // Open the native configuration file in an editor.
    info!("openSettings", "Open the native configuration file in an editor.", "none", "M12"),
    // Display the searchable shortcut overlay.
    info!("showKeybindings", "Display the searchable shortcut overlay.", "none", "now"),
    // Toggle the accessible text mirror.
    info!("toggleScreenReaderMode", "Toggle the accessible text mirror.", "none", "M14"),
    // Toggle the native window fullscreen state.
    info!("toggleFullscreen", "Toggle the native window fullscreen state.", "none", "M12"),
    // Ask the desktop host for another terminal window.
    info!("newWindow", "Ask the desktop host for another terminal window.", "none", "M12"),
    // Close this desktop terminal window.
    info!("closeWindow", "Close this desktop terminal window.", "none", "M12"),
    // Invoke a registered plugin or embedding-page action.
    info!("ext", "Invoke a registered plugin or embedding-page action.", "text", "registered action"),
    // Consume this shortcut without writing input.
    info!("ignore", "Consume this shortcut without writing input.", "none", "now"),
    // Remove this trigger and restore ordinary key encoding.
    info!("unbind", "Remove this trigger and restore ordinary key encoding.", "none", "now"),
 ];

/// Relates the typed payload to the one authoritative catalogue entry.
/// Associates typed payloads with their catalogue metadata.
impl Action {
    /// Returns metadata without reparsing arguments on the input path.
    pub fn info(&self) -> &'static ActionInfo {
        let index = match self {
            Self::Copy(_) => 0,
            Self::Paste => 1,
            Self::PasteSelection => 2,
            Self::SelectAll => 3,
            Self::SelectCommandOutput => 4,
            Self::CopyCommandOutput => 5,
            Self::SendText(_) => 6,
            Self::Csi(_) => 7,
            Self::Esc(_) => 8,
            Self::ClearScreen => 9,
            Self::ClearScrollback => 10,
            Self::Reset => 11,
            Self::ScrollLines(_) => 12,
            Self::ScrollPages(_) => 13,
            Self::ScrollTop => 14,
            Self::ScrollBottom => 15,
            Self::JumpToPrompt(_) => 16,
            Self::Search => 17,
            Self::SearchNext => 18,
            Self::SearchPrev => 19,
            Self::Zoom(_) => 20,
            Self::ZoomReset => 21,
            Self::SetAppearance(_) => 22,
            Self::ToggleAppearance => 23,
            Self::Profile(_) => 24,
            Self::ReloadSettings => 25,
            Self::OpenSettings => 26,
            Self::ShowKeybindings => 27,
            Self::ToggleScreenReaderMode => 28,
            Self::ToggleFullscreen => 29,
            Self::NewWindow => 30,
            Self::CloseWindow => 31,
            Self::Ext(_) => 32,
            Self::Ignore => 33,
            Self::Unbind => 34,
        };
        // The exhaustive match and catalogue are emitted together here;
        // tests pin every parser name to its corresponding metadata.
        &ACTIONS[index]
    }

    /// Current feature availability, before contextual checks such as copy.
    pub fn performable(&self) -> bool {
        self.info().available == "now" && !matches!(self, Self::Unbind)
    }
}
