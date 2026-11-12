// M18.5: copy native user gestures into puzzle.h; no OSC parsing, filesystem I/O or permission tokens here.
import AppKit
#if PUZZLE_ENABLED

/// Drop callbacks update only the creating pane's native registration and acceptance cache.
extension PuzzleRuntime {
    /// Called after tick, with owned payloads; stale surface epochs have already been rejected.
    func applyDropEvent(_ name: String, value: Any?, pane: TerminalSurfaceView?) {
        guard let pane, let map = value as? [String: Any], let text = map["generation"] as? String,
              let generation = UInt64(text), generation != 0 else { return }
        if name == "drop/registration" {
            if let names = map["mimes"] as? [String] {
                guard pane.dropTarget.map({ $0.generation < generation }) ?? true else { return } // Replay cannot reset a live hover.
                pane.dropTarget = PuzzleDropTarget(generation: generation, names: names.reduce(into: [String]()) { if !$0.contains($1) { $0.append($1) } })
                pane.registerForDraggedTypes(TerminalDropPasteboard.types(for: names))
            } else if map["mimes"] is NSNull, pane.dropTarget?.generation == generation {
                pane.resetDropDestination()
            }
        } else {
            guard let target = pane.dropTarget, target.generation == generation else { return }
            if name == "drop/concluded" { target.clear() }
            else if let operation = map["operation"] as? UInt32, let names = map["mimes"] as? [String] {
                target.accept(operation: operation, preferred: names)
            }
        }
    }

    /// C copies all MIME ranges before return; points include padding, with no DPR arithmetic in the host.
    @discardableResult
    func nativeDrop(paneID: UInt64, generation: UInt64, kind: UInt32, point: NSPoint,
                    operations: UInt32, items: [TerminalDropItem]) -> Bool {
        guard let surface = surfaces[paneID] else { return false }
        let result = PuzzleClipboardBuffers.withItems(items.map { PuzzleClipboardItem(mime: $0.mime, data: $0.data) }) {
            puzzle_surface_drop(surface, generation, kind, point.x, point.y, operations, $0.baseAddress, $0.count)
        }
        // A closed surface is a cancelled OS gesture, not a modal error inside AppKit's drag loop.
        return result == PUZZLE_OK
    }

    /// Unregistered file paths or user-dragged reply text use ordinary sanitized/bracketed paste.
    func pasteDroppedPaths(paneID: UInt64, text: String) -> Bool {
        guard let surface = surfaces[paneID] else { return false }
        return Self.withBytes(text) { puzzle_surface_paste(surface, $0) } == PUZZLE_OK
    }
}
#endif
