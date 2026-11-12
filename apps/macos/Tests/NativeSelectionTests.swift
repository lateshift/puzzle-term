// Native Cocoa gestures traverse the app adapter and puzzle.h; only Puzzle resolves text boundaries (M12.selection).
import AppKit
#if PUZZLE_ENABLED

/// Compare public selection ranges with the harness's nearest-edge and line-end rules, without clipboard access.
@MainActor
enum NativeSelectionTests {
    /// Existing transport observation provides a parser barrier; this fixture temporarily observes selection facts too.
    static func run(_ runtime: PuzzleRuntime, pane: TerminalSurfaceView, input: () -> Data, reset: () -> Void) {
        let oldObserver = runtime.eventObserver
        var selected: [UInt64] = []
        runtime.eventObserver = { paneID, name, bytes in
            oldObserver?(paneID, name, bytes)
            guard paneID == pane.paneID, name == "terminal/selectionChanged" else { return }
            selected = range(bytes)
        }
        defer { runtime.eventObserver = oldObserver }
        let surface = runtime.surfaces[pane.paneID]!
        for padding in [4, 0] {
            configure(surface, "{\"render\":{\"padding\":\(padding)},\"selection\":{\"lineEndSnap\":true}}")
            // Start/end use boundaries, not inclusive hit cells; both drag directions must select the same characters.
            for (a, b, expected): (NSPoint, NSPoint, [UInt64]) in [
                (NSPoint(x: 0.1, y: 0), NSPoint(x: 0.75, y: 0), [0, 0, 0]),
                (NSPoint(x: 0.75, y: 0), NSPoint(x: 0.1, y: 0), [0, 0, 0]),
                (NSPoint(x: 0.75, y: 0), NSPoint(x: 2.25, y: 0), [1, 1, 0]),
                (NSPoint(x: 2.75, y: 0), NSPoint(x: 1.25, y: 0), [1, 2, 0]),
                (NSPoint(x: 6.2, y: 0), NSPoint(x: 1.8, y: 1), [3, 1, 1]),
                (NSPoint(x: 1.8, y: 1), NSPoint(x: 6.2, y: 0), [3, 1, 1])
            ] {
                prepare(surface, pane: pane, text: "abc\r\nXYZ", padding: padding, input: input, reset: reset)
                selected = []
                drag(pane, from: a, to: b)
                PuzzleRuntimeTests.until("Cocoa selection \(a)→\(b) at padding \(padding): expected \(expected), got \(selected)") { selected == expected }
            }
            prepare(surface, pane: pane, text: "a界b", padding: padding, input: input, reset: reset)
            selected = []
            drag(pane, from: NSPoint(x: 0.75, y: 0), to: NSPoint(x: 3.25, y: 0))
            PuzzleRuntimeTests.until("Native wide-character selection stays atomic") { selected == [1, 2, 0] }
        }
        // This public setting is core-owned; toggling it must not require host pointer-offset changes.
        configure(surface, "{\"selection\":{\"lineEndSnap\":false}}")
        prepare(surface, pane: pane, text: "abc\r\nXYZ", padding: 0, input: input, reset: reset)
        selected = []
        drag(pane, from: NSPoint(x: 6.2, y: 0), to: NSPoint(x: 1.8, y: 1))
        PuzzleRuntimeTests.until("Live snap opt-out preserves the clicked trailing-blank boundary") { selected == [6, 1, 1] }
        configure(surface, "{\"selection\":{\"lineEndSnap\":true}}")
        print("PASS: native Cocoa→puzzle.h selection matches harness boundaries: first-letter midpoint, both directions, trailing-blank snap, wide text, padding 4/0 and live snap opt-out")
    }

    /// A core fact carries inclusive columns and absolute line IDs; compare row distance rather than private identities.
    private static func range(_ bytes: Data) -> [UInt64] {
        guard let value = try? JSONSerialization.jsonObject(with: bytes) as? [String: Any],
              let range = value["range"] as? [String: Any],
              let start = range["start"] as? [String: Any], let end = range["end"] as? [String: Any],
              let first = start["col"] as? NSNumber, let last = end["col"] as? NSNumber,
              let a = start["lineId"] as? NSNumber, let b = end["lineId"] as? NSNumber else { return [] }
        precondition(b.uint64Value >= a.uint64Value)
        return [first.uint64Value, last.uint64Value, b.uint64Value - a.uint64Value]
    }

    /// Feed/DSR proves the document was parsed before control-channel gestures; no sleep guesses or fake cell metrics.
    private static func prepare(_ surface: UInt64, pane: TerminalSurfaceView, text: String, padding: Int,
                                input: () -> Data, reset: () -> Void) {
        reset()
        let output = "\u{1b}c" + text + "\u{1b}[H\u{1b}[6n"
        var accepted = 0
        precondition(PuzzleRuntime.withBytes(output) { puzzle_surface_feed_output(surface, $0, &accepted) } == PUZZLE_OK)
        precondition(accepted == output.utf8.count)
        PuzzleRuntimeTests.until("Fresh selection document and presented first-cell origin") {
            input() == Data("\u{1b}[1;1R".utf8) && pane.imeCursorRect.minX == CGFloat(padding) && pane.imeCursorRect.minY == CGFloat(padding)
        }
    }

    /// Configuration is queued through the same live public C API as ordinary app settings.
    private static func configure(_ surface: UInt64, _ json: String) {
        precondition(PuzzleRuntime.withBytes(json) { puzzle_surface_set_config(surface, $0) } == PUZZLE_OK)
    }

    /// Create native test samples in window coordinates so TerminalInput performs its real view conversion.
    private static func drag(_ pane: TerminalSurfaceView, from: NSPoint, to: NSPoint) {
        let cell = pane.imeCursorRect
        for (type, point) in [(NSEvent.EventType.leftMouseDown, from), (.leftMouseDragged, to), (.leftMouseUp, to)] {
            let local = NSPoint(x: cell.minX + point.x * cell.width, y: cell.minY + (point.y + 0.5) * cell.height)
            let event = NSEvent.mouseEvent(with: type, location: pane.convert(local, to: nil), modifierFlags: [], timestamp: 0,
                windowNumber: pane.window!.windowNumber, context: nil, eventNumber: 1, clickCount: 1, pressure: 1)!
            switch type {
            case .leftMouseDown: pane.mouseDown(with: event)
            case .leftMouseDragged: pane.mouseDragged(with: event)
            default: pane.mouseUp(with: event)
            }
        }
    }
}
#endif
