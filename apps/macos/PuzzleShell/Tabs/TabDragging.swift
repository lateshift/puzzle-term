import AppKit

/// A native button recognizes deliberate drags while preserving click and accessibility activation.
@MainActor
final class TabDragButton: NSButton {
    /// The strip supplies the full-tab drag preview after the pointer crosses the movement threshold.
    var onDrag: ((NSEvent) -> Void)?
    /// Option-click uses the same persistent-session tear-off path as a drag released over content or desktop.
    var onOptionClick: (() -> Void)?

    /// Keep modifier-sensitive click policy separate from mouse tracking so it can be verified directly.
    func activateClick(modifiers: NSEvent.ModifierFlags) {
        if modifiers.contains(.option) { onOptionClick?() }
        else { performClick(nil) } // Native target/action and accessibility activation still select normally.
    }

    /// Track only mouse-up/drag events; a small jitter remains an ordinary tab-selection click.
    override func mouseDown(with event: NSEvent) {
        guard let window else { return }
        let start = event.locationInWindow
        // This is AppKit's normal mouse-tracking loop, not another application event loop.
        while let next = window.nextEvent(matching: [.leftMouseDragged, .leftMouseUp], until: .distantFuture, inMode: .eventTracking, dequeue: true) {
            if next.type == .leftMouseUp {
                if bounds.contains(convert(next.locationInWindow, from: nil)) { activateClick(modifiers: event.modifierFlags) }
                return
            }
            if hypot(next.locationInWindow.x - start.x, next.locationInWindow.y - start.y) >= 5 {
                onDrag?(next)
                return // NSDraggingSession takes over tracking, cancellation, and cross-window movement.
            }
        }
    }
}

/// A lightweight insertion ghost makes the reserved full-width slot visible without a heavy highlight.
@MainActor
final class TabDropSlot: NSView {
    /// Render with dynamic system colors so the preview fits both light and dark title-bar materials.
    override func draw(_ dirtyRect: NSRect) {
        guard let context = NSGraphicsContext.current?.cgContext else { return }
        // Reuse the top-rounded silhouette so the insertion ghost fits the new content-aligned tabs.
        let outline = TabChrome.tabPath(in: bounds.insetBy(dx: 1, dy: 1), closed: true)
        context.saveGState() // Restore the dash/line width before AppKit paints other controls.
        context.addPath(outline)
        context.setFillColor(NSColor.controlAccentColor.withAlphaComponent(0.045).cgColor)
        context.fillPath()
        context.addPath(outline) // Filling consumes the current path; add it again for the dashed outline.
        context.setStrokeColor(NSColor.controlAccentColor.withAlphaComponent(0.35).cgColor)
        context.setLineWidth(1)
        context.setLineDash(phase: 0, lengths: [3, 3])
        context.strokePath()
        context.restoreGState()
    }
    /// The ghost is decorative; pointer events continue to the containing strip.
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
}

/// Manage native drag lifetime and page-hover timing without mutating terminal ownership during hover.
@MainActor
final class TabDragInteraction: NSObject, NSDraggingSource {
    /// This private type carries only a process-local session identity, never terminal content.
    static let pasteboardType = NSPasteboard.PasteboardType("dev.puzzleshell.terminal-tab")
    /// Reject payloads copied from earlier app runs, other processes, or the general clipboard.
    private static let nonce = UUID().uuidString
    /// Weak ownership avoids a cycle with the toolbar view that retains this interaction helper.
    private weak var strip: TitlebarTabStrip?
    /// Keep the current source identity until AppKit reports success or cancellation.
    private(set) var activeID: UInt64?
    /// Hovering a pager arrow advances through otherwise inaccessible destination pages.
    private var pagingTimer: Timer?
    /// The latest drag position remains usable when the pointer is stationary over a pager arrow.
    private var hoverPoint = NSPoint.zero
    /// Destination identity is retained only for computing the hover gap, not for transferring ownership.
    private var hoverID: UInt64?
    /// Changing edge direction resets the dwell time instead of immediately paging backwards.
    private var pageDirection = 0
    /// Distinguish Escape cancellation from an ordinary release over content or outside a registered destination.
    private var wasCancelled = false
    /// Mouse release is tracked because AppKit reports both rejected drops and cancellation as .none.
    private var mouseReleased = false
    /// Remove the local monitor at session end; it observes only this app's native drag events.
    private var cancellationMonitor: Any?
    /// The session is owned by AppKit; this weak reference only adjusts its return animation policy.
    private weak var nativeSession: NSDraggingSession?

    /// Associate one interaction helper with one native toolbar strip.
    init(strip: TitlebarTabStrip) { self.strip = strip }

    /// Start an AppKit-managed drag with a full-tab image and a local-only identity payload.
    func begin(id: UInt64, image: NSImage, frame: NSRect, event: NSEvent) {
        guard let strip, ShellCoordinator.shared.owner(of: id) != nil else { return }
        activeID = id
        wasCancelled = false
        mouseReleased = false
        cancellationMonitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .leftMouseUp]) { [weak self] event in
            // AppKit invokes local event monitors on its main thread, including drag tracking.
            MainActor.assumeIsolated {
                if event.type == .keyDown && event.keyCode == 53 {
                    self?.wasCancelled = true
                    self?.nativeSession?.animatesToStartingPositionsOnCancelOrFail = true
                }
                if event.type == .leftMouseUp { self?.mouseReleased = true }
            }
            return event // Never consume Escape: AppKit still cancels its native session.
        }
        let payload = NSPasteboardItem()
        payload.setString("\(Self.nonce):\(id)", forType: Self.pasteboardType)
        let item = NSDraggingItem(pasteboardWriter: payload)
        item.setDraggingFrame(frame, contents: image)
        strip.setDraggedTab(id)
        let session = strip.beginDraggingSession(with: [item], event: event, source: self)
        nativeSession = session
        session.animatesToStartingPositionsOnCancelOrFail = true // Escape/rejected title-bar drops return the lifted tab home.
    }

    /// Advertise move-only behavior inside this application; copying a terminal session is undefined.
    func draggingSession(_ session: NSDraggingSession, sourceOperationMaskFor context: NSDraggingContext) -> NSDragOperation {
        context == .withinApplication ? .move : []
    }
    /// Option/Command cannot silently turn a session move into a copy or link operation.
    func ignoreModifierKeys(for session: NSDraggingSession) -> Bool { true }

    /// Disable the return-to-source flight over a valid tear-off area: the new window appears there.
    func draggingSession(_ session: NSDraggingSession, movedTo screenPoint: NSPoint) {
        session.animatesToStartingPositionsOnCancelOrFail = wasCancelled || !Self.isTearOffArea(screenPoint)
    }

    /// Snapshot native front-to-back geometry, protecting title bars while accepting terminal content.
    /// A maximized source leaves plenty of valid space below its strip; desktop visibility is irrelevant.
    static func isTearOffArea(_ point: NSPoint) -> Bool {
        // Native ordering also includes unrelated panels; identity filtering keeps geometry limited to owned shell windows.
        let owned = Set(ShellCoordinator.shared.windows.values.compactMap { $0.window }.map(ObjectIdentifier.init))
        // Front-to-back snapshots let a visible content area win over another window's covered title bar.
        let regions = NSApp.orderedWindows.compactMap { window -> TabTearOffPolicy.WindowRegion? in
            guard owned.contains(ObjectIdentifier(window)), window.isVisible, !window.isMiniaturized else { return nil }
            // The native content layout rect is window-local; the drag source supplies global screen coordinates.
            return TabTearOffPolicy.WindowRegion(frame: window.frame, contentFrame: window.convertToScreen(window.contentLayoutRect))
        }
        return TabTearOffPolicy.isTearOffArea(point, windows: regions)
    }

    /// Complete a native drag once: a successful local drop is already committed by its destination.
    /// An unhandled release over content/desktop creates a window; accepted drops and cancellation do not.
    @discardableResult
    static func finish(id: UInt64, at point: NSPoint, operation: NSDragOperation, cancelled: Bool, released: Bool) -> UInt64? {
        guard TabTearOffPolicy.allowsFallback(operation: operation, cancelled: cancelled, released: released), isTearOffArea(point),
              !ShellBridge.shared.isDelivering else { return nil }
        return ShellCoordinator.shared.moveToNewWindow(id, at: point)?.id
    }

    /// Restore source state, remove observers, and optionally tear off the same persistent session.
    func draggingSession(_ session: NSDraggingSession, endedAt screenPoint: NSPoint, operation: NSDragOperation) {
        let id = activeID
        // Some native drag loops consume mouse-up internally; the physical button state is a fallback.
        let released = mouseReleased || (NSEvent.pressedMouseButtons & 1) == 0
        if let monitor = cancellationMonitor { NSEvent.removeMonitor(monitor) }
        cancellationMonitor = nil
        nativeSession = nil
        activeID = nil
        leave()
        strip?.setDraggedTab(nil)
        if let id { Self.finish(id: id, at: screenPoint, operation: operation, cancelled: wasCancelled, released: released) }
    }

    /// Accept only a currently live source in this process, with a matching native source object.
    /// Shared by tab-strip insertion and content tear-off, so both reject stale/foreign/forged payloads identically.
    static func validatedID(_ sender: NSDraggingInfo) -> UInt64? {
        guard !ShellBridge.shared.isDelivering,
              let source = sender.draggingSource as? TabDragInteraction, let id = source.activeID,
              sender.draggingPasteboard.string(forType: Self.pasteboardType) == "\(Self.nonce):\(id)",
              sender.draggingSourceOperationMask.contains(.move), ShellCoordinator.shared.owner(of: id) != nil else { return nil }
        return id
    }

    /// Animate an insertion boundary while hovering; ownership changes only in performDrop.
    func update(_ sender: NSDraggingInfo) -> NSDragOperation {
        guard let strip, strip.onDrop != nil, let id = Self.validatedID(sender) else { leave(); return [] }
        hoverPoint = strip.convert(sender.draggingLocation, from: nil)
        hoverID = id
        strip.previewDrop(tabID: id, index: strip.dropIndex(at: hoverPoint, tabID: id))
        // Pager hit zones are independent of the moving document; use deliberate 550-ms dwell.
        let direction = strip.dragPageDirection(at: hoverPoint)
        if direction != pageDirection {
            pagingTimer?.invalidate()
            pagingTimer = nil
            pageDirection = direction
            if direction != 0 {
                let timer = Timer(timeInterval: 0.55, target: self, selector: #selector(advancePage), userInfo: nil, repeats: true)
                RunLoop.main.add(timer, forMode: .common) // Native drag tracking uses common/event-tracking modes.
                pagingTimer = timer
            }
        }
        return .move
    }

    /// Auto-page while a tab is held over a chevron, then reposition its gap on the revealed strip.
    @objc private func advancePage() {
        guard let strip, let id = hoverID, ShellCoordinator.shared.owner(of: id) != nil else { leave(); return }
        strip.page(by: pageDirection)
        strip.previewDrop(tabID: id, index: strip.dropIndex(at: hoverPoint, tabID: id))
    }

    /// Revalidate at mouse-up to reject a tab/window that disappeared during a drag.
    func performDrop(_ sender: NSDraggingInfo) -> Bool {
        guard let strip, let id = Self.validatedID(sender), let commit = strip.onDrop else { leave(); return false }
        let point = strip.convert(sender.draggingLocation, from: nil)
        let index = strip.dropIndex(at: point, tabID: id)
        leave(restore: false) // Preserve neighbor positions so the committed tab settles into the open gap.
        let success = commit(id, index)
        strip.clearDropPreview() // Restore correctly even if the owning window rejected the move.
        return success
    }

    /// Stop hover timers and restore the destination when leaving, cancelling, or completing a drop.
    func leave(restore: Bool = true) {
        pagingTimer?.invalidate()
        pagingTimer = nil
        hoverID = nil
        pageDirection = 0
        strip?.clearDropPreview(restore: restore)
    }
}

/// NSView destination callbacks forward to the helper; AppKit owns the actual drag tracking loop.
extension TitlebarTabStrip {
    /// Start previewing only after the payload's live, local source has been validated.
    override func draggingEntered(_ sender: NSDraggingInfo) -> NSDragOperation { dragInteraction.update(sender) }
    /// Recompute insertion while moving through the title bar, with stable animated target frames.
    override func draggingUpdated(_ sender: NSDraggingInfo) -> NSDragOperation { dragInteraction.update(sender) }
    /// Restore spacing when the pointer leaves this window's title-bar strip.
    override func draggingExited(_ sender: NSDraggingInfo?) { dragInteraction.leave() }
    /// Periodic updates support stationary pager-hover dwell without requiring pointer movement.
    override func wantsPeriodicDraggingUpdates() -> Bool { true }
    /// Commit on drop through the same transactional move path exposed to Rust.
    override func performDragOperation(_ sender: NSDraggingInfo) -> Bool { dragInteraction.performDrop(sender) }
    /// AppKit calls this after a successful drop; clear any remaining decorative preview state.
    override func concludeDragOperation(_ sender: NSDraggingInfo?) { dragInteraction.leave() }
}
