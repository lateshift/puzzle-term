import AppKit

/// One native menu shortcut, keeping its logical key and required modifiers together.
/// AppKit resolves menu equivalents before forwarding a matching key event to the terminal.
struct ShellShortcut {
    /// Printable characters or AppKit function-key Unicode scalars, rather than hardware key codes.
    let key: String
    /// Explicit modifiers make each binding independent of NSMenuItem's default Command modifier.
    let modifiers: NSEvent.ModifierFlags
}

/// Central configuration for shell hotkeys; edit these constants to change the native menu bindings.
/// Keeping bindings out of menu construction avoids duplicating shortcut policy in terminal input code.
enum ShellShortcuts {
    /// Create an independent native window.
    static let newWindow = ShellShortcut(key: "n", modifiers: .command)
    /// Add a terminal tab to the focused window.
    static let newTab = ShellShortcut(key: "t", modifiers: .command)
    /// Close the selected session; its final tab also closes the window.
    static let closeTab = ShellShortcut(key: "w", modifiers: .command)
    /// Close every session in the focused window.
    static let closeWindow = ShellShortcut(key: "w", modifiers: [.command, .shift])
    /// Minimize the focused native window.
    static let minimize = ShellShortcut(key: "m", modifiers: .command)
    /// Hide PuzzleShell using the native application command.
    static let hide = ShellShortcut(key: "h", modifiers: .command)
    /// Hide other applications using the standard macOS binding.
    static let hideOthers = ShellShortcut(key: "h", modifiers: [.command, .option])
    /// Quit through the normal session teardown path.
    static let quit = ShellShortcut(key: "q", modifiers: .command)

    /// Option-Command-Right advances to the next tab, wrapping at the end.
    static let nextTab = ShellShortcut(key: functionKey(NSRightArrowFunctionKey), modifiers: [.option, .command])
    /// Option-Command-Left moves to the previous tab, wrapping at the beginning.
    static let previousTab = ShellShortcut(key: functionKey(NSLeftArrowFunctionKey), modifiers: [.option, .command])
    /// Preserve the earlier navigation binding as an alias without duplicating the visible menu command.
    static let nextTabAlternate = ShellShortcut(key: "]", modifiers: [.command, .shift])
    /// Preserve the earlier backward-navigation binding alongside the new arrow shortcut.
    static let previousTabAlternate = ShellShortcut(key: "[", modifiers: [.command, .shift])

    /// Native numbered navigation: 1–8 address visible slots; 9 always addresses the last tab.
    /// Zero is deliberately absent so Puzzle's zoomReset binding retains Command-0.
    static let numberedTabs = (1...9).map { ShellShortcut(key: String($0), modifiers: .command) }

    /// Resolve a one-based digit against current ordering; unavailable slots silently have no destination.
    static func tabIndex(number: Int, count: Int) -> Int? {
        guard (1...9).contains(number), count > 0 else { return nil }
        let index = number == 9 ? count - 1 : number - 1
        return index < count ? index : nil
    }

    /// Pane defaults live beside every other shell hotkey; persisted overrides are read when menus are installed.
    static let splitLeftDefault = ShellShortcut(key: functionKey(NSLeftArrowFunctionKey), modifiers: [.control, .command])
    /// Place the new pane to the right of the focused pane.
    static let splitRightDefault = ShellShortcut(key: functionKey(NSRightArrowFunctionKey), modifiers: [.control, .command])
    /// Place the new pane above the focused pane.
    static let splitTopDefault = ShellShortcut(key: functionKey(NSUpArrowFunctionKey), modifiers: [.control, .command])
    /// Place the new pane below the focused pane.
    static let splitBottomDefault = ShellShortcut(key: functionKey(NSDownArrowFunctionKey), modifiers: [.control, .command])
    /// Close a focused pane separately from Cmd-W's complete-tab semantics.
    static let closePaneDefault = ShellShortcut(key: "w", modifiers: [.control, .command])
    /// Traverse the pane tree in visual order.
    static let nextPaneDefault = ShellShortcut(key: "]", modifiers: [.control, .command])
    /// Traverse the pane tree backward, wrapping at the first leaf.
    static let previousPaneDefault = ShellShortcut(key: "[", modifiers: [.control, .command])

    /// Native field-editor actions use the standard macOS responder-chain equivalents.
    static let cut = ShellShortcut(key: "x", modifiers: .command)
    /// Copy remains selection-aware on a terminal and ordinary editing in fields.
    static let copy = ShellShortcut(key: "c", modifiers: .command)
    /// The responder chooses field paste or the terminal's MIME-aware user-paste path.
    static let paste = ShellShortcut(key: "v", modifiers: .command)
    /// Select all applies to the focused editor, never a background pane.
    static let selectAll = ShellShortcut(key: "a", modifiers: .command)
    /// Undo belongs to the native editor while editing a form.
    static let undo = ShellShortcut(key: "z", modifiers: .command)
    /// Redo is the conventional shifted undo shortcut.
    static let redo = ShellShortcut(key: "z", modifiers: [.command, .shift])

    /// Resolve launch-time user preferences without scattering shortcut lookup into terminal input code.
    static var splitLeft: ShellShortcut { configured("splitLeft", fallback: splitLeftDefault) }
    /// Resolve the right split binding using the same validation policy.
    static var splitRight: ShellShortcut { configured("splitRight", fallback: splitRightDefault) }
    /// Resolve the top split binding using the same validation policy.
    static var splitTop: ShellShortcut { configured("splitTop", fallback: splitTopDefault) }
    /// Resolve the bottom split binding using the same validation policy.
    static var splitBottom: ShellShortcut { configured("splitBottom", fallback: splitBottomDefault) }
    /// Resolve the focused-pane destruction shortcut.
    static var closePane: ShellShortcut { configured("closePane", fallback: closePaneDefault) }
    /// Resolve forward pane navigation independently of title-bar navigation.
    static var nextPane: ShellShortcut { configured("nextPane", fallback: nextPaneDefault) }
    /// Resolve backward pane navigation independently of title-bar navigation.
    static var previousPane: ShellShortcut { configured("previousPane", fallback: previousPaneDefault) }

    /// Missing or malformed preferences leave the documented code defaults available.
    private static func configured(_ name: String, fallback: ShellShortcut) -> ShellShortcut {
        let prefix = "PuzzleShell.Shortcuts.\(name)"
        return configured(key: UserDefaults.standard.string(forKey: prefix + ".key"),
                          modifiers: UserDefaults.standard.stringArray(forKey: prefix + ".modifiers"), fallback: fallback)
    }

    /// A pure parser makes configuration testable without changing the user's actual preferences.
    /// Named arrows avoid making users enter AppKit private-use Unicode scalars into preference files.
    static func configured(key: String?, modifiers: [String]?, fallback: ShellShortcut) -> ShellShortcut {
        let arrows = ["left": functionKey(NSLeftArrowFunctionKey), "right": functionKey(NSRightArrowFunctionKey),
                      "up": functionKey(NSUpArrowFunctionKey), "down": functionKey(NSDownArrowFunctionKey)]
        let resolved = key.map { arrows[$0.lowercased()] ?? $0.lowercased() } ?? fallback.key
        guard resolved.unicodeScalars.count == 1 else { return fallback } // AppKit menu equivalents represent one logical key.
        var flags = fallback.modifiers
        if let modifiers {
            flags = []
            let known: [String: NSEvent.ModifierFlags] = ["command": .command, "control": .control, "option": .option, "shift": .shift]
            for name in modifiers {
                guard let flag = known[name.lowercased()] else { return fallback }
                flags.insert(flag)
            }
        }
        guard flags.contains(.command) || flags.contains(.control),
              resolved.unicodeScalars.allSatisfy({ !CharacterSet.controlCharacters.contains($0) }) else { return fallback }
        return ShellShortcut(key: resolved, modifiers: flags)
    }

    /// AppKit represents arrow menu equivalents with private-use Unicode function-key characters.
    /// Its named constants are valid scalars; converting them avoids unexplained Unicode magic numbers.
    private static func functionKey(_ key: Int) -> String { String(UnicodeScalar(UInt32(key))!) }
}
