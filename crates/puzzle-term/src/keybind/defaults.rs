//! Platform defaults as portable binding data (§6.8, §14.9).
//!
//! Physical M6 entries retain their layout-independent identities. Zoom
//! also has logical plus/equal aliases because layouts produce + differently.
//!
//! Paste gestures differ by OS because shell Control chords are meaningful.
//! Windows keeps its familiar Ctrl+V as well as the shared shifted gesture;
//! Linux leaves Ctrl+V to the shell and offers Shift+Insert instead.
//! Copy is performable, so Windows Ctrl+C remains interrupt without selection.
//! On macOS the natural-editing shortcuts send readline/ZLE control bytes.
//! They are independent of negotiated kitty modes because Cmd+Left is a host
//! editing action, not an application request to report a Super arrow key.
//! Option arrows produce no layout text, so these bindings also work when
//! Option is configured for special characters instead of Meta (§6.8).
//! Zoom uses logical punctuation so shifted US plus and unshifted German plus
//! both work. The separate equal alias preserves the customary unshifted key.
//! Scrolling, search and help defaults are listed now but remain unavailable
//! until their owning milestones, rather than hiding portable configuration.
//! Reload uses physical Comma with Shift, which retains its identity even
//! when the layout produces a semicolon or angle bracket for that chord.
use super::Platform;

/// M6 macOS clipboard, natural editing and settings reload entries.
pub const MAC: &[&str] = &[
    // Clipboard copy remains performable so an absent selection falls through.
    "super+KeyV=paste",
    "performable:super+KeyC=copy",
    // These exact byte actions are readline/ZLE operations, not kitty keys.
    "super+ArrowLeft=sendText:\\x01",
    "super+ArrowRight=sendText:\\x05",
    "super+Backspace=sendText:\\x15",
    "alt+ArrowLeft=esc:b",
    "alt+ArrowRight=esc:f",
    // Comma is physical: Shift may change its produced layout character.
    "super+shift+Comma=reloadSettings",
];
/// Windows keeps Ctrl+C as interrupt until a selection can be copied.
pub const WINDOWS: &[&str] = &[
    // Three standard paste gestures, including Windows' unshifted Ctrl+V.
    "ctrl+KeyV=paste",
    "ctrl+shift+KeyV=paste",
    "shift+Insert=paste",
    // Both copy chords must preserve encoder fallback without selection.
    "performable:ctrl+KeyC=copy",
    "performable:ctrl+shift+KeyC=copy",
    "ctrl+shift+Comma=reloadSettings",
];
/// Linux conventions avoid intercepting readline's ordinary Ctrl chords.
pub const LINUX: &[&str] = &[
    // PRIMARY middle-click is a separate M13 action, not clipboard paste.
    "ctrl+shift+KeyV=paste",
    "shift+Insert=paste",
    "performable:ctrl+shift+KeyC=copy",
    "ctrl+shift+Comma=reloadSettings",
];

/// Full portable defaults; deferred features are visible but unperformable.
pub fn lines(platform: Platform) -> Vec<String> {
    let initial = match platform {
        Platform::Mac => MAC,
        Platform::Windows => WINDOWS,
        Platform::Linux => LINUX,
    };
    let mut lines = initial
        .iter()
        .map(|line| (*line).into())
        .collect::<Vec<String>>();
    // Primary resolves in the parser; no host-specific grammar is introduced.
    for entry in [
        "primary+plus=zoom:1",
        "primary+equal=zoom:1",
        "primary+minus=zoom:-1",
        "primary+0=zoomReset",
        "shift+pageup=scrollPages:-1",
        "shift+pagedown=scrollPages:1",
        "primary+shift+slash=showKeybindings",
    ] {
        lines.push(entry.into());
    }
    let extra = if platform == Platform::Mac {
        [
            "super+home=scrollTop",
            "super+end=scrollBottom",
            "super+arrowup=jumpToPrompt:-1",
            "super+arrowdown=jumpToPrompt:1",
            "super+f=search",
            "super+a=selectAll",
            "super+k=clearScreen",
        ]
    } else {
        [
            "shift+home=scrollTop",
            "shift+end=scrollBottom",
            "ctrl+shift+arrowup=jumpToPrompt:-1",
            "ctrl+shift+arrowdown=jumpToPrompt:1",
            "ctrl+shift+f=search",
            "ctrl+shift+a=selectAll",
            "ctrl+shift+k=clearScreen",
        ]
    };
    lines.extend(extra.iter().map(|line| (*line).into()));
    lines
}
