// Main-thread seam between native pane lifetime/input and the optional puzzle.h runtime (D-56).
import AppKit

/// Swift-only seam implemented by PuzzleRuntime against the committed puzzle.h.
/// Every method runs on the main thread. Events borrow their native pointers only for the call;
/// an implementation copies any data it retains and never waits for the session/parser thread.
/// These method names describe host actions, not proposed C exports or a replacement Puzzle API.
@MainActor
protocol PuzzleGlueRuntime: AnyObject {
    /// Create one Puzzle surface on event.metal_layer, indexed by pane_id, with the default login PTY.
    /// TAB_CREATED is deliberately not creation: its initial-pane pointers are only informational.
    /// Ignore an already-mapped pane when the optional C observer's installation replays live state.
    func createPane(_ event: PuzzleShellEvent)

    /// Synchronously release the render surface before returning; its layer may die immediately.
    /// Phase B must use the committed synchronous close contract, never queue this release for later.
    func closePane(_ event: PuzzleShellEvent)

    /// Forward extent/scale/display synchronously before CA commit, including zero extents; never alter the child or drawableSize.
    func resizePane(_ event: PuzzleShellEvent)

    /// Cocoa interpretation supplies layout text with its key in one message, avoiding duplicate input.
    func nativeKey(paneID: UInt64, input: PuzzleKeyInput)

    /// Send committed UTF-8 once, including dead-key/IME output, separately from physical keys.
    func commitText(_ event: PuzzleShellEvent, text: String)

    /// Send preedit and its UTF-16 selection, converting units if required by puzzle.h.
    /// Empty text clears composition; preedit is never written directly to the PTY.
    func compose(_ event: PuzzleShellEvent, text: String)

    /// Forward top-left view-local points unchanged and translate native buttons to xterm numbering.
    func pointer(_ event: PuzzleShellEvent)

    /// Preserve logical precise-point versus line units and the OS's momentum samples.
    func scroll(_ event: PuzzleShellEvent)

    /// Forward true only for the first responder in a key window, not merely the selected pane.
    func focus(_ event: PuzzleShellEvent)

    /// Suspend presentation for hidden/occluded panes while their terminal sessions keep running.
    func visibility(_ event: PuzzleShellEvent)

    /// Deliver Puzzle's pending callbacks through puzzle_app_tick(), always on the main queue.
    func tick()
}
