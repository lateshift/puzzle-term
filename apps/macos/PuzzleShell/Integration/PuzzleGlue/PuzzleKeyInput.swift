// AppKit key facts for puzzle.h; terminal encoding and Option policy remain on Puzzle's Session owner.
import AppKit

/// An owned snapshot allows Cocoa interpretation to finish before a single key message is queued.
/// Ordinary text travels with its key for kitty/bindings; IME commits travel separately as text.
struct PuzzleKeyInput {
    /// C ABI logical kind: character, named, dead or unidentified.
    let kind: UInt32
    /// Layout-produced identity, never inferred from the physical US key position.
    let logical: String
    /// Active-layout character with Shift/Option removed, as required by puzzle.h.
    let base: String
    /// W3C physical code preserves binding and keypad identity across layouts.
    let physical: String
    /// Printable text committed by Cocoa for this key, or empty for controls/releases.
    let text: String
    /// Standard/left/right/numpad location in the C ABI numbering.
    let location: UInt32
    /// Shift/Alt/Control/Super plus per-side Option bookkeeping.
    let modifiers: UInt32
    /// A release never inserts text but may generate a kitty release report.
    let pressed: Bool
    /// Repeats remain distinguishable under kitty event reporting.
    let repeated: Bool

    /// A native menu selection reaches the existing core binding seam without creating synthetic NSEvents.
    init(menuBinding logical: String, physical: String, pressed: Bool) {
        kind = 0
        self.logical = logical
        base = logical
        self.physical = physical
        text = ""
        location = 0
        modifiers = 8
        self.pressed = pressed
        repeated = false
    }

    /// Snapshot only key events; flagsChanged must not read AppKit's key-only properties.
    init(event: NSEvent, committed: String?) {
        let code = event.keyCode
        physical = Self.codes[code] ?? "Unidentified"
        location = physical.hasPrefix("Numpad") ? 3 : ([54, 60, 61, 62].contains(code) ? 2 : ([55, 56, 58, 59].contains(code) ? 1 : 0))
        let named = Self.names[code]
        base = event.characters(byApplyingModifiers: []) ?? event.charactersIgnoringModifiers ?? ""
        // Cocoa's printable commit is authoritative for layout/dead-key results; control chords
        // keep their layout identity because the Session encoder owns Ctrl and terminal modes.
        logical = named ?? (committed.flatMap { Self.printable($0) ? $0 : nil } ?? event.charactersIgnoringModifiers ?? "")
        let dead = named == nil && committed == nil && event.type == .keyDown &&
            event.modifierFlags.intersection([.control, .command]).isEmpty
        kind = named != nil ? 1 : (dead ? 2 : (logical.isEmpty ? 3 : 0))
        text = committed.flatMap { Self.printable($0) ? $0 : nil } ?? ""
        modifiers = Self.modifiers(event.modifierFlags)
        pressed = event.type == .keyDown
        repeated = event.isARepeat
    }

    /// A release keeps its press identity, including dead keys and Option-produced characters.
    /// Only the current modifier state changes; reconstructing the kind would invent a kitty
    /// character release for a dead key whose press was deliberately ignored by the encoder.
    init(releasing press: PuzzleKeyInput, event: NSEvent) {
        kind = press.kind
        logical = press.logical
        base = press.base
        physical = press.physical
        text = press.text // Puzzle uses this to preserve Option's text-modifier policy on release.
        location = press.location
        modifiers = Self.modifiers(event.modifierFlags)
        pressed = false
        repeated = false
    }

    /// C0/DEL are key encoder artifacts and must never be inserted a second time as text.
    static func printable(_ text: String) -> Bool {
        !text.isEmpty && !text.unicodeScalars.contains { $0.value < 32 || $0.value == 127 }
    }

    /// Native side bits distinguish right Option typing from configured left-Option Alt shortcuts.
    static func modifiers(_ flags: NSEvent.ModifierFlags) -> UInt32 {
        var bits: UInt32 = 0
        for (flag, bit): (NSEvent.ModifierFlags, UInt32) in [(.shift, 1), (.option, 2), (.control, 4), (.command, 8)] {
            if flags.contains(flag) { bits |= bit }
        }
        // NX_DEVICELALTKEYMASK/NX_DEVICERALTKEYMASK are the public device-dependent bits.
        if flags.rawValue & 0x20 != 0 { bits |= 16 }
        if flags.rawValue & 0x40 != 0 { bits |= 32 }
        if bits & 2 != 0 && bits & 48 == 0 { bits |= 16 } // Synthetic/aggregate-only events lack a side.
        return bits
    }

    /// Hardware controls retain protocol meanings even when Cocoa supplies CR or private-use text.
    private static let names: [UInt16: String] = [
        36: "Enter", 48: "Tab", 51: "Backspace", 53: "Escape", 76: "Enter",
        114: "Insert", 115: "Home", 116: "PageUp", 117: "Delete", 119: "End", 121: "PageDown",
        123: "ArrowLeft", 124: "ArrowRight", 125: "ArrowDown", 126: "ArrowUp",
        122: "F1", 120: "F2", 99: "F3", 118: "F4", 96: "F5", 97: "F6", 98: "F7",
        100: "F8", 101: "F9", 109: "F10", 103: "F11", 111: "F12", 105: "F13", 107: "F14",
        113: "F15", 106: "F16", 64: "F17", 79: "F18", 80: "F19", 90: "F20"
    ]

    /// Apple's virtual ANSI/ISO/JIS positions mapped to W3C codes, independent of typed text.
    private static let codes: [UInt16: String] = [
        0: "KeyA", 1: "KeyS", 2: "KeyD", 3: "KeyF", 4: "KeyH", 5: "KeyG", 6: "KeyZ",
        7: "KeyX", 8: "KeyC", 9: "KeyV", 10: "IntlBackslash", 11: "KeyB", 12: "KeyQ",
        13: "KeyW", 14: "KeyE", 15: "KeyR", 16: "KeyY", 17: "KeyT", 18: "Digit1",
        19: "Digit2", 20: "Digit3", 21: "Digit4", 22: "Digit6", 23: "Digit5", 24: "Equal",
        25: "Digit9", 26: "Digit7", 27: "Minus", 28: "Digit8", 29: "Digit0", 30: "BracketRight",
        31: "KeyO", 32: "KeyU", 33: "BracketLeft", 34: "KeyI", 35: "KeyP", 36: "Enter",
        37: "KeyL", 38: "KeyJ", 39: "Quote", 40: "KeyK", 41: "Semicolon", 42: "Backslash",
        43: "Comma", 44: "Slash", 45: "KeyN", 46: "KeyM", 47: "Period", 48: "Tab", 49: "Space",
        50: "Backquote", 51: "Backspace", 53: "Escape", 54: "MetaRight", 55: "MetaLeft",
        56: "ShiftLeft", 57: "CapsLock", 58: "AltLeft", 59: "ControlLeft", 60: "ShiftRight",
        61: "AltRight", 62: "ControlRight", 63: "Fn", 65: "NumpadDecimal", 67: "NumpadMultiply",
        69: "NumpadAdd", 71: "NumLock", 75: "NumpadDivide", 76: "NumpadEnter", 78: "NumpadSubtract",
        81: "NumpadEqual", 82: "Numpad0", 83: "Numpad1", 84: "Numpad2", 85: "Numpad3",
        86: "Numpad4", 87: "Numpad5", 88: "Numpad6", 89: "Numpad7", 91: "Numpad8", 92: "Numpad9",
        93: "IntlYen", 94: "IntlRo", 95: "NumpadComma", 102: "Lang2", 104: "Lang1",
        114: "Insert", 115: "Home", 116: "PageUp", 117: "Delete", 119: "End", 121: "PageDown",
        123: "ArrowLeft", 124: "ArrowRight", 125: "ArrowDown", 126: "ArrowUp",
        122: "F1", 120: "F2", 99: "F3", 118: "F4", 96: "F5", 97: "F6", 98: "F7",
        100: "F8", 101: "F9", 109: "F10", 103: "F11", 111: "F12", 105: "F13", 107: "F14",
        113: "F15", 106: "F16", 64: "F17", 79: "F18", 80: "F19", 90: "F20"
    ]
}
