// M18.3: clipboard completions enqueue Puzzle messages; Swift never generates paste-grant tokens.
import AppKit
#if PUZZLE_ENABLED

/// Adapts native MIME results to puzzle.h while guarding handles across deferred OS work.
extension PuzzleRuntime {
    /// The copied callback contains alternatives and correlation, never the program's secret password.
    func clipboardMime(_ value: Any?, paneID: UInt64, surface: UInt64) {
        guard let request = PuzzleClipboardRequest(value) else { return }
        clipboard.complete(request, isLive: { [weak self] in self?.surfaces[paneID] == surface }) { [weak self] answer in
            guard let self, request.token != 0 else { return } // API writes intentionally have no OSC acknowledgement.
            let result = PuzzleClipboardBuffers.withItems(answer.items) { items in
                puzzle_surface_clipboard_mime_answer(surface, request.token, answer.status.rawValue,
                                                     answer.revision, items.baseAddress, items.count)
            }
            self.report(result, operation: "Complete MIME clipboard operation")
        }
    }

    /// clipboard/read is Puzzle's bound user-paste effect; OSC reads use clipboard/mime instead.
    func pasteClipboard(surface: UInt64) {
        guard let snapshot = clipboard.pasteSnapshot() else { return }
        let result = PuzzleClipboardBuffers.withNames(snapshot.names) { names in
            Self.withBytes(snapshot.text ?? "") { text in
                puzzle_surface_clipboard_paste(surface, 0, snapshot.revision, names.baseAddress, names.count, text)
            }
        }
        report(result, operation: "Paste clipboard gesture")
    }
}
#endif
