import AppKit

/// A title-bar tab owns a pane tree, metadata and every persistent future terminal surface in that tree.
/// Moving the tab moves the whole tree; neither pane IDs nor Metal layers are recreated.
@MainActor
final class TerminalTab {
    /// Stable identity for title-bar order, naming and window ownership.
    let id: UInt64
    /// Metadata belongs to the tab, independently of which pane currently has keyboard focus.
    var metadata: TabMetadata
    /// Mirror Puzzle's per-pane OSC 7501 records; the tab retains them during window transfers.
    var extensionStatus = TabStatus()
    /// Naming precedence remains the responsibility of the existing metadata model.
    var title: String { metadata.effectiveTitle }
    /// Window ownership is mirrored into every leaf when the entire tab transfers.
    var windowID: UInt64 {
        didSet { for pane in panes.values { pane.windowID = windowID } }
    }
    /// The value model stores branch proportions, visual traversal order and focused-pane identity.
    private(set) var layout: PaneLayout
    /// Stable rendering views are retained separately from replaceable native split/host nodes.
    private(set) var panes: [UInt64: TerminalSurfaceView]
    /// This persistent root is the only content view mounted/unmounted by the window controller.
    let content = PaneContainerView()
    /// Current host views paint focus outside each renderer-owned layer.
    private var hosts: [UInt64: PaneHostView] = [:]
    /// Current native dividers are addressable by the same branch IDs stored in the model.
    private var splits: [UInt64: PaneSplitView] = [:]
    /// Legacy tab-scoped commands address the currently focused pane; new integrations should use pane IDs.
    var surface: TerminalSurfaceView { panes[layout.activeID]! }
    /// Visual order is deterministic even though the backing registry is a dictionary.
    var orderedSurfaces: [TerminalSurfaceView] { layout.root.paneIDs.compactMap { panes[$0] } }

    /// Construct one placeholder pane without any Rust, renderer, parser or PTY dependency.
    init(id: UInt64, windowID: UInt64, title: String, producer: SurfaceLaunch? = nil) {
        self.id = id
        self.windowID = windowID
        metadata = TabMetadata(fallbackName: title)
        let paneID = ShellCoordinator.shared.allocateID() // Windows, tabs, panes and divider branches share a non-reused namespace.
        layout = PaneLayout(initialPaneID: paneID)
        panes = [paneID: TerminalSurfaceView(tabID: id, windowID: windowID, paneID: paneID, producer: producer)]
        rebuild()
    }

    /// Announce tab identity first, then every owned pane before any focus/input can reach Rust.
    func emitCreated() {
        var event = surface.surfaceEvent(PUZZLE_TAB_CREATED) // Initial pointers are informational; PANE_CREATED owns surface lifecycle in ABI v2.
        event.code = UInt32(clamping: ShellCoordinator.shared.owner(of: id)?.tabs.firstIndex(where: { $0.id == id }) ?? 0)
        ShellBridge.shared.emit(event, text: title)
        for pane in orderedSurfaces { pane.emitCreated(title: title) }
        emitLayout()
    }

    /// Pane teardown is the final safe point for each borrowed view/layer, followed by structural tab closure.
    func emitClosing() {
        for pane in orderedSurfaces { pane.emitClosing() }
        // The controller detaches this tab before closure, so cleanup cannot rely on global ownership lookup.
        extensionStatus = TabStatus()
        ShellBridge.shared.emit(ShellBridge.shared.event(PUZZLE_TAB_CLOSING, window: windowID, tab: id))
    }

    /// Visibility applies to ALL leaves of a selected tab; only one leaf receives keyboard focus.
    func setVisible(_ visible: Bool) { for pane in orderedSurfaces { pane.setSessionVisible(visible) } }
    /// Geometry changes affect every renderer surface, including nested leaves.
    func emitGeometry() { for pane in orderedSurfaces { pane.emitGeometry() } }
    /// Backing-scale changes are propagated even if a nested pane's point dimensions do not change.
    func updateGeometry() { for pane in orderedSurfaces { pane.updateGeometry() } }
    /// Key-window changes update the focused leaf and the surrounding native focus outline.
    func emitFocus() { for pane in orderedSurfaces { pane.emitFocus() }; updateHighlights() }

    /// Refuse unusably small splits rather than silently collapsing an existing pane.
    func canSplit(_ direction: PaneDirection) -> Bool {
        let size = surface.superview?.bounds.size ?? .zero // The host includes the small focus inset.
        return content.window != nil && (direction.sideBySide
            ? size.width >= PaneSizing.leafMinimum.width * 2 + PaneSizing.divider
            : size.height >= PaneSizing.leafMinimum.height * 2 + PaneSizing.divider)
    }

    /// Insert beside the focused pane, preserving every existing view and saved neighboring divider ratio.
    @discardableResult
    func split(_ direction: PaneDirection) -> UInt64 {
        content.layoutSubtreeIfNeeded() // Ensure the size guard sees the real current leaf after tab mounting or resize.
        guard canSplit(direction), !ShellBridge.shared.isDelivering else { return 0 }
        let paneID = ShellCoordinator.shared.allocateID()
        let branchID = ShellCoordinator.shared.allocateID()
        guard layout.split(direction: direction, paneID: paneID, branchID: branchID) else { return 0 }
        let pane = TerminalSurfaceView(tabID: id, windowID: windowID, paneID: paneID)
        panes[paneID] = pane // Make the new identity queryable before PANE_CREATED is delivered.
        pane.emitCreated(title: title)
        rebuild()
        emitLayout()
        emitSelection()
        pane.setSessionVisible(surface.window?.occlusionState.contains(.visible) == true && surface.window?.isMiniaturized != true)
        content.window?.makeFirstResponder(pane)
        ShellCoordinator.shared.owner(of: id)?.updateMinimumSize()
        return paneID
    }

    /// Close a leaf and promote its sibling; closing the only leaf uses the complete tab-close lifecycle.
    @discardableResult
    func closePane(_ paneID: UInt64) -> Bool {
        guard let pane = panes[paneID], !ShellBridge.shared.isDelivering else { return false }
        if panes.count == 1 {
            ShellCoordinator.shared.owner(of: id)?.closeTab(id)
            return true
        }
        if content.window?.firstResponder === pane { content.window?.makeFirstResponder(nil) }
        guard layout.close(paneID) else { return false }
        pane.setSessionVisible(false)
        pane.emitClosing() // Rust releases GPU resources synchronously while this local reference retains the native view/layer.
        pane.removeFromSuperview()
        panes.removeValue(forKey: paneID)
        extensionStatus.removePane(paneID) // Done/error survive prompts, but never survive destruction of their pane.
        rebuild()
        emitLayout()
        emitSelection()
        if content.window != nil { content.window?.makeFirstResponder(surface) }
        ShellCoordinator.shared.owner(of: id)?.updateMinimumSize()
        ShellCoordinator.shared.owner(of: id)?.refreshStatus()
        return true
    }

    /// Native clicks and Rust commands share focused-pane state; selection never destroys a session.
    @discardableResult
    func selectPane(_ paneID: UInt64, focus: Bool = true) -> Bool {
        let previous = layout.activeID
        guard layout.select(paneID) else { return false }
        updateHighlights()
        if previous != paneID { emitSelection() }
        if focus { content.window?.makeFirstResponder(surface) }
        return true
    }

    /// Cycle visually left-to-right/top-to-bottom, wrapping just like the title-bar tab command.
    func cyclePane(by delta: Int) {
        layout.cycle(by: delta)
        updateHighlights()
        emitSelection()
        content.window?.makeFirstResponder(surface)
    }

    /// Apply a preferred proportion from Rust, subject to the same minimum-size policy as native dragging.
    @discardableResult
    func resizeSplit(_ branchID: UInt64, fraction: Double) -> Bool {
        guard layout.resize(branchID: branchID, fraction: fraction) else { return false }
        splits[branchID]?.setFraction(fraction)
        emitLayout()
        return true
    }

    /// Publish topology changes as an invalidation; callback-safe getters return the complete tree snapshot.
    func emitLayout() { ShellBridge.shared.emit(ShellBridge.shared.event(PUZZLE_PANE_LAYOUT_CHANGED, window: windowID, tab: id)) }
    /// Selection has pane identity even when the window is backgrounded and keyboard focus is false.
    func emitSelection() { ShellBridge.shared.emit(ShellBridge.shared.event(PUZZLE_PANE_SELECTED, window: windowID, tab: id, pane: layout.activeID)) }

    /// Regenerate only cheap layout nodes; terminal surfaces, IME state and GPU handles remain persistent.
    private func rebuild() {
        let window = content.window
        window?.makeFirstResponder(nil) // Avoid leaving a first responder inside a host being replaced.
        for pane in panes.values { pane.removeFromSuperview() }
        hosts.removeAll()
        splits.removeAll()
        content.install(build(layout.root))
        content.layoutSubtreeIfNeeded()
        updateHighlights()
        // Callers restore first responder after delivering layout/selection lifecycle notifications.
    }

    /// Recursively translate the pure tree into AppKit's native draggable split views.
    private func build(_ node: PaneNode) -> NSView {
        switch node {
        case .leaf(let paneID):
            let pane = panes[paneID]!
            pane.onFocus = { [weak self] in _ = self?.selectPane(paneID, focus: false) } // Click focus updates the next split target.
            let host = PaneHostView(surface: pane)
            hosts[paneID] = host
            return host
        case .split(let id, let axis, let fraction, let first, let second):
            let split = PaneSplitView(id: id, sideBySide: axis, fraction: fraction,
                                      first: build(first), second: build(second),
                                      firstMinimum: first.minimumSize, secondMinimum: second.minimumSize)
            split.onResize = { [weak self] branchID, proportion in
                guard let self, self.layout.resize(branchID: branchID, fraction: proportion) else { return }
                self.emitLayout() // Leaf layout already delivers pixel resize events to each renderer.
            }
            splits[id] = split
            return split
        }
    }

    /// A focus outline is useful only when there is more than one pane and this window accepts input.
    private func updateHighlights() {
        for (id, host) in hosts {
            host.highlighted = panes.count > 1 && id == layout.activeID && content.window?.isKeyWindow == true
        }
    }
}
