// M18.5: NSDraggingDestination adapts genuine AppKit gestures to Puzzle's asynchronous native contract.
import AppKit

/// Terminal content accepts file-path fallback or program-registered native MIME drops, independently of tab drags.
extension TerminalSurfaceView {
    /// Keep file drops available after unregister/reset; internal tab drags remain the ancestor's responsibility.
    func resetDropDestination() {
        #if PUZZLE_ENABLED
        dropTarget?.clear()
        dropTarget = nil
        #endif
        unregisterDraggedTypes()
        registerForDraggedTypes([.fileURL, .string])
    }

    /// External sources retain the established policy; only a matching active native reply source may cross panes.
    /// A private title-bar drag or forged provenance/pasteboard type never authenticates an in-process source.
    private func validDrop(_ sender: NSDraggingInfo) -> Bool {
        guard isLive, sender.draggingDestinationWindow === window, window != nil else { return false }
        if sender.draggingSource == nil { return true }
        #if PUZZLE_ENABLED
        return (sender.draggingSource as? SurfaceDragSource)?.allowsDrop(sender, into: self) == true
        #else
        return false
        #endif
    }

    /// AppKit's native move bit differs from Puzzle's: translate permitted operations explicitly.
    private func dropOperations(_ sender: NSDraggingInfo) -> UInt32 {
        (sender.draggingSourceOperationMask.contains(.copy) ? 1 : 0) |
            (sender.draggingSourceOperationMask.contains(.move) ? 2 : 0)
    }

    /// Hover reads type metadata only. AppKit receives rejection until the program answers through tick.
    private func updateDrop(_ sender: NSDraggingInfo) -> NSDragOperation {
        guard validDrop(sender) else { leaveDrop(); return [] }
        let board = TerminalDropPasteboard(board: sender.draggingPasteboard)
        #if PUZZLE_ENABLED
        if let target = dropTarget {
            let names = board.offer(target.names)
            let point = convert(sender.draggingLocation, from: nil)
            let operations = dropOperations(sender)
            guard !names.isEmpty, operations != 0 else { leaveDrop(); return [] }
            if target.update(sequence: sender.draggingSequenceNumber, revision: board.board.changeCount,
                             offered: names, point: point, operations: operations) {
                let items = names.map { TerminalDropItem(mime: $0, data: Data()) }
                guard (PuzzleGlue.shared.runtime as? PuzzleRuntime)?.nativeDrop(paneID: paneID, generation: target.generation,
                    kind: 0, point: point, operations: operations, items: items) == true else { target.clear(); return [] }
            }
            return target.operation
        }
        // Ordinary file-path fallback is a native user paste, never an OSC 72 data grant.
        let fallback = board.board.types?.contains(.fileURL) == true || sender.draggingSource is SurfaceDragSource
        return fallback && sender.draggingSourceOperationMask.contains(.copy) ? .copy : []
        #else
        return [] // Placeholder builds cannot paste into a terminal Session.
        #endif
    }

    /// First entry establishes a fresh native sequence; enter and move share kind 0 in puzzle.h.
    override func draggingEntered(_ sender: NSDraggingInfo) -> NSDragOperation { updateDrop(sender) }
    /// Native periodic updates expose asynchronous copy/move/reject feedback without moving the mouse.
    override func draggingUpdated(_ sender: NSDraggingInfo) -> NSDragOperation { updateDrop(sender) }
    /// Only active native drags need periodic callbacks; no application polling timer is installed.
    override func wantsPeriodicDraggingUpdates() -> Bool { true }
    /// The final native gate still requires a program-approved operation for registered destinations.
    override func prepareForDragOperation(_ sender: NSDraggingInfo) -> Bool { !updateDrop(sender).isEmpty }

    /// Capture accepted data at the actual release, checking native ownership before and after the reads.
    override func performDragOperation(_ sender: NSDraggingInfo) -> Bool {
        guard validDrop(sender) else { return false }
        #if PUZZLE_ENABLED
        guard let runtime = PuzzleGlue.shared.runtime as? PuzzleRuntime else { return false }
        let board = TerminalDropPasteboard(board: sender.draggingPasteboard)
        if let target = dropTarget {
            guard target.sequence == sender.draggingSequenceNumber, !target.operation.isEmpty,
                  sender.draggingSourceOperationMask.contains(target.operation),
                  let items = board.capture(target.captureNames, revision: target.revision) else { leaveDrop(); return false }
            let point = convert(sender.draggingLocation, from: nil)
            let accepted = runtime.nativeDrop(paneID: paneID, generation: target.generation, kind: 2,
                point: point, operations: target.operation == .move ? 2 : 1, items: items)
            target.clear() // Rust now owns the immutable bytes; native drag objects need not survive.
            return accepted
        }
        guard sender.draggingSourceOperationMask.contains(.copy) else { return false }
        let text: String?
        if sender.draggingSource is SurfaceDragSource {
            let revision = board.board.changeCount
            let value = board.board.string(forType: .string)
            text = board.board.changeCount == revision && validDrop(sender) ? value : nil
        } else { text = board.quotedPaths() }
        guard let text, text.utf8.count <= TerminalDropPasteboard.maximumBytes else { return false }
        return runtime.pasteDroppedPaths(paneID: paneID, text: text)
        #else
        return false
        #endif
    }

    /// Cancellation/exit releases hover state and sends no path paste, even for a rejected registered target.
    private func leaveDrop() {
        #if PUZZLE_ENABLED
        guard let target = dropTarget, target.sequence != nil else { return }
        _ = (PuzzleGlue.shared.runtime as? PuzzleRuntime)?.nativeDrop(paneID: paneID, generation: target.generation,
            kind: 1, point: .zero, operations: 0, items: [])
        target.clear()
        #endif
    }
    /// Leaving without a drop is an explicit protocol observation, independent of eventual source completion.
    override func draggingExited(_ sender: NSDraggingInfo?) { leaveDrop() }
    /// A rejected/cancelled drag can end without an exit; a completed drop has already cleared its sequence.
    override func draggingEnded(_ sender: NSDraggingInfo) { leaveDrop() }
}
