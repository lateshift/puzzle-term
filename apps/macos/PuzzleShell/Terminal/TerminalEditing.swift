// Native Edit menu actions target the focused responder, never a fixed terminal or background tab.
import AppKit

/// Field editors implement their own Cocoa actions; only focused terminal views use these handlers.
extension TerminalSurfaceView {
    /// User paste enters Puzzle's MIME-aware grant API, preserving alternatives/revision and bracketed-paste policy.
    @objc func paste(_ sender: Any?) {
        #if PUZZLE_ENABLED
        guard isLive, let runtime = PuzzleGlue.shared.runtime as? PuzzleRuntime,
              let surface = runtime.surfaces[paneID] else { return }
        runtime.pasteClipboard(surface: surface)
        #endif
    }
    /// Reuse the core's established copy binding; no AppKit key event or OS keystroke is fabricated.
    @objc func copy(_ sender: Any?) { menuBinding("c", physical: "KeyC") }
    /// Core selection stays on its Session owner and includes logical/folded source text.
    override func selectAll(_ sender: Any?) { menuBinding("a", physical: "KeyA") }
    /// The C keybinding seam is the existing command route for copy/select-all; it remains asynchronous.
    private func menuBinding(_ logical: String, physical: String) {
        guard isLive else { return }
        PuzzleGlue.shared.nativeKey(paneID: paneID, input: PuzzleKeyInput(menuBinding: logical, physical: physical, pressed: true))
        PuzzleGlue.shared.nativeKey(paneID: paneID, input: PuzzleKeyInput(menuBinding: logical, physical: physical, pressed: false))
    }
}
