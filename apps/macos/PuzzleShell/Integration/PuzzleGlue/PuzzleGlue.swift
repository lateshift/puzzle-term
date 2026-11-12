// AppKit dispatch seam: window chrome stays native, pane terminal work uses puzzle.h (M17.demo).
import AppKit

/// Route pane/input notifications without making Rust depend on the prototype's C bridge.
/// PuzzleRuntime owns one Puzzle app and a pane-ID → surface-handle map in enabled builds.
@MainActor
final class PuzzleGlue {
    /// The application-wide adapter shares Puzzle's GPU/text owner across all windows (D-30).
    static let shared = PuzzleGlue()
    /// Install before the first window is created; destroy only after every pane's close returns.
    var runtime: (any PuzzleGlueRuntime)?

    /// Install before creating windows; placeholder-only builds have no Rust symbols or runtime.
    func start() {
        #if PUZZLE_ENABLED
        if runtime == nil { runtime = PuzzleRuntime() }
        #endif
    }

    /// Native closes have already completed before the delegate releases the shared App.
    func stop() {
        #if PUZZLE_ENABLED
        (runtime as? PuzzleRuntime)?.shutdown()
        #endif
        runtime = nil
    }

    /// Physical keys need the complete Cocoa snapshot rather than the legacy observer payload.
    func nativeKey(paneID: UInt64, input: PuzzleKeyInput) { runtime?.nativeKey(paneID: paneID, input: input) }

    /// Dispatch synchronously while ShellBridge's mutation guard protects the pane traversal.
    /// Moving/reordering tabs changes host ownership only; it must never recreate a Puzzle session.
    func receive(_ event: PuzzleShellEvent, text: String?) {
        guard let runtime else { return }
        switch event.kind {
        case PUZZLE_PANE_CREATED.rawValue: runtime.createPane(event)
        case PUZZLE_PANE_CLOSING.rawValue: runtime.closePane(event)
        case PUZZLE_SURFACE_CHANGED.rawValue: runtime.resizePane(event)
        case PUZZLE_TEXT_COMMITTED.rawValue:
            // keyDown collects synchronous insertText; its combined key message owns that text.
            let pane = ShellCoordinator.shared.tab(containing: event.pane_id)?.panes[event.pane_id]
            if pane?.interpretingKey != true { runtime.commitText(event, text: text ?? "") }
        case PUZZLE_COMPOSITION_CHANGED.rawValue: runtime.compose(event, text: text ?? "")
        case PUZZLE_POINTER.rawValue: runtime.pointer(event)
        case PUZZLE_SCROLL.rawValue: runtime.scroll(event)
        case PUZZLE_FOCUS_CHANGED.rawValue: runtime.focus(event)
        case PUZZLE_VISIBILITY_CHANGED.rawValue: runtime.visibility(event)
        default: break // Structural events stay native; legacy key payloads are superseded by nativeKey.
        }
    }

    /// The one callback Puzzle may invoke from a worker schedules its tick onto AppKit's queue.
    /// No borrowed event data or raw context pointer crosses this queue hop; shutdown clears runtime.
    nonisolated static func requestTick() {
        puzzle_tick_wakeup()
        #if !PUZZLE_UNCOALESCED_TICKS
        // Several workers may wake at once. Exactly one owns the pending main-queue block.
        guard puzzle_tick_claim() != 0 else { return }
        #endif
        puzzle_tick_enqueued()
        DispatchQueue.main.async {
            // Clear before delivery: a wake during tick must enqueue a successor, not disappear.
            #if !PUZZLE_UNCOALESCED_TICKS
            puzzle_tick_clear()
            #endif
            PuzzleAppProbe.blockRan()
            shared.runtime?.tick()
        }
    }
}
