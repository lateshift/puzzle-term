// Native app lifecycle and menus; editing actions follow the current responder rather than a fixed pane.
import AppKit

/// The app delegate installs native menus and creates the initial independent window.
@MainActor
final class ShellAppDelegate: NSObject, NSApplicationDelegate, NSMenuItemValidation {
    /// Explicitly require secure AppKit restoration coding on every supported macOS version.
    /// This opts into NSSecureCoding; it does not implement terminal-session persistence or restore Rust objects.
    func applicationSupportsSecureRestorableState(_ app: NSApplication) -> Bool { true }

    /// Menus must exist before a window opens so shortcuts work from the first frame.
    func applicationDidFinishLaunching(_ notification: Notification) {
        installMenus()
        PuzzleGlue.shared.start() // One owner must exist before PANE_CREATED for the first window.
        let owner = ShellCoordinator.shared.createWindow()
        PuzzleAppProbe.start(owner: owner)
        NSApp.activate(ignoringOtherApps: true) // Bring the newly launched shell to the front.
    }

    /// Retain the macOS convention that closing all windows does not quit the application.
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }

    /// Clicking the Dock icon with no windows creates a fresh placeholder session.
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        if !flag { ShellCoordinator.shared.createWindow() }
        return true
    }

    /// Tear down surfaces while Rust callbacks and AppKit are still alive on the main thread.
    func applicationWillTerminate(_ notification: Notification) {
        ShellCoordinator.shared.closeAllWindows()
        PuzzleGlue.shared.stop() // Synchronous per-pane frees precede the process-wide worker join.
    }

    /// Build responder-chain menus so commands always target the currently focused window.
    func installMenus() {
        let bar = NSMenu()
        let appMenu = NSMenu(title: "PuzzleShell")
        append("About PuzzleShell", action: #selector(showAbout(_:)), to: appMenu, target: self)
        appMenu.addItem(.separator())
        append("Hide PuzzleShell", action: #selector(NSApplication.hide(_:)), shortcut: ShellShortcuts.hide, to: appMenu)
        append("Hide Others", action: #selector(NSApplication.hideOtherApplications(_:)), shortcut: ShellShortcuts.hideOthers, to: appMenu)
        append("Show All", action: #selector(NSApplication.unhideAllApplications(_:)), to: appMenu)
        appMenu.addItem(.separator())
        append("Quit PuzzleShell", action: #selector(NSApplication.terminate(_:)), shortcut: ShellShortcuts.quit, to: appMenu)
        attach(appMenu, to: bar)

        let file = NSMenu(title: "File")
        append("New Window", action: #selector(newWindow(_:)), shortcut: ShellShortcuts.newWindow, to: file, target: self)
        append("New Tab", action: #selector(newTab(_:)), shortcut: ShellShortcuts.newTab, to: file, target: self)
        HostExtensions.installMenu(delegate: self, menu: file)
        file.addItem(.separator())
        append("Close Tab", action: #selector(closeTab(_:)), shortcut: ShellShortcuts.closeTab, to: file, target: self)
        append("Close Window", action: #selector(NSWindow.performClose(_:)), shortcut: ShellShortcuts.closeWindow, to: file)
        attach(file, to: bar)

        let edit = NSMenu(title: "Edit")
        for (title, action, shortcut) in [("Undo", "undo:", ShellShortcuts.undo), ("Redo", "redo:", ShellShortcuts.redo),
            ("Cut", "cut:", ShellShortcuts.cut), ("Copy", "copy:", ShellShortcuts.copy),
            ("Paste", "paste:", ShellShortcuts.paste), ("Select All", "selectAll:", ShellShortcuts.selectAll)] {
            if title == "Cut" || title == "Select All" { edit.addItem(.separator()) }
            append(title, action: Selector(action), shortcut: shortcut, to: edit) // Nil target follows the actual field editor/pane.
        }
        attach(edit, to: bar)

        let pane = NSMenu(title: "Pane")
        append("Split Left", action: #selector(splitLeft(_:)), shortcut: ShellShortcuts.splitLeft, to: pane, target: self)
        append("Split Right", action: #selector(splitRight(_:)), shortcut: ShellShortcuts.splitRight, to: pane, target: self)
        append("Split Top", action: #selector(splitTop(_:)), shortcut: ShellShortcuts.splitTop, to: pane, target: self)
        append("Split Bottom", action: #selector(splitBottom(_:)), shortcut: ShellShortcuts.splitBottom, to: pane, target: self)
        pane.addItem(.separator())
        append("Next Pane", action: #selector(nextPane(_:)), shortcut: ShellShortcuts.nextPane, to: pane, target: self)
        append("Previous Pane", action: #selector(previousPane(_:)), shortcut: ShellShortcuts.previousPane, to: pane, target: self)
        append("Close Pane", action: #selector(closePane(_:)), shortcut: ShellShortcuts.closePane, to: pane, target: self)
        attach(pane, to: bar)

        let window = NSMenu(title: "Window")
        append("Minimize", action: #selector(NSWindow.performMiniaturize(_:)), shortcut: ShellShortcuts.minimize, to: window)
        append("Zoom", action: #selector(NSWindow.performZoom(_:)), to: window)
        window.addItem(.separator())
        appendTabShortcuts(to: window)
        append("Move Tab to New Window", action: #selector(moveTab(_:)), to: window, target: self)
        append("Collect All Tabs Here", action: #selector(collectTabs(_:)), to: window, target: self)
        window.addItem(.separator())
        append("Rename Tab…", action: #selector(renameTab(_:)), to: window, target: self)
        append("Use Automatic Name", action: #selector(resetTabName(_:)), to: window, target: self)
        window.addItem(.separator())
        append("Bring All to Front", action: #selector(NSApplication.arrangeInFront(_:)), to: window)
        attach(window, to: bar)
        NSApp.windowsMenu = window // AppKit appends its native open-window list here.
        NSApp.mainMenu = bar
    }

    /// Keep navigation aliases in the Window menu without adding nine visible rows.
    private func appendTabShortcuts(to menu: NSMenu) {
        append("Next Tab", action: #selector(nextTab(_:)), shortcut: ShellShortcuts.nextTab, to: menu, target: self)
        append("Previous Tab", action: #selector(previousTab(_:)), shortcut: ShellShortcuts.previousTab, to: menu, target: self)
        append("Next Tab", action: #selector(nextTab(_:)), shortcut: ShellShortcuts.nextTabAlternate, to: menu, target: self, hidden: true)
        append("Previous Tab", action: #selector(previousTab(_:)), shortcut: ShellShortcuts.previousTabAlternate, to: menu, target: self, hidden: true)
        for (index, shortcut) in ShellShortcuts.numberedTabs.enumerated() {
            append(index == 8 ? "Select Last Tab" : "Select Tab \(index + 1)", action: #selector(selectNumberedTab(_:)),
                   shortcut: shortcut, to: menu, target: self, hidden: true, tag: index + 1)
        }
    }

    /// Attach a submenu to the main bar without relying on storyboard resources.
    private func attach(_ menu: NSMenu, to bar: NSMenu) {
        let item = NSMenuItem(title: menu.title, action: nil, keyEquivalent: "")
        item.submenu = menu
        bar.addItem(item)
    }

    /// Distributed renderer/font credits stay reachable in the native About panel (R11).
    @objc private func showAbout(_ sender: Any?) {
        let credits = "Puzzle Terminal\nSlug reference shaders — Eric Lengyel / Terathon Software\n" +
            "JetBrains Mono — JetBrains (SIL OFL 1.1)\nSymbols Nerd Font Mono — Nerd Fonts and icon authors"
        NSApp.orderFrontStandardAboutPanel(options: [.credits: NSAttributedString(string: credits)])
    }

    /// Construct menus from the shared shortcut constants; a nil target uses AppKit's responder chain.
    /// Hidden alias items still participate in native key-equivalent dispatch and menu validation.
    private func append(_ title: String, action: Selector, shortcut: ShellShortcut? = nil, to menu: NSMenu, target: AnyObject? = nil, hidden: Bool = false, tag: Int = 0) {
        let item = NSMenuItem(title: title, action: action, keyEquivalent: shortcut?.key ?? "")
        item.keyEquivalentModifierMask = shortcut?.modifiers ?? [] // Commands without a binding have no implicit shortcut.
        item.target = target
        item.tag = tag // Numbered aliases carry their slot independently of localized titles.
        item.isHidden = hidden
        item.allowsKeyEquivalentWhenHidden = hidden // Preserve aliases without adding duplicate visible entries.
        menu.addItem(item)
    }

    /// Create an independent native window, even when no other window exists.
    @objc private func newWindow(_ sender: Any?) { ShellCoordinator.shared.createWindow() }
    /// Add a tab to the key window, or create a window when invoked from an empty app.
    @objc private func newTab(_ sender: Any?) {
        if let owner = ShellCoordinator.shared.activeWindow { owner.addTab() }
        else { ShellCoordinator.shared.createWindow() }
    }
    /// Close the active tab; its last tab also closes its owning window.
    @objc private func closeTab(_ sender: Any?) { ShellCoordinator.shared.activeWindow?.closeSelectedTab() }
    /// Wrap forward through tabs instead of falling off the end.
    @objc private func nextTab(_ sender: Any?) { ShellCoordinator.shared.activeWindow?.cycleTab(by: 1) }
    /// Wrap backward through tabs instead of falling off the beginning.
    @objc private func previousTab(_ sender: Any?) { ShellCoordinator.shared.activeWindow?.cycleTab(by: -1) }
    /// Resolve the key window's current visible ordering at dispatch, including after reorder or transfer.
    @objc private func selectNumberedTab(_ sender: NSMenuItem) {
        ShellCoordinator.shared.activeWindow?.selectTab(number: sender.tag)
    }
    /// Transfer the existing session into a new window rather than recreate its surface.
    @objc private func moveTab(_ sender: Any?) {
        if let tab = ShellCoordinator.shared.activeWindow?.selectedTab { ShellCoordinator.shared.moveToNewWindow(tab.id) }
    }

    /// Collect into the focused window; the coordinator preserves selection and all session identities.
    @objc private func collectTabs(_ sender: Any?) {
        if let owner = ShellCoordinator.shared.activeWindow { ShellCoordinator.shared.collectTabs(into: owner.id) }
    }

    /// The menu command renames the selected tab; context menus can rename an inactive tab directly.
    @objc private func renameTab(_ sender: Any?) {
        if let owner = ShellCoordinator.shared.activeWindow, let id = owner.selectedID { owner.requestRename(id) }
    }

    /// Reset only the custom override, revealing the most recently supplied runtime title.
    @objc private func resetTabName(_ sender: Any?) {
        if let owner = ShellCoordinator.shared.activeWindow, let id = owner.selectedID {
            owner.updateMetadata(id) { $0.customName = nil }
        }
    }

    /// All directional commands act on the same focused pane and differ only in insertion edge.
    private func split(_ direction: PaneDirection) { ShellCoordinator.shared.activeWindow?.selectedTab?.split(direction) }
    /// Insert and focus a pane on the left.
    @objc private func splitLeft(_ sender: Any?) { split(.left) }
    /// Insert and focus a pane on the right.
    @objc private func splitRight(_ sender: Any?) { split(.right) }
    /// Insert and focus a pane above the current one.
    @objc private func splitTop(_ sender: Any?) { split(.top) }
    /// Insert and focus a pane below the current one.
    @objc private func splitBottom(_ sender: Any?) { split(.bottom) }
    /// Wrap forward through the active tab's visual pane order.
    @objc private func nextPane(_ sender: Any?) { ShellCoordinator.shared.activeWindow?.selectedTab?.cyclePane(by: 1) }
    /// Wrap backward through the active tab's visual pane order.
    @objc private func previousPane(_ sender: Any?) { ShellCoordinator.shared.activeWindow?.selectedTab?.cyclePane(by: -1) }
    /// Destroy only the focused pane; the last pane delegates to the existing tab-close behavior.
    @objc private func closePane(_ sender: Any?) {
        if let tab = ShellCoordinator.shared.activeWindow?.selectedTab { tab.closePane(tab.layout.activeID) }
    }

    /// Keep unavailable commands visibly disabled rather than silently ignoring menu selections.
    func validateMenuItem(_ menuItem: NSMenuItem) -> Bool {
        let owner = ShellCoordinator.shared.activeWindow
        let paneAvailable = owner?.selectedTab != nil && owner?.window?.attachedSheet == nil
        if let additional = HostExtensions.validate(menuItem: menuItem, owner: owner) { return additional }
        switch menuItem.action {
        case #selector(splitLeft(_:)): return paneAvailable && owner?.selectedTab?.canSplit(.left) == true
        case #selector(splitRight(_:)): return paneAvailable && owner?.selectedTab?.canSplit(.right) == true
        case #selector(splitTop(_:)): return paneAvailable && owner?.selectedTab?.canSplit(.top) == true
        case #selector(splitBottom(_:)): return paneAvailable && owner?.selectedTab?.canSplit(.bottom) == true
        case #selector(closePane(_:)): return paneAvailable
        // Keep unavailable numbers enabled: AppKit consumes them and the selection method silently does nothing.
        case #selector(selectNumberedTab(_:)): return paneAvailable
        case #selector(nextPane(_:)), #selector(previousPane(_:)): return paneAvailable && (owner?.selectedTab?.panes.count ?? 0) > 1
        case #selector(collectTabs(_:)): return owner != nil && ShellCoordinator.shared.windows.count > 1
        case #selector(resetTabName(_:)): return owner?.selectedTab?.metadata.customName != nil
        case #selector(renameTab(_:)): return owner?.selectedTab != nil && owner?.window?.attachedSheet == nil
        case #selector(closeTab(_:)), #selector(nextTab(_:)), #selector(previousTab(_:)), #selector(moveTab(_:)):
            return owner?.selectedTab != nil
        default: return true // New Window/New Tab remain available even when no windows exist.
        }
    }
}
