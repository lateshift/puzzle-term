// Native integration exercises Cocoa → real puzzle.h → host transport, never a mock runtime.
import AppKit

/// Separate from the host C observer tests: these witnesses verify the runtime actually calls Puzzle.
@MainActor
enum PuzzleRuntimeTests {
    /// Advance AppKit and queued ticks until a functional result arrives; timeout is a hang guard.
    /// Lazy failure text captures the final neutral transport bytes rather than a pre-wait snapshot.
    static func until(_ message: @autoclosure () -> String, _ predicate: () -> Bool) {
        let deadline = Date().addingTimeInterval(15)
        while !predicate() && Date() < deadline {
            RunLoop.main.run(until: Date().addingTimeInterval(0.01))
            // NSApplication.run normally dispatches these notifications. This standalone
            // executable must dispatch native events too, including window occlusion changes.
            while let event = NSApp.nextEvent(matching: .any, until: .distantPast, inMode: .default, dequeue: true) {
                NSApp.sendEvent(event)
            }
            NSApp.updateWindows()
        }
        precondition(predicate(), message())
    }

    /// Enabled native tests retain actual layers, send Cocoa keys and observe exact C callback bytes.
    static func run() {
        #if PUZZLE_ENABLED
        guard let runtime = PuzzleRuntime(transport: 1) else { preconditionFailure("Puzzle App creation failed") }
        PuzzleGlue.shared.runtime = runtime
        var input = Data()
        var firstFrames = 0
        var caretEvents = 0
        runtime.eventObserver = { [weak runtime] paneID, name, bytes in
            guard let runtime else { return }
            NativeTransportWriter.consume(runtime, paneID: paneID, name: name, payload: bytes) { input.append($0) }
            if name == "surface/firstFrame" { firstFrames += 1 }
            if name == "surface/cursorRect" {
                caretEvents += 1
            }
        }
        // Unlike the host-only tests, this fixture needs an exposed, activating window:
        // a prohibited-activation process never receives reliable occlusion notifications.
        let application = NSApplication.shared
        let previousPolicy = application.activationPolicy()
        application.setActivationPolicy(.accessory)
        application.finishLaunching()
        defer { application.setActivationPolicy(previousPolicy) }
        let owner = ShellCoordinator.shared.createWindow()
        // This fixture verifies presentation. Prohibited-activation test windows can otherwise
        // remain covered by the user's active app, correctly suspending Puzzle's display link.
        owner.window?.makeKeyAndOrderFront(nil)
        application.activate(ignoringOtherApps: true)
        let pane = owner.selectedTab!.surface
        let handle = runtime.surfaces[pane.paneID]!
        until("First presentation hides the native placeholder") { firstFrames == 1 && !pane.placeholderVisible }
        until("Real cursor and cell observations reach the host") { caretEvents > 0 && runtime.cells[pane.paneID] != nil }
        until("Render fixture is exposed on the actual display") { owner.window?.occlusionState.contains(.visible) == true }
        let presentationChild = NativePresentationTests.run(runtime, pane: pane)
        checkPaddingPointers(runtime, pane: pane, handle: handle, input: { input }, reset: { input.removeAll() })
        NativeSelectionTests.run(runtime, pane: pane, input: { input }, reset: { input.removeAll() })
        checkKeys(pane, input: { input }, reset: { input.removeAll() })
        checkComposition(pane, input: { input }, reset: { input.removeAll() })
        // Output changes the actual caret; its new rectangle must reach the same NSTextInputClient.
        let previousCaret = caretEvents
        let output = "\u{1b}[2J\u{1b}[Habc"
        var accepted = 0
        precondition(PuzzleRuntime.withBytes(output) { puzzle_surface_feed_output(handle, $0, &accepted) } == PUZZLE_OK)
        precondition(accepted == output.utf8.count)
        // The C observation precedes the deferred AppKit effect. Wait for the
        // new three-character caret too, so a prior queued rect cannot satisfy this.
        until("Output delivers and applies a new cursor rectangle") {
            caretEvents > previousCaret && pane.imeCursorRect.minX > pane.imeCursorRect.width * 2
        }
        precondition(pane.imeCursorRect.minX > 0, "Output updates native IME rectangle: \(pane.imeCursorRect)")
        precondition(firstFrames == 1, "Later input, padding changes and output never repeat first presentation")
        NativeTabShortcutTests(runtime).run() // Menu equivalents precede Puzzle input and preserve native pane focus.
        HostExtensionTests.drag(runtime) // Lazy source data and same-app destination policy use real native boards.
        NativeDropTests.run(runtime) // Real native dragging callbacks drive the C protocol owner and isolated boards.
        NativeNotificationTests.run(runtime) // Real C notification lifecycle with a controlled native OS boundary.
        NativeClipboardTests.run(runtime, owner: owner) // Real MIME callbacks/paste grants use an isolated native clipboard.
        HostExtensionTests.status(runtime, owner: owner) // Real OSC deltas drive native tab status and its lifecycle.
        let retainedHandle = handle
        owner.window?.close()
        precondition(presentationChild.superlayer == nil && presentationChild.contents == nil,
                     "Synchronous close detaches and clears the retained Puzzle IOSurface child")
        precondition(runtime.surfaces.isEmpty && puzzle_surface_focus(retainedHandle, 0) != PUZZLE_OK,
                     "Native close synchronously retires the C surface")
        PuzzleGlue.shared.stop()
        precondition(runtime.app == 0, "Application shutdown joins and releases Puzzle")
        print("PASS: Puzzle ownership/release, first presentation, caret callbacks, default/zero-padding cell 1,1 clicks, Cocoa key/IME exact input")
        #endif
    }

    #if PUZZLE_ENABLED
    /// Change the same surface's padding and await its actual presented caret before clicking again.
    private static func checkPaddingPointers(_ runtime: PuzzleRuntime, pane: TerminalSurfaceView, handle: UInt64,
                                            input: () -> Data, reset: () -> Void) {
        checkPointer(runtime, pane: pane, handle: handle, input: input, reset: reset)
        // No host geometry cache or pointer offset is patched when this setting changes.
        precondition(pane.imeCursorRect.minX > 0 && pane.imeCursorRect.minY > 0,
                     "Default Puzzle padding is present in the rendered first-cell rectangle")
        precondition(PuzzleRuntime.withBytes("{\"render\":{\"padding\":0}}") {
            puzzle_surface_set_config(handle, $0)
        } == PUZZLE_OK)
        until("Live zero padding reaches the native cursor rectangle") {
            pane.imeCursorRect.minX == 0 && pane.imeCursorRect.minY == 0 && pane.imeCursorRect.width > 0
        }
        checkPointer(runtime, pane: pane, handle: handle, input: input, reset: reset)
    }

    /// A click inside the rendered first cell must report column/row 1, independently of padding.
    /// This uses the same top-left point payload as Cocoa rather than guessing the grid's origin.
    private static func checkPointer(_ runtime: PuzzleRuntime, pane: TerminalSurfaceView, handle: UInt64,
                                     input: () -> Data, reset: () -> Void) {
        reset()
        var accepted = 0
        // The fresh host-transport terminal is blank with its cursor at 1,1.
        // DSR is an ordered acknowledgement that both mouse modes were parsed
        // before the control-channel pointer input, which otherwise could overtake output.
        let output = "\u{1b}[?1000h\u{1b}[?1006h\u{1b}[6n"
        precondition(PuzzleRuntime.withBytes(output) { puzzle_surface_feed_output(handle, $0, &accepted) } == PUZZLE_OK)
        precondition(accepted == output.utf8.count, "Pointer fixture output is accepted in full")
        until("Mouse modes are parsed and the rendered caret is at terminal cell 1,1; neutral transport bytes=\(Array(input()))") {
            input() == Data("\u{1b}[1;1R".utf8)
        }
        reset()
        let rect = pane.imeCursorRect
        var event = PuzzleShellEvent()
        event.pane_id = pane.paneID
        event.code = 0 // Native left-button press, in view-local top-left points.
        event.detail = 0
        event.x = rect.minX + rect.width * 0.75
        event.y = rect.midY
        runtime.pointer(event)
        until("Rendered first-cell click reaches the real C transport") { !input().isEmpty }
        precondition(input() == Data("\u{1b}[<0;1;1M".utf8),
                     "Rendered first-cell click must report 1,1; caret=\(rect), bytes=\(Array(input()))")
        print("PASS: first-cell SGR 1;1, caret=\(rect), scale=\(pane.metalLayer.contentsScale), grid=\(String(describing: runtime.cells[pane.paneID]))")
    }

    /// Table witnesses catch duplicate insertText, C0 controls and named-key translation regressions.
    private static func checkKeys(_ pane: TerminalSurfaceView, input: () -> Data, reset: () -> Void) {
        for (characters, base, code, flags, expected): (String, String, UInt16, NSEvent.ModifierFlags, [UInt8]) in [
            ("a", "a", 0, [], [97]), ("\u{3}", "c", 8, [.control], [3]),
            ("\r", "\r", 36, [], [13]), ("\u{f700}", "\u{f700}", 126, [], [27, 91, 65])
        ] {
            reset()
            let down = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: flags, timestamp: 0,
                                       windowNumber: pane.window!.windowNumber, context: nil, characters: characters,
                                       charactersIgnoringModifiers: base, isARepeat: false, keyCode: code)!
            pane.keyDown(with: down)
            let up = NSEvent.keyEvent(with: .keyUp, location: .zero, modifierFlags: flags, timestamp: 0,
                                     windowNumber: pane.window!.windowNumber, context: nil, characters: characters,
                                     charactersIgnoringModifiers: base, isARepeat: false, keyCode: code)!
            pane.keyUp(with: up)
            until("Cocoa key produces exact terminal bytes once") { input().count >= expected.count }
            precondition(input() == Data(expected), "Duplicate or incorrect native key input: \(Array(input()))")
        }
    }

    /// A marked UTF-16 buffer remains provisional; its Unicode commit is sent once as UTF-8.
    private static func checkComposition(_ pane: TerminalSurfaceView, input: () -> Data, reset: () -> Void) {
        reset()
        pane.setMarkedText("漢🧩", selectedRange: NSRange(location: 3, length: 0), replacementRange: NSRange(location: NSNotFound, length: 0))
        pane.insertText("漢字🧩", replacementRange: NSRange(location: NSNotFound, length: 0))
        let expected = Data("漢字🧩".utf8)
        until("IME commit reaches real Puzzle transport") { input().count >= expected.count }
        precondition(input() == expected && !pane.hasMarkedText(), "Preedit must not leak or duplicate a commit")
    }
    #endif
}
