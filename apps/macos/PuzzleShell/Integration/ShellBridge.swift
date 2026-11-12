// Synchronous shell notifications feed the Swift Puzzle adapter and the prototype's optional C observer.
import AppKit

/// One main-thread dispatcher owns the borrowed Rust callback and scopes all transient pointers.
@MainActor
final class ShellBridge {
    /// Shared by every window; a future Rust runtime installs one process-wide callback.
    static let shared = ShellBridge()
    /// Prevent callback-triggered mutations from invalidating an ongoing lifecycle traversal.
    private(set) var isDelivering = false
    /// C function pointer, not a Swift closure; no Swift runtime knowledge is required by Rust.
    private var callback: PuzzleShellEventCallback?
    /// Opaque Rust-owned context; the shell never dereferences or frees it.
    private var context: UnsafeMutableRawPointer?

    /// Install or detach a listener, replaying current state when one is attached.
    func install(_ callback: PuzzleShellEventCallback?, context: UnsafeMutableRawPointer?) {
        self.callback = callback
        self.context = context
        if callback != nil { ShellCoordinator.shared.replayState() }
    }

    /// Zero unspecified fields and populate only the identity common to every event.
    func event(_ kind: PuzzleShellEventKind, window: UInt64 = 0, tab: UInt64 = 0, pane: UInt64 = 0) -> PuzzleShellEvent {
        var value = PuzzleShellEvent()
        value.abi_version = UInt32(PUZZLE_SHELL_ABI_VERSION)
        value.kind = kind.rawValue
        value.window_id = window
        value.tab_id = tab
        value.pane_id = pane // Structural window/tab events leave this zero; surface/input events identify a leaf.
        return value
    }

    /// Keep the stack record and optional UTF-8 buffer alive only during the synchronous callback.
    func emit(_ value: PuzzleShellEvent, text: String? = nil) {
        precondition(!isDelivering, "Nested shell events violate the C callback contract")
        isDelivering = true
        defer { isDelivering = false } // Reset the mutation guard even when there is no text.
        // The Swift adapter receives every event even when the prototype's C listener is absent.
        // In particular, synchronous surface release must finish before PANE_CLOSING returns.
        PuzzleGlue.shared.receive(value, text: text)
        guard let callback else { return } // The Phase A adapter has no runtime; placeholders remain visible.
        var record = value
        if let text {
            let bytes = Array(text.utf8) // The C payload deliberately excludes a trailing NUL.
            bytes.withUnsafeBufferPointer { buffer in
                record.text = buffer.baseAddress
                record.text_length = buffer.count
                withUnsafePointer(to: &record) { callback(context, $0) }
            }
        } else {
            withUnsafePointer(to: &record) { callback(context, $0) }
        }
    }
}

