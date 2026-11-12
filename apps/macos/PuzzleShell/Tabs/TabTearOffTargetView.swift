import AppKit

/// The terminal-content container accepts only PuzzleShell's authenticated local tab payload.
/// Registering a real destination gives native move feedback instead of a forbidden-drop cursor over content.
@MainActor
final class TabTearOffTargetView: NSView {
    /// Install before any pane is mounted; descendant renderer views keep their existing input behavior.
    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        registerForDraggedTypes([TabDragInteraction.pasteboardType])
    }

    /// This content container is created in code, with no storyboard restoration path.
    required init?(coder: NSCoder) { fatalError("Use init(frame:) for tear-off destinations") }

    /// Validate source identity and the deliberate detachment margin before advertising a native move.
    private func accepts(_ sender: NSDraggingInfo) -> Bool {
        guard TabDragInteraction.validatedID(sender) != nil, let window else { return false }
        let point = window.convertPoint(toScreen: sender.draggingLocation) // Destination locations are window-local; policy uses global screen points.
        return TabDragInteraction.isTearOffArea(point)
    }

    /// A tab entering terminal content offers tear-off after crossing below the protected title-bar margin.
    override func draggingEntered(_ sender: NSDraggingInfo) -> NSDragOperation { accepts(sender) ? .move : [] }
    /// Recompute acceptance while moving back toward the strip, including in a maximized window.
    override func draggingUpdated(_ sender: NSDraggingInfo) -> NSDragOperation { accepts(sender) ? .move : [] }
    /// Reject a source that closed or a location that changed between hover and release.
    override func prepareForDragOperation(_ sender: NSDraggingInfo) -> Bool { accepts(sender) }

    /// Commit one identity-preserving transfer; the source subsequently receives .move and skips its fallback.
    override func performDragOperation(_ sender: NSDraggingInfo) -> Bool {
        guard accepts(sender), let id = TabDragInteraction.validatedID(sender), let window else { return false }
        let point = window.convertPoint(toScreen: sender.draggingLocation) // Use the same screen-space anchor as the drag source's completion fallback.
        return ShellCoordinator.shared.moveToNewWindow(id, at: point) != nil
    }
}
