// Numbered native menu navigation uses real panes and C input so Command digits cannot leak to the PTY.
import AppKit
#if PUZZLE_ENABLED

/// AppKit menu equivalents are exercised through TerminalInput, without synthesizing system keyboard events.
@MainActor
final class NativeTabShortcutTests {
    /// Shared runtime uses host transport: fixtures neither launch shells nor depend on the user's terminal contents.
    let runtime: PuzzleRuntime
    /// Capture every pane's terminal bytes, including accidental delivery to an inactive tab.
    private var input = Data()
    /// Hardware codes are fixture facts; menu matching itself uses the logical digit.
    private static let codes: [Int: UInt16] = [0: 29, 1: 18, 5: 23, 8: 28, 9: 25]

    /// Retain only the existing runtime; all test windows have production ownership and teardown.
    init(_ runtime: PuzzleRuntime) { self.runtime = runtime }

    /// Install actual menus temporarily and cover three/ten tabs, reorder, focused panes and tear-off.
    func run() {
        let delegate = ShellAppDelegate()
        let menu = NSApp.mainMenu
        let windowsMenu = NSApp.windowsMenu
        let observer = runtime.eventObserver
        let previousWindow = NSApp.keyWindow
        delegate.installMenus()
        let owner = ShellCoordinator.shared.createWindow(initialTab: false)
        runtime.eventObserver = { [weak self] paneID, name, data in
            guard let self else { return }
            NativeTransportWriter.consume(self.runtime, paneID: paneID, name: name, payload: data) { self.input.append($0) }
        }
        defer {
            owner.window?.close()
            runtime.eventObserver = observer
            NSApp.mainMenu = menu
            NSApp.windowsMenu = windowsMenu
            previousWindow?.makeKeyAndOrderFront(nil)
        }
        checkMenu(delegate)
        for count in 1...10 {
            let tab = owner.addTab()
            // All-key reporting makes leaked Command digits observable, even though legacy encoding suppresses them.
            barrier(tab, output: "\u{1b}[>31u")
            if count == 3 || count == 10 { checkNumbers(owner) }
        }
        owner.reorder(owner.tabs.last!.id, to: 0)
        checkNumbers(owner) // Resolve against visible order, not creation IDs or the prior selected index.
        checkPaneFocus(owner)
        checkTransfer(owner)
        precondition(!NSApp.mainMenu!.performKeyEquivalent(with: event(0, pane: owner.selectedTab!.surface)),
                     "Command-0 remains available to Puzzle's zoomReset binding")
        print("PASS: numbered hidden menu aliases, Cmd-1/5/8/9 with three/ten tabs, silent missing slots, reorder/tear-off order, saved pane focus and no terminal input")
    }

    /// Nine hidden aliases use the shared table, remain enabled with missing slots and reserve no zero.
    private func checkMenu(_ delegate: ShellAppDelegate) {
        let items = NSApp.windowsMenu!.items.filter { $0.action == NSSelectorFromString("selectNumberedTab:") }
        precondition(items.count == 9)
        for (index, item) in items.enumerated() {
            precondition(item.keyEquivalent == ShellShortcuts.numberedTabs[index].key && item.keyEquivalentModifierMask == .command)
            precondition(item.tag == index + 1 && item.isHidden && item.allowsKeyEquivalentWhenHidden)
            precondition(item.target === delegate)
        }
        precondition(!NSApp.windowsMenu!.items.contains { $0.keyEquivalent == "0" })
    }

    /// A DSR queues behind input on the real Session owner, making the no-leak assertion deterministic.
    private func barrier(_ tab: TerminalTab, output: String = "", reset: Bool = true) {
        if reset { input.removeAll() }
        let bytes = output + "\u{1b}[5n"
        var accepted = 0
        precondition(PuzzleRuntime.withBytes(bytes) { puzzle_surface_feed_output(runtime.surfaces[tab.surface.paneID]!, $0, &accepted) } == PUZZLE_OK)
        precondition(accepted == bytes.utf8.count)
        PuzzleRuntimeTests.until("Numbered shortcut input barrier") { self.input.suffix(4) == Data("\u{1b}[0n".utf8) }
        precondition(input == Data("\u{1b}[0n".utf8), "Menu equivalents must never reach any terminal pane")
    }

    /// Use the destination view's production key-equivalent method, including its main-menu-first guard.
    private func invoke(_ number: Int, owner: TerminalWindowController) {
        let pane = owner.selectedTab!.surface
        input.removeAll() // Clear before dispatch so even synchronously delivered stray bytes remain observable.
        precondition(owner.window === NSApp.keyWindow && owner.window?.firstResponder === pane)
        // Switching detaches the source pane, so preserve the release's original native window before dispatch.
        let release = event(number, pane: pane, type: .keyUp)
        precondition(pane.performKeyEquivalent(with: event(number, pane: pane)), "Native menu consumes numbered shortcut")
        precondition(pane.pressedKeys[Self.codes[number]!] == nil, "TerminalInput must not send a menu key to Puzzle")
        // A menu-handled press has no remembered terminal press, so release cannot create kitty input either.
        pane.keyUp(with: release)
        barrier(owner.selectedTab!, reset: false)
    }

    /// Logical menu keys retain the real macOS hardware identity for the input-leak witness.
    private func event(_ number: Int, pane: TerminalSurfaceView, type: NSEvent.EventType = .keyDown) -> NSEvent {
        NSEvent.keyEvent(with: type, location: .zero, modifierFlags: .command, timestamp: 0,
                        windowNumber: pane.window!.windowNumber, context: nil, characters: String(number),
                        charactersIgnoringModifiers: String(number), isARepeat: false, keyCode: Self.codes[number]!)!
    }

    /// Missing slots stay selected without a beep-producing disabled key equivalent.
    private func checkNumbers(_ owner: TerminalWindowController) {
        for number in [1, 5, 8, 9] {
            owner.select(owner.tabs[1].id)
            let before = owner.selectedID
            let expected = number == 9 ? owner.tabs.last!.id : number <= owner.tabs.count ? owner.tabs[number - 1].id : before
            invoke(number, owner: owner)
            precondition(owner.selectedID == expected)
            precondition(owner.window?.firstResponder === owner.selectedTab!.surface)
        }
    }

    /// Keyboard selection must restore the same saved leaf as the title-bar click selection path.
    private func checkPaneFocus(_ owner: TerminalWindowController) {
        let first = owner.tabs[0]
        owner.select(first.id)
        let pane = first.split(.right)
        precondition(pane != 0 && first.layout.activeID == pane)
        owner.select(owner.tabs[1].id)
        invoke(1, owner: owner)
        precondition(first.layout.activeID == pane && owner.window?.firstResponder === first.panes[pane])
    }

    /// Tear-off updates ownership and ordering; digits target the key window exclusively.
    private func checkTransfer(_ owner: TerminalWindowController) {
        let moved = owner.tabs[4]
        let before = owner.selectedID
        guard let destination = ShellCoordinator.shared.moveToNewWindow(moved.id) else { preconditionFailure("Tear-off failed") }
        invoke(9, owner: destination)
        precondition(destination.selectedID == moved.id && owner.selectedID == before)
        destination.window?.close()
        owner.window?.makeKeyAndOrderFront(nil)
        invoke(9, owner: owner)
        precondition(owner.selectedID == owner.tabs.last!.id && owner.tabs.count == 9)
    }
}
#endif
