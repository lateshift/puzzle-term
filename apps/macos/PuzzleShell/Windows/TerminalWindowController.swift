// Native window/tab ownership and standard field editing remain independent of each Puzzle surface.
import AppKit

/// One independent native window owns a strip of sessions and mounts only its selected view.
@MainActor
final class TerminalWindowController: NSWindowController, NSWindowDelegate, NSToolbarDelegate {
    /// Normal editing inside window-owned fields uses a shared undo-capable native field editor.
    private let fieldEditor = ShellFieldEditor.make()
    /// Supply only text-field editing; terminal panes keep their own NSTextInputClient responder.
    func windowWillReturnFieldEditor(_ sender: NSWindow, to client: Any?) -> Any? { client is NSTextField ? fieldEditor : nil }

    /// Stable shell identity, separate from AppKit's recyclable windowNumber.
    let id: UInt64
    /// Ordered session list determines tab-strip order and keyboard navigation.
    private(set) var tabs: [TerminalTab] = []
    /// Selection stores an ID instead of an index because removals shift array positions.
    private(set) var selectedID: UInt64?
    /// Window-local labels are derived from session titles; moving never writes suffixes into names.
    private(set) var displayTitles: [UInt64: String] = [:]
    /// A toolbar-hosted view keeps the traffic lights and title-bar dragging entirely native.
    private let tabStrip = TitlebarTabStrip()
    /// Size the custom toolbar view against the window, leaving room for native chrome and add.
    private var tabStripWidth: NSLayoutConstraint?
    /// Constrain renderer content below chrome and accept dragged tabs here as native tear-off destinations.
    private let container = TabTearOffTargetView(frame: .zero)
    /// Paint slate chrome under the native transparent titlebar; renderer content remains a separate child.
    private let chromeBackground = TitlebarBackgroundView()
    /// Reentrant delegate closure is ignored once teardown has begun.
    private var isClosing = false
    /// Toolbar identifiers are scoped to this app; no native NSWindow tab groups are used.
    private static let tabsItem = NSToolbarItem.Identifier("PuzzleShell.tabs")
    private static let addItem = NSToolbarItem.Identifier("PuzzleShell.addTab")

    /// Resolve the selection defensively, including the temporary empty state during tab moves.
    var selectedTab: TerminalTab? { tabs.first { $0.id == selectedID } }

    /// Configure native chrome without SwiftUI, private APIs, or replacing standard window buttons.
    init(id: UInt64) {
        self.id = id
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1000, height: 640),
                              styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
                              backing: .buffered, defer: false)
        super.init(window: window)
        window.delegate = self
        window.isReleasedWhenClosed = false // The coordinator controls lifetime explicitly.
        window.minSize = NSSize(width: 480, height: 320)
        window.title = "PuzzleShell"
        window.titleVisibility = .hidden
        window.toolbarStyle = .unifiedCompact // Tabs sit on the same native row as the traffic lights.
        window.titlebarAppearsTransparent = true // Reveal the PSD palette without replacing any native window controls.
        window.titlebarSeparatorStyle = .none // The open-bottom tab stroke must not acquire a system divider beneath it.
        window.tabbingMode = .disallowed // Prevent an additional automatic macOS window-tab bar.
        window.contentView = chromeBackground
        chromeBackground.wantsLayer = true
        container.translatesAutoresizingMaskIntoConstraints = false
        chromeBackground.addSubview(container)
        // The full-size root extends behind chrome, but the future Rust surface must not.
        // Use AppKit's opaque layout-guide object through NSLayoutConstraint's public item API;
        // this follows toolbar/fullscreen changes without guessing a fixed titlebar height.
        NSLayoutConstraint.activate([NSLayoutConstraint.Attribute.leading, .trailing, .top, .bottom].map { edge in
            NSLayoutConstraint(item: container, attribute: edge, relatedBy: .equal,
                               toItem: window.contentLayoutGuide, attribute: edge, multiplier: 1, constant: 0)
        })
        window.center()
        // Cascade independently opened windows so their identical dimensions do not hide them.
        if let previous = NSApp.keyWindow {
            let nextTopLeft = NSPoint(x: previous.frame.minX + 24, y: previous.frame.maxY - 24)
            window.cascadeTopLeft(from: nextTopLeft)
        }
        let toolbar = NSToolbar(identifier: "PuzzleShell.titlebar")
        toolbar.delegate = self
        toolbar.displayMode = .iconOnly
        toolbar.allowsUserCustomization = false
        // A concrete width lets the toolbar fill the title bar instead of using an intrinsic
        // 600-point island. The reserved 160 points accommodate native traffic lights, margins
        // and the add button; fixed-width tab pages use the remainder without compressing tabs.
        tabStripWidth = tabStrip.widthAnchor.constraint(equalToConstant: max(TitlebarTabStrip.tabWidth, window.frame.width - 160))
        tabStripWidth?.isActive = true
        window.toolbar = toolbar
        tabStrip.onSelect = { [weak self] in self?.select($0) }
        tabStrip.onClose = { [weak self] in self?.closeTab($0) }
        tabStrip.onTearOff = { tabID in ShellCoordinator.shared.moveToNewWindow(tabID) }
        tabStrip.onRename = { [weak self] in self?.requestRename($0) }
        tabStrip.onResetName = { [weak self] tabID in self?.updateMetadata(tabID) { $0.customName = nil } }
        tabStrip.onSetColor = { [weak self] tabID, rgba in self?.updateMetadata(tabID) { $0.colorRGBA = rgba } }
        tabStrip.onDrop = { [weak self] tabID, index in
            guard let self else { return false }
            return ShellCoordinator.shared.moveTab(tabID, to: self.id, at: index)
        }
    }

    /// Storyboards are deliberately unsupported; all shell windows use the documented initializer.
    required init?(coder: NSCoder) { fatalError("Use init(id:) to construct shell windows") }

    /// Create one persistent placeholder session and announce it before mounting its surface.
    @discardableResult
    func addTab(producer: SurfaceLaunch? = nil) -> TerminalTab {
        let tabID = ShellCoordinator.shared.allocateID()
        let tab = TerminalTab(id: tabID, windowID: id, title: ShellCoordinator.shared.allocateFallbackName(), producer: producer)
        tabs.append(tab)
        tab.emitCreated()
        select(tab.id)
        return tab
    }

    /// Transfer ownership of an existing session without a second TAB_CREATED event.
    func adopt(_ tab: TerminalTab, at index: Int, selectOnArrival: Bool = true, beforeSelection: (() -> Void)? = nil) {
        tabs.insert(tab, at: index)
        beforeSelection?() // Announce the move after ownership is queryable but before selection/geometry events.
        if selectOnArrival || selectedID == nil { select(tab.id) }
        else { updateMinimumSize(); refreshTitles() } // Collection retains the active surface and keyboard responder.
    }

    /// Change ordering only; the selected surface and its first responder remain untouched.
    func reorder(_ tabID: UInt64, to index: Int) {
        guard let previous = tabs.firstIndex(where: { $0.id == tabID }), previous != index else { return }
        let tab = tabs.remove(at: previous)
        tabs.insert(tab, at: index)
        var event = ShellBridge.shared.event(PUZZLE_TAB_REORDERED, window: id, tab: tabID)
        event.code = UInt32(clamping: index) // Rust can mirror the final zero-based tab ordering.
        ShellBridge.shared.emit(event)
        refreshTitles()
    }

    /// Mount the selected view, preserving inactive views in their tab objects for future reuse.
    func select(_ tabID: UInt64) {
        guard let tab = tabs.first(where: { $0.id == tabID }) else { return }
        if selectedID != tabID {
            // Resign focus and visibility before removing the old rendering view from the window.
            if let previous = selectedTab {
                window?.makeFirstResponder(nil)
                previous.setVisible(false)
                previous.content.removeFromSuperview()
            }
            selectedID = tabID
            ShellBridge.shared.emit(ShellBridge.shared.event(PUZZLE_TAB_SELECTED, window: id, tab: tabID))
            tab.content.frame = container.bounds
            tab.content.autoresizingMask = [.width, .height]
            container.addSubview(tab.content)
            tab.content.layoutSubtreeIfNeeded() // Size the complete split tree before renderer snapshots and first-responder restoration.
            tab.emitSelection() // Restore this tab's saved leaf selection independently of native keyboard focus.
        }
        tab.setVisible(window?.isMiniaturized != true && window?.occlusionState.contains(.visible) == true)
        tab.emitGeometry()
        window?.makeFirstResponder(tab.surface)
        updateMinimumSize()
        refreshTitles()
    }

    /// Runtime updates change the automatic title while preserving any user-supplied custom name.
    @discardableResult
    func rename(_ tabID: UInt64, title: String) -> Bool {
        updateMetadata(tabID) { $0.automaticTitle = title }
    }

    /// Validate and apply metadata once, then notify Rust even when a custom name hides an update.
    @discardableResult
    func updateMetadata(_ tabID: UInt64, change: (inout TabMetadata) -> Void) -> Bool {
        guard !ShellBridge.shared.isDelivering, let tab = tabs.first(where: { $0.id == tabID }) else { return false }
        var candidate = tab.metadata
        change(&candidate)
        guard TabMetadata.validTitle(candidate.automaticTitle), TabMetadata.validTitle(candidate.customName ?? "") else { return false }
        guard candidate != tab.metadata else { return true }
        tab.metadata = candidate
        refreshTitles(forceNotification: tabID)
        return true
    }

    /// Publish an owned metadata snapshot; getters are permitted during this synchronous notification.
    func emitMetadata(for tab: TerminalTab) {
        var event = ShellBridge.shared.event(PUZZLE_TAB_METADATA_CHANGED, window: id, tab: tab.id)
        event.flags = (tab.metadata.customName != nil ? UInt64(PUZZLE_TAB_HAS_CUSTOM_NAME.rawValue) : 0)
            | (tab.metadata.colorRGBA != nil ? UInt64(PUZZLE_TAB_HAS_COLOR.rawValue) : 0)
        event.code = tab.metadata.colorRGBA ?? 0
        ShellBridge.shared.emit(event, text: tab.title)
    }

    /// Refresh indicators without renaming tabs, publishing metadata or changing the selected pane.
    func refreshStatus() {
        tabStrip.update(tabs: tabs, selected: selectedID, displayTitles: displayTitles)
    }

    /// Refresh display-only duplicate suffixes, native window titles, and all affected Rust observers.
    private func refreshTitles(forceNotification: UInt64? = nil) {
        let previous = displayTitles
        displayTitles = TabMetadata.displayTitles(tabs.map { ($0.id, $0.title) })
        window?.title = selectedID.flatMap { displayTitles[$0] }.map { "\($0) — PuzzleShell" } ?? "PuzzleShell"
        tabStrip.update(tabs: tabs, selected: selectedID, displayTitles: displayTitles)
        for tab in tabs where previous[tab.id] != displayTitles[tab.id] || tab.id == forceNotification {
            emitMetadata(for: tab)
        }
    }

    /// Remove a tab from this window while retaining it for a transfer or final close callback.
    func detachTab(_ tabID: UInt64) -> TerminalTab? {
        guard let index = tabs.firstIndex(where: { $0.id == tabID }) else { return nil }
        let tab = tabs[index]
        if selectedID == tabID {
            window?.makeFirstResponder(nil)
            tab.setVisible(false)
            tab.content.removeFromSuperview()
            selectedID = nil
        }
        tabs.remove(at: index)
        if selectedID == nil, !tabs.isEmpty { select(tabs[min(index, tabs.count - 1)].id) }
        else { refreshTitles() }
        updateMinimumSize()
        return tab
    }

    /// Deliver the final borrowed pointers before releasing the detached session object.
    func closeTab(_ tabID: UInt64) {
        guard let tab = detachTab(tabID) else { return }
        tab.emitClosing()
        if tabs.isEmpty { window?.close() }
    }

    /// Cmd-W has tab semantics; a separate Shift-Cmd-W menu closes the complete window.
    func closeSelectedTab() { if let id = selectedID { closeTab(id) } }

    /// Numbered menu navigation shares clicking's visibility and last-focused-pane restoration path.
    func selectTab(number: Int) {
        guard let index = ShellShortcuts.tabIndex(number: number, count: tabs.count) else { return }
        select(tabs[index].id)
    }

    /// Apply modulo twice so a negative step wraps correctly in Swift's remainder arithmetic.
    func cycleTab(by delta: Int) {
        guard let index = tabs.firstIndex(where: { $0.id == selectedID }), !tabs.isEmpty else { return }
        select(tabs[((index + delta) % tabs.count + tabs.count) % tabs.count].id)
    }

    /// Keep every tab's nested minimum usable when switching, resizing, or collecting split tabs.
    func updateMinimumSize() {
        guard let window else { return }
        let chromeHeight = window.frame.height - window.contentLayoutRect.height
        let minima = tabs.map { $0.layout.root.minimumSize }
        window.minSize = NSSize(width: max(480, minima.map(\.width).max() ?? 0),
                                height: max(320, (minima.map(\.height).max() ?? 0) + chromeHeight))
        // Collected tabs can require more space than their destination previously provided.
        if window.frame.width < window.minSize.width || window.frame.height < window.minSize.height {
            var frame = window.frame
            frame.size.width = max(frame.width, window.minSize.width)
            frame.size.height = max(frame.height, window.minSize.height)
            window.setFrame(frame, display: true)
        }
    }

    /// Toolbar actions arrive through Objective-C target/action dispatch.
    @objc private func newTab(_ sender: Any?) { addTab() }

    /// Fix the title-bar layout to the tab strip plus an always-accessible native add button.
    func toolbarDefaultItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] { [Self.tabsItem, Self.addItem] }
    /// Customization is disabled, but AppKit still asks for the allowed identifier set.
    func toolbarAllowedItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] { toolbarDefaultItemIdentifiers(toolbar) }

    /// Fixed-width pages keep sessions readable and reachable even in narrow windows.
    func toolbar(_ toolbar: NSToolbar, itemForItemIdentifier identifier: NSToolbarItem.Identifier, willBeInsertedIntoToolbar flag: Bool) -> NSToolbarItem? {
        let item = NSToolbarItem(itemIdentifier: identifier)
        if identifier == Self.tabsItem {
            item.label = "Terminal Tabs"
            item.view = tabStrip
            item.isBordered = false // The strip paints individual tabs instead of one large toolbar pill.
            item.visibilityPriority = .high // Collapse neither the entire strip nor its selected tab first.
        } else if identifier == Self.addItem {
            item.label = "New Tab"
            item.toolTip = "New Tab (⌘T)"
            let button = NewTabButton(frame: NSRect(x: 0, y: 0, width: 28, height: 28))
            button.image = NSImage(systemSymbolName: "plus", accessibilityDescription: "New Tab")
            button.symbolConfiguration = NSImage.SymbolConfiguration(pointSize: 14, weight: .medium)
            button.imagePosition = .imageOnly
            button.isBordered = false // The subclass paints the gradient bezel; AppKit retains the button behavior.
            button.contentTintColor = .labelColor
            button.target = self
            button.action = #selector(newTab(_:))
            button.toolTip = item.toolTip
            button.setAccessibilityLabel("New Tab")
            item.view = button
            item.isBordered = false
            item.visibilityPriority = .high // Keep creation reachable alongside pagination at narrow widths.
        } else { return nil }
        return item
    }

    /// Window closure is the single teardown path for traffic lights, menu commands and the ABI.
    func windowWillClose(_ notification: Notification) {
        guard !isClosing else { return }
        isClosing = true
        window?.makeFirstResponder(nil)
        for tab in tabs {
            tab.setVisible(false)
            tab.emitClosing()
        }
        tabs.removeAll()
        selectedID = nil
        ShellCoordinator.shared.removeWindow(id)
    }

    /// Focus events are recomputed after AppKit updates the key-window status.
    func windowDidBecomeKey(_ notification: Notification) { selectedTab?.emitFocus(); tabStrip.updateWindowAppearance() }
    /// Losing the key window must stop input even if its first responder does not change.
    func windowDidResignKey(_ notification: Notification) { selectedTab?.emitFocus(); tabStrip.updateWindowAppearance() }
    /// Occlusion includes minimization, Spaces changes and complete coverage by other windows.
    func windowDidChangeOcclusionState(_ notification: Notification) {
        selectedTab?.setVisible(window?.occlusionState.contains(.visible) == true && window?.isMiniaturized != true)
    }
    /// Keep the pager at the trailing edge while preserving each tab's fixed width on resize.
    func windowDidResize(_ notification: Notification) {
        tabStripWidth?.constant = max(TitlebarTabStrip.tabWidth, (window?.frame.width ?? 1000) - 160)
    }
    /// A display change can alter the backing scale without changing the window's point size.
    func windowDidChangeBackingProperties(_ notification: Notification) { selectedTab?.updateGeometry() }
    /// A move between equal-scale displays still changes Puzzle's display-link association.
    func windowDidChangeScreen(_ notification: Notification) { selectedTab?.updateGeometry() }
}
