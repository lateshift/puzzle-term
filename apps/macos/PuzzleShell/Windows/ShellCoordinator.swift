import AppKit

/// Retains controllers after showWindow and provides process-wide ID routing for the C ABI.
@MainActor
final class ShellCoordinator {
    /// The standalone app and exported C functions use the same window registry.
    static let shared = ShellCoordinator()
    /// Window controllers must outlive their windows because NSWindow.delegate is weak.
    private(set) var windows: [UInt64: TerminalWindowController] = [:]
    /// Window and tab IDs share one namespace, reserving zero for a failed C command.
    private var nextID: UInt64 = 1

    /// Resolve key-window ownership, avoiding accidental commands against a background window.
    var activeWindow: TerminalWindowController? {
        windows.values.first { $0.window === NSApp.keyWindow }
    }

    /// Allocate monotonically, failing loudly before an overflow could recycle a stale ID.
    func allocateID() -> UInt64 {
        precondition(nextID < UInt64.max, "Shell ID namespace exhausted")
        defer { nextID += 1 }
        return nextID
    }

    /// Count only terminal creation, independently of window IDs and current tab counts.
    private var nextTerminalNumber: UInt64 = 1

    /// Allocate a fallback label once; abandoned/closed session numbers are deliberately not reused.
    func allocateFallbackName() -> String {
        precondition(nextTerminalNumber < UInt64.max, "Terminal name namespace exhausted")
        defer { nextTerminalNumber += 1 }
        return "Terminal \(nextTerminalNumber)"
    }

    /// Register the window before emitting events so Rust sees a fully addressable object.
    @discardableResult
    func createWindow(initialTab: Bool = true) -> TerminalWindowController {
        let owner = TerminalWindowController(id: allocateID())
        windows[owner.id] = owner
        ShellBridge.shared.emit(ShellBridge.shared.event(PUZZLE_WINDOW_CREATED, window: owner.id))
        if initialTab { owner.addTab() }
        owner.showWindow(nil)
        owner.window?.makeKeyAndOrderFront(nil)
        return owner
    }

    /// Search current ownership instead of maintaining a second mapping that might become stale.
    func owner(of tabID: UInt64) -> TerminalWindowController? {
        windows.values.first { $0.tabs.contains(where: { $0.id == tabID }) }
    }

    /// Resolve a pane through current tab ownership so transfers never leave a stale routing table.
    func tab(containing paneID: UInt64) -> TerminalTab? {
        windows.values.lazy.flatMap(\.tabs).first { $0.panes[paneID] != nil }
    }

    /// Notify closure exactly once after all tab-closing callbacks have completed.
    func removeWindow(_ id: UInt64) {
        guard windows.removeValue(forKey: id) != nil else { return }
        ShellBridge.shared.emit(ShellBridge.shared.event(PUZZLE_WINDOW_CLOSED, window: id))
    }

    /// Move to an exact final index, shared by native drag/drop and the public C command.
    /// Validate everything before detaching so rejected drops leave both windows untouched.
    @discardableResult
    func moveTab(_ tabID: UInt64, to windowID: UInt64, at index: Int, selectOnArrival: Bool = true) -> Bool {
        guard !ShellBridge.shared.isDelivering, let source = owner(of: tabID),
              let destination = windows[windowID] else { return false }
        let remainingCount = destination.tabs.count - (source === destination ? 1 : 0)
        guard index >= 0, index <= remainingCount else { return false }
        if source === destination {
            source.reorder(tabID, to: index) // Reordering must not disturb focus or mount the surface again.
            return true
        }
        guard let tab = source.detachTab(tabID) else { return false }
        tab.windowID = destination.id
        // Ownership reaches Rust before destination selection, geometry, and focus notifications.
        var moved = ShellBridge.shared.event(PUZZLE_TAB_MOVED, window: destination.id, tab: tab.id)
        moved.code = UInt32(clamping: index)
        destination.adopt(tab, at: index, selectOnArrival: selectOnArrival) {
            ShellBridge.shared.emit(moved) // Callback getters now resolve the tab in its destination.
        }
        if selectOnArrival { destination.window?.makeKeyAndOrderFront(nil) }
        if source.tabs.isEmpty { source.window?.close() } // Last-tab transfers close only the empty source.
        return true
    }

    /// Collect without mounting each arriving surface, preserving the destination's selection/focus.
    /// Snapshot the plan before transfers close source windows and mutate the live registry.
    @discardableResult
    func collectTabs(into windowID: UInt64) -> Bool {
        guard !ShellBridge.shared.isDelivering, let destination = windows[windowID] else { return false }
        let plan = TabMetadata.collectionPlan(destination: windowID, windows: windows.values.map { ($0.id, $0.tabs.map(\.id)) })
        for tabID in plan {
            guard moveTab(tabID, to: windowID, at: destination.tabs.count, selectOnArrival: false) else { return false }
        }
        destination.window?.makeKeyAndOrderFront(nil)
        return true
    }

    /// A menu/FFI tear-off uses the same transfer path as dropping into an existing window.
    @discardableResult
    func moveToNewWindow(_ tabID: UInt64, at screenPoint: NSPoint? = nil) -> TerminalWindowController? {
        guard !ShellBridge.shared.isDelivering, let source = owner(of: tabID) else { return nil }
        let sourceWindow = source.window
        let sourceScreen = source.window?.screen
        // A maximized source must stay expanded while its torn-off tab gets a usable independent window.
        // Ordinary windows retain their familiar dimensions; expanded sources use the new window's normal initial size.
        let sourceSize = sourceWindow.flatMap { window -> NSSize? in
            TabTearOffPolicy.prefersNormalWindowSize(source: window.frame.size, visibleScreen: sourceScreen?.visibleFrame.size,
                                                     zoomed: window.isZoomed, fullscreen: window.styleMask.contains(.fullScreen)) ? nil : window.frame.size
        }
        let destination = createWindow(initialTab: false)
        if let point = screenPoint, let window = destination.window {
            // Keep the lifted tab near the pointer; expanded sources retain the destination's normal initial dimensions.
            var frame = window.frame
            if let sourceSize { frame.size = sourceSize }
            if let screen = NSScreen.screens.first(where: { $0.frame.contains(point) }) ?? sourceScreen ?? NSScreen.main {
                let visible = screen.visibleFrame
                frame.size.width = min(frame.width, visible.width)
                frame.size.height = min(frame.height, visible.height)
                frame.origin.x = min(max(point.x - 100, visible.minX), visible.maxX - frame.width)
                frame.origin.y = min(max(point.y + 20 - frame.height, visible.minY), visible.maxY - frame.height)
            }
            window.setFrame(frame, display: true) // Public AppKit geometry also handles secondary displays.
        }
        guard moveTab(tabID, to: destination.id, at: 0) else {
            destination.window?.close()
            return nil
        }
        return destination
    }

    /// Snapshot existing state in ID order for predictable callback registration and diagnostics.
    func replayState() {
        for owner in windows.values.sorted(by: { $0.id < $1.id }) {
            ShellBridge.shared.emit(ShellBridge.shared.event(PUZZLE_WINDOW_CREATED, window: owner.id))
            for tab in owner.tabs {
                tab.emitCreated()
                owner.emitMetadata(for: tab)
                tab.emitGeometry()
                for pane in tab.orderedSurfaces { pane.emitVisibility() }
                tab.emitSelection() // Inactive tabs also remember a focused-leaf choice for their next activation.
            }
            if let tab = owner.selectedTab {
                ShellBridge.shared.emit(ShellBridge.shared.event(PUZZLE_TAB_SELECTED, window: owner.id, tab: tab.id))
                tab.emitFocus()
            }
        }
    }

    /// Copy the collection before closing; delegate callbacks mutate the live registry.
    func closeAllWindows() {
        for owner in Array(windows.values) { owner.window?.close() }
    }
}
