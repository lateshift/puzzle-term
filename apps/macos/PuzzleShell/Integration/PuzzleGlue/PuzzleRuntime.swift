// M17.demo: one C Puzzle App owns all native pane surfaces; AppKit owns windows and exact layers.
import AppKit
#if PUZZLE_ENABLED

/// Nonblocking worker callback: never enters Puzzle recursively or carries a borrowed pointer.
private nonisolated func puzzleWakeup(_ context: UnsafeMutableRawPointer?) { PuzzleGlue.requestTick() }

/// Copy callback buffers before returning; C promises event delivery on the App's owner thread.
private nonisolated func puzzleEvent(_ context: UnsafeMutableRawPointer?, _ surface: UInt64,
                                    _ event: UnsafePointer<puzzle_event_t>?) {
    guard let event else { return }
    let name = event.pointee.name
    let bytes = event.pointee.payload
    let ownedName = String(decoding: UnsafeBufferPointer(start: name.data, count: name.len), as: UTF8.self)
    let payload = bytes.data.map { Data(bytes: $0, count: bytes.len) } ?? Data()
    MainActor.assumeIsolated {
        (PuzzleGlue.shared.runtime as? PuzzleRuntime)?.receive(surface: surface, name: ownedName, payload: payload)
    }
}

/// Keeps a native view alive until synchronous surface release has succeeded.
/// The handle map is also a generation guard for deferred host actions and callback replay.
@MainActor
final class PuzzleRuntime: PuzzleGlueRuntime {
    /// One process-wide render/text owner serves every independent window (D-30/D-41).
    private(set) var app: puzzle_app_t = 0
    /// Exactly one live surface for each shell leaf; moving a tab never changes this map.
    private(set) var surfaces: [UInt64: puzzle_surface_t] = [:]
    /// Strong native ownership prevents an erroneous free result from destroying an attached layer.
    private var views: [UInt64: TerminalSurfaceView] = [:]
    /// Device cell width/height and grid columns/rows arrive together for diagnostic receipts.
    var cells: [UInt64: (UInt32, UInt32, UInt32, UInt32)] = [:]
    /// Native clipboard I/O is deferred and revision-checked; tests use an isolated pasteboard backend.
    var clipboard = PuzzleClipboard()
    /// Delivery generations stay associated with their original surface through native callbacks.
    var notifications = PuzzleNotifications()
    /// Keep one visible diagnostic per error instead of opening a sheet for every failed event.
    private var reportedErrors: Set<String> = []
    /// Default shell ownership is Puzzle's PTY; native integration tests may drive host transport.
    private let transport: UInt32
    #if PUZZLE_GLUE_TESTS
    /// Test-only observation of the actual C event stream, especially exact host-transport input.
    var eventObserver: ((UInt64, String, Data) -> Void)?
    /// Observe real C completion admission without initiating artificial OS drag sessions in tests.
    var dragCompletionObserver: ((UInt64, UInt64, UInt32, Bool) -> Void)?
    #endif

    /// Construct on AppKit's main thread before any pane creation is announced.
    init?(transport: UInt32 = 0) {
        self.transport = transport
        var options = puzzle_app_options_t()
        options.abi_version = 1
        options.callbacks = puzzle_callbacks_t(userdata: nil, wakeup: puzzleWakeup, event: puzzleEvent)
        let result = puzzle_app_new(&options, &app)
        guard result == PUZZLE_OK else { report(result, operation: "Create Puzzle app"); return nil }
    }

    /// Copy a Swift UTF-8 buffer only for the duration of a C call; Puzzle copies retained inputs.
    static func withBytes<T>(_ value: String, _ body: (puzzle_bytes_t) -> T) -> T {
        Array(value.utf8).withUnsafeBufferPointer { body(puzzle_bytes_t(data: $0.baseAddress, len: $0.count)) }
    }

    /// Expose errors through native UI, never print library diagnostics outside tracing (R10).
    func report(_ result: puzzle_result_t, operation: String) {
        guard result != PUZZLE_OK else { return }
        let message = "\(operation): Puzzle error \(result.rawValue)"
        guard reportedErrors.insert(message).inserted else { return }
        DispatchQueue.main.async {
            let alert = NSAlert()
            alert.messageText = "Puzzle could not complete an operation"
            alert.informativeText = message
            alert.runModal()
        }
    }

    /// Create from the exact borrowed layer, even if detached geometry is temporarily zero.
    func createPane(_ event: PuzzleShellEvent) {
        guard surfaces[event.pane_id] == nil,
              let pane = ShellCoordinator.shared.tab(containing: event.pane_id)?.panes[event.pane_id],
              !pane.launchAttempted else { return }
        var options = puzzle_surface_options_t()
        options.native_kind = 5
        options.transport = transport
        options.native_view = event.metal_layer
        options.geometry = geometry(event, pane: pane)
        var surface: puzzle_surface_t = 0
        let result = HostExtensions.create(runtime: self, pane: pane, options: &options, surface: &surface)
        report(result, operation: "Create pane \(event.pane_id)")
        guard result == PUZZLE_OK else { return }
        surfaces[event.pane_id] = surface
        views[event.pane_id] = pane
        HostExtensions.created(runtime: self, surface: surface)
        // Preserve Puzzle's configured padding. Input stays view-local regardless of that setting.
    }

    /// Free is a cold lifecycle barrier; never defer it past destruction of the host layer.
    func closePane(_ event: PuzzleShellEvent) {
        guard let surface = surfaces[event.pane_id] else { return }
        views[event.pane_id]?.replyDragSource?.revoke() // Revoke promises before synchronous child/layer teardown.
        views[event.pane_id]?.replyDragSource = nil
        let result = puzzle_surface_free(surface)
        report(result, operation: "Release pane \(event.pane_id)")
        guard result == PUZZLE_OK else { return } // Keep the view retained until app shutdown on error.
        views[event.pane_id]?.resetDropDestination()
        notifications.remove(surface: surface)
        surfaces.removeValue(forKey: event.pane_id)
        views.removeValue(forKey: event.pane_id)
        cells.removeValue(forKey: event.pane_id)
    }

    /// Release the App after native pane closes; callbacks remain valid until this returns.
    func shutdown() {
        guard app != 0 else { return }
        views.values.forEach { $0.replyDragSource?.revoke(); $0.replyDragSource = nil }
        let result = puzzle_app_free(app)
        report(result, operation: "Release Puzzle app")
        guard result == PUZZLE_OK else { return }
        app = 0
        views.values.forEach { $0.resetDropDestination() }
        notifications.removeAll()
        surfaces.removeAll()
        views.removeAll()
        cells.removeAll()
    }

    /// Display identity changes independently of backing extent on equal-scale monitor transfers.
    private func geometry(_ event: PuzzleShellEvent, pane: TerminalSurfaceView?) -> puzzle_geometry_t {
        let screen = pane?.window?.screen ?? NSScreen.main
        let display = (screen?.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?.uint64Value ?? 0
        let rate = screen?.maximumFramesPerSecond ?? 60
        return puzzle_geometry_t(width: event.width_px, height: event.height_px, scale: event.scale,
                                 display: display, millihertz: UInt32(clamping: rate * 1000))
    }

    /// D-60 updates native scale/display synchronously before CA commit and queues rendering; the host never writes drawableSize.
    func resizePane(_ event: PuzzleShellEvent) {
        guard let surface = surfaces[event.pane_id] else { return }
        report(puzzle_surface_resize(surface, geometry(event, pane: views[event.pane_id])), operation: "Resize pane")
    }

    /// Send one combined key+layout+text message, retaining kitty, Option and binding policy in Rust.
    func nativeKey(paneID: UInt64, input: PuzzleKeyInput) {
        guard let surface = surfaces[paneID] else { return }
        let result = Self.withBytes(input.logical) { logical in
            Self.withBytes(input.base) { base in
                Self.withBytes(input.physical) { physical in
                    Self.withBytes(input.text) { text in
                        var key = puzzle_key_t(kind: input.kind, logical: logical, base: base, physical: physical,
                                               text: text, location: input.location, modifiers: input.modifiers,
                                               pressed: input.pressed ? 1 : 0, repeat: input.repeated ? 1 : 0)
                        return puzzle_surface_key(surface, &key)
                    }
                }
            }
        }
        report(result, operation: "Send key")
    }

    /// Independent Cocoa/IME commits have no accompanying key message and are sent exactly once.
    func commitText(_ event: PuzzleShellEvent, text: String) {
        guard let surface = surfaces[event.pane_id], PuzzleKeyInput.printable(text) else { return }
        report(Self.withBytes(text) { puzzle_surface_text(surface, $0) }, operation: "Commit text")
    }

    /// Convert AppKit UTF-16 selection to a UTF-8 byte boundary; unknown caret uses SIZE_MAX.
    func compose(_ event: PuzzleShellEvent, text: String) {
        guard let surface = surfaces[event.pane_id] else { return }
        let cursor = PuzzleComposition.byteOffset(text, utf16: event.code)
        report(Self.withBytes(text) { puzzle_surface_preedit(surface, $0, cursor) }, operation: "Update composition")
    }

    /// Forward actual key-window responder state, independent of selected-tab visibility.
    func focus(_ event: PuzzleShellEvent) {
        guard let surface = surfaces[event.pane_id] else { return }
        report(puzzle_surface_focus(surface, event.flags == 0 ? 0 : 1), operation: "Set focus")
    }

    /// Hidden/occluded panes suspend rendering while their PTY and session continue running.
    func visibility(_ event: PuzzleShellEvent) {
        guard let surface = surfaces[event.pane_id] else { return }
        let window = views[event.pane_id]?.window
        let occluded = window?.occlusionState.contains(.visible) != true || window?.isMiniaturized == true
        report(puzzle_surface_visibility(surface, event.flags == 0 ? 0 : 1, occluded ? 1 : 0), operation: "Set visibility")
    }

    /// Wakeups schedule this on the main queue; no polling timer or second event loop is installed.
    func tick() {
        guard app != 0 else { return }
        PuzzleAppProbe.tickRan()
        let measuredStart = PuzzleAppProbe.tickStart()
        report(puzzle_app_tick(app), operation: "Deliver Puzzle events")
        PuzzleAppProbe.tickEnd(measuredStart)
    }
}
#endif
