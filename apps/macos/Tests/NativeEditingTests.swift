// Native responder-chain and field-editor checks, using isolated pasteboards rather than personal clipboard data.
import AppKit
#if PUZZLE_ENABLED

/// Verify native field editing and any separately installed optional host component.
@MainActor
enum NativeEditingTests {
    /// Temporary menus/window ownership never alter the user's ordinary windows, defaults or clipboard contents.
    static func run() {
        let delegate = ShellAppDelegate()
        let oldMenu = NSApp.mainMenu, oldWindows = NSApp.windowsMenu
        delegate.installMenus()
        let owner = ShellCoordinator.shared.createWindow(initialTab: false)
        let name = "PuzzleShell.SheetEditingTests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: name)!
        defer {
            owner.window?.close()
            NSApp.mainMenu = oldMenu
            NSApp.windowsMenu = oldWindows
            defaults.removePersistentDomain(forName: name)
        }
        let menu = NSApp.mainMenu!.items.first { $0.title == "Edit" }!.submenu!
        for action in ["cut:", "copy:", "paste:", "selectAll:", "undo:", "redo:"] {
            let item = menu.items.first { $0.action == Selector(action) }!
            precondition(item.target == nil && item.keyEquivalentModifierMask.contains(.command))
        }
        let field = NSTextField(frame: NSRect(x: 20, y: 30, width: 300, height: 24))
        owner.window!.contentView!.addSubview(field)
        checkField(field, window: owner.window!)
        HostExtensionTests.editing(owner: owner, defaults: defaults)
        print("PASS: nil-target Edit menu, native field selection/copy/paste/undo/redo in ordinary windows and optional host components")
    }

    /// Normal NSTextView clipboard serialization and undo are tested without reading/writing NSPasteboard.general.
    static func checkField(_ field: NSTextField, window: NSWindow) {
        window.makeKeyAndOrderFront(nil)
        window.makeFirstResponder(nil) // End automatic initial editing before installing this fixture's value.
        field.stringValue = "before"
        precondition(window.makeFirstResponder(field))
        let editor = field.currentEditor() as! NSTextView
        precondition(editor.textStorage != nil && editor.layoutManager != nil && editor.allowsUndo)
        precondition(editor.string == "before", "Native field editor must load the field value, got ‘\(editor.string)’ from ‘\(field.stringValue)’")
        NSApp.mainMenu!.items.first { $0.title == "Edit" }!.submenu!.update() // Native validation normally runs between user events.
        for action in ["cut:", "copy:", "paste:", "selectAll:"] {
            precondition(NSApp.target(forAction: Selector(action), to: nil, from: nil) as AnyObject? === editor,
                         "Every editing action must target the active native editor, never the terminal behind it")
        }
        let select = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: .command, timestamp: 0,
            windowNumber: window.windowNumber, context: nil, characters: "a", charactersIgnoringModifiers: "a", isARepeat: false, keyCode: 0)!
        precondition(NSApp.mainMenu!.performKeyEquivalent(with: select), "Cmd-A must be handled by the native Edit menu")
        precondition(editor.selectedRange().length == 6, "Cmd-A must select the active editor's complete fixture value; range=\(editor.selectedRange()), text=‘\(editor.string)’, keyWindow=\(NSApp.keyWindow === window)")
        let board = NSPasteboard.withUniqueName()
        defer { board.releaseGlobally() }
        precondition(editor.writeSelection(to: board, types: editor.writablePasteboardTypes) && board.string(forType: .string) == "before")
        board.clearContents()
        board.setString("literal ‘quotes’ $(no shell)", forType: .string)
        let undo = editor.undoManager!
        undo.groupsByEvent = false
        undo.removeAllActions()
        undo.beginUndoGrouping()
        precondition(editor.readSelection(from: board))
        undo.endUndoGrouping()
        precondition(editor.string == "literal ‘quotes’ $(no shell)" && undo.canUndo)
        precondition(NSApp.sendAction(Selector(("undo:")), to: nil, from: nil) && editor.string == "before")
        precondition(NSApp.sendAction(Selector(("redo:")), to: nil, from: nil) && editor.string == "literal ‘quotes’ $(no shell)")
        precondition(NSApp.sendAction(Selector(("undo:")), to: nil, from: nil) && editor.string == "before")
        window.makeFirstResponder(nil)
    }
}
#endif
