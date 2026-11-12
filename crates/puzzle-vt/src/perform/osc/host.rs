//! Common xterm OSC 22 pointer-shape request (§6.11, D-58).
//! Reference: <https://sw.kovidgoyal.net/kitty/pointer-shapes/#legacy-xterm-compatibility>.
//! The protocol originated with xterm; a finite CSS-name allowlist prevents injection.
use crate::terminal::{Terminal, TerminalEvent};

/// Keeps OSC 22 inside a finite CSS cursor-name allowlist.
pub(super) fn pointer(term: &mut Terminal, params: &[&[u8]]) {
    // A UTF-8 shape name is required before comparing with CSS keywords.
    let Some(value) = params.get(1).and_then(|raw| std::str::from_utf8(raw).ok()) else {
        return;
    };
    // These names are the existing supported CSS subset, not arbitrary host strings.
    // Text, pointing and precision cursors cover the terminal's usual interactions.
    // Drag cursors distinguish an available handle from an active grab.
    // Progress/wait expose a busy state without fetching any external cursor resource.
    // Resize names encode axes only; the application never gets window-manager access.
    // Queries and shape stacks remain outside the shipped OSC 22 subset.
    // Keeping exact string comparison preserves current invalid-name rejection.
    /// Fixed accepted cursor names; never pass an unchecked OSC payload to a host.
    const ALLOWED: &[&str] = &[
        "default",
        "text",
        "pointer",
        "crosshair",
        "move",
        "grab",
        "grabbing",
        "not-allowed",
        "wait",
        "progress",
        "help",
        "cell",
        "copy",
        "alias",
        "ew-resize",
        "ns-resize",
        "nesw-resize",
        "nwse-resize",
    ];
    if ALLOWED.contains(&value) {
        // The event is a request; a platform adapter applies it later.
        term.pointer_shape = Some(value.to_owned());
        term.events
            .push(TerminalEvent::PointerShape(value.to_owned()));
    }
}
