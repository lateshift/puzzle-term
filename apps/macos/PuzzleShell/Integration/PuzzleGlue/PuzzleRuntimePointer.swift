// Forward top-left view-local AppKit points; Puzzle owns scale, padding and cell mapping.
import AppKit
#if PUZZLE_ENABLED

/// Native gestures carry logical positions without reconstructing Puzzle's rendering geometry.
extension PuzzleRuntime {
    /// Translate Cocoa button identities to xterm's left/middle/right and auxiliary numbering.
    private func button(_ native: UInt32) -> UInt32 {
        switch native {
        case 0: return 0
        case 1: return 2
        case 2: return 1
        default: return min(128 + native - 3, UInt32(UInt16.max))
        }
    }

    /// Shell modifier bits differ from puzzle.h: Control and Option exchange positions.
    private func pointerModifiers(_ bits: UInt64) -> UInt32 {
        UInt32(bits & 1) | (bits & 2 != 0 ? 4 : 0) | (bits & 4 != 0 ? 2 : 0) | UInt32(bits & 8)
    }

    /// Keep signed view-local points for selection beyond the viewport; no DPR or padding arithmetic.
    private func pointerValue(_ event: PuzzleShellEvent, kind: UInt32) -> puzzle_pointer_t? {
        guard let pane = ShellCoordinator.shared.tab(containing: event.pane_id)?.panes[event.pane_id] else { return nil }
        // bf59c46 makes the legacy cell metrics reserved: Puzzle maps this sample using
        // its own live geometry, including padding/font/scale changes not yet delivered to tick.
        return puzzle_pointer_t(kind: kind, button: button(event.detail), x: event.x, y: event.y,
                                cell_width: 0, cell_height: 0, modifiers: pointerModifiers(event.flags),
                                held: pane.heldPointerButtons.isEmpty ? 0 : 1, clicks: pane.pointerClicks)
    }

    /// Puzzle coalesces motion; button edges remain immediate and ordered with other controls.
    func pointer(_ event: PuzzleShellEvent) {
        let kind: UInt32 = event.code == 0 ? 1 : (event.code == 1 ? 2 : (event.code == 3 ? 3 : 0))
        guard let surface = surfaces[event.pane_id], var value = pointerValue(event, kind: kind) else { return }
        report(puzzle_surface_pointer(surface, &value), operation: "Send pointer")
    }

    /// Precise OS inertia uses logical points; ordinary wheels use rows, as puzzle.h specifies.
    func scroll(_ event: PuzzleShellEvent) {
        guard let surface = surfaces[event.pane_id], var value = pointerValue(event, kind: 0) else { return }
        // Positive Cocoa Y means up; Puzzle's positive viewport delta means down.
        let delta = -event.delta_y
        report(puzzle_surface_scroll(surface, delta, event.detail == 1 ? 1 : 0, &value), operation: "Send scroll")
        // ABI v1 accepts vertical samples including OS momentum. It has no phase/horizontal fields;
        // those legacy observer fields remain available to native tests without inventing a C ABI.
    }
}
#endif
