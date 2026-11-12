// Host-owned container geometry and input lifetime; Puzzle owns its IOSurface child and image allocation (D-56/D-60).
import AppKit
import QuartzCore

/// Configure only the retained container; Puzzle alone configures its IOSurface presentation child.
enum TerminalSurfaceGeometry {
    /// D-54 keeps a stale frame unscaled at the visual top left while Puzzle prepares a resized frame.
    /// This also works for placeholders: they are native subviews laid out in points, not drawables.
    static func configureHostLayer(_ layer: CAMetalLayer, scale: CGFloat) {
        // AppKit's flipped view (including inherited orientation) reverses gravity's top/bottom names.
        layer.contentsGravity = layer.contentsAreFlipped() ? .bottomLeft : .topLeft
        layer.contentsScale = scale
    }

    /// Normalize backing pixels for the C event, including empty or non-finite transient layout states.
    /// Clamp before converting so malformed geometry cannot trap Swift's integer conversion.
    static func pixelExtent(_ pixels: CGFloat) -> UInt32 {
        guard pixels.isFinite, pixels > 0 else { return 0 }
        return UInt32(min(pixels.rounded(), CGFloat(UInt32.max)))
    }
}

/// A host-owned native surface with a placeholder until Puzzle reports its first presented frame.
/// Native kind 5 installs Puzzle's IOSurface child under this exact container; moving a pane preserves both.
@MainActor
final class TerminalSurfaceView: NSView {
    /// A session ID stays fixed for the complete view lifetime, including window transfers.
    let tabID: UInt64
    /// A stable leaf identity routes rendering and input independently of the title-bar tab.
    let paneID: UInt64
    /// Becoming first responder makes this leaf the target of subsequent split commands.
    var onFocus: (() -> Void)?
    /// Window ownership changes only when this persistent surface moves into a new window.
    var windowID: UInt64
    /// The shell retains this layer until PANE_CLOSING has returned.
    let metalLayer = CAMetalLayer()
    /// Compact panes hide secondary decoration instead of letting labels spill into their neighbors.
    private var placeholderImage: NSImageView?
    /// The explanatory text is optional at narrow pane widths; the central Puzzle label remains.
    private var placeholderMessage: NSTextField?
    /// Placeholder widgets are ordinary native views above the empty renderer layer.
    private let placeholder = NSStackView()
    /// Lifecycle state prevents input and geometry callbacks before PANE_CREATED/after PANE_CLOSING.
    private(set) var isLive = false
    /// Visibility concerns presentation only; background Rust sessions can keep their PTYs alive.
    private var sessionVisible = false
    /// Cache geometry so layout storms do not issue redundant resize callbacks.
    private var lastGeometry: NSSize = .zero
    /// Scale changes must emit even when the pixel size happens to remain unchanged.
    private var lastScale: CGFloat = 0
    /// Actual window focus differs from being the first responder in a background window.
    private var lastFocus = false
    /// AppKit input-method state is in UTF-16 coordinates, matching NSTextInputClient.
    var markedText = NSAttributedString(string: "")
    /// Selection is relative to markedText; terminal scrollback is not an editable Cocoa document.
    var markedSelection = NSRange(location: NSNotFound, length: 0)
    /// Rust updates the caret in view-local points so macOS can position IME candidate windows.
    var imeCursorRect = NSRect(x: 16, y: 16, width: 8, height: 18)
    /// Cocoa insertText during keyDown is bundled with that key for Puzzle's kitty/binding encoder.
    var interpretingKey = false
    /// Owned text gathered synchronously during one interpretKeyEvents call.
    var interpretedText: String?
    /// Releases use the press's committed layout identity even if modifiers have since changed.
    var pressedKeys: [UInt16: PuzzleKeyInput] = [:]
    /// Native click chains and held buttons are input facts absent from the legacy C observer record.
    var pointerClicks: UInt32 = 0
    /// Track native buttons through drag/release, including synthetic events used by the host tests.
    var heldPointerButtons: Set<Int> = []
    /// Genuine native motion retained only while the corresponding left-button gesture is active.
    var replyDragEvent: NSEvent?
    /// Press/release epochs invalidate ready work deferred until after a gesture has ended.
    var replyDragGesture: UInt64 = 0
    /// A constant-size enclosure of earlier ended motions refuses ambiguous delayed callbacks conservatively.
    var replyDragRetiredBounds: NSRect?
    /// Current gesture's motion enclosure is retired on release/new press, not an unbounded NSEvent history.
    var replyDragActiveBounds: NSRect?
    /// Cursor regions are per pane; other panes must not inherit a link/drag cursor.
    var puzzleCursor: NSCursor = .iBeam
    #if PUZZLE_ENABLED
    /// Present only for a program-registered native drop target; reset and close discard its acceptance.
    var dropTarget: PuzzleDropTarget?
    /// At most one ready/native export per surface; AppKit retains a started source independently on close.
    var replyDragSource: SurfaceDragSource?
    /// Monotonic surface-local request watermark makes replayed ready/cancel callbacks inert.
    var lastReplyDragRequest: UInt64 = 0
    #endif
    /// Public policy for the initial approved producer only; split panes and ordinary tabs leave this nil.
    private var pendingLaunch: SurfaceLaunch?
    /// Failed creation/replayed lifecycle events must never retry a grant or fall back to a login shell.
    private(set) var launchAttempted = false

    /// Consume one explicit approval exactly once, releasing command buffers after the cold C call.
    func takeLaunch() -> SurfaceLaunch? {
        guard !launchAttempted, let request = pendingLaunch else { return nil }
        launchAttempted = true
        pendingLaunch = nil
        return request
    }

    /// Equal-scale display transfers still need a new Puzzle display-link identity.
    private var lastDisplay: UInt64 = 0

    /// Install a stable Metal layer without creating a GPU device, surface, or renderer yet.
    init(tabID: UInt64, windowID: UInt64, paneID: UInt64, producer: SurfaceLaunch? = nil) {
        precondition(tabID != 0 && paneID != 0 && windowID != 0, "A rendering leaf requires live shell identities")
        self.tabID = tabID
        self.paneID = paneID
        pendingLaunch = producer
        self.windowID = windowID
        super.init(frame: .zero)
        layer = metalLayer
        wantsLayer = true
        clipsToBounds = true // A narrow pane must never let placeholder subviews draw across a native divider.
        metalLayer.isOpaque = true
        metalLayer.backgroundColor = NSColor(calibratedWhite: 0.065, alpha: 1).cgColor
        TerminalSurfaceGeometry.configureHostLayer(metalLayer, scale: 1)
        resetDropDestination()
        buildPlaceholder()
        setAccessibilityLabel("Terminal content placeholder")
    }

    /// A surface requires stable shell identities, so nib decoding is intentionally unsupported.
    required init?(coder: NSCoder) { fatalError("Use init(tabID:windowID:paneID:) for terminal surfaces") }
    /// Use top-left input coordinates, matching terminal row/column orientation.
    override var isFlipped: Bool { true }
    /// Terminal views must accept keyboard focus without a hidden text field.
    override var acceptsFirstResponder: Bool { true }
    /// The terminal content area handles pointer events, never title-bar window dragging.
    override var mouseDownCanMoveWindow: Bool { false }

    /// Make the future terminal area visually explicit without imitating unfinished Rust rendering.
    private func buildPlaceholder() {
        let image = NSImageView(image: NSImage(systemSymbolName: "terminal", accessibilityDescription: "Terminal")!)
        placeholderImage = image
        image.contentTintColor = NSColor(calibratedWhite: 0.5, alpha: 1) // Stay legible over the fixed dark placeholder.
        image.translatesAutoresizingMaskIntoConstraints = false
        image.widthAnchor.constraint(equalToConstant: 40).isActive = true
        image.heightAnchor.constraint(equalToConstant: 40).isActive = true
        let title = NSTextField(labelWithString: "Puzzle")
        title.font = .systemFont(ofSize: 22, weight: .semibold)
        title.textColor = NSColor(calibratedWhite: 0.85, alpha: 1)
        let message = NSTextField(labelWithString: "Terminal content will appear here")
        placeholderMessage = message
        message.font = .systemFont(ofSize: 13)
        message.textColor = NSColor(calibratedWhite: 0.5, alpha: 1)
        placeholder.orientation = .vertical
        placeholder.alignment = .centerX
        placeholder.spacing = 12
        placeholder.translatesAutoresizingMaskIntoConstraints = false
        [image, title, message].forEach { placeholder.addArrangedSubview($0) }
        addSubview(placeholder)
        NSLayoutConstraint.activate([
            placeholder.centerXAnchor.constraint(equalTo: centerXAnchor),
            placeholder.centerYAnchor.constraint(equalTo: centerYAnchor)
        ])
    }

    /// Only the overlay changes; Rust remains responsible for configuring and drawing the layer.
    func setPlaceholderVisible(_ visible: Bool) {
        placeholder.isHidden = !visible
        setAccessibilityLabel(visible ? "Terminal content placeholder" : "Terminal")
    }

    /// Headless/runtime tests observe presentation without acquiring or inspecting a GPU drawable.
    var placeholderVisible: Bool { !placeholder.isHidden }

    /// AppKit restores the correct affordance whenever the pointer enters this pane's bounds.
    override func resetCursorRects() { addCursorRect(bounds, cursor: puzzleCursor) }

    /// Populate borrowed surface pointers for lifecycle and geometry events.
    func surfaceEvent(_ kind: PuzzleShellEventKind) -> PuzzleShellEvent {
        var event = ShellBridge.shared.event(kind, window: windowID, tab: tabID, pane: paneID)
        event.ns_view = Unmanaged.passUnretained(self).toOpaque()
        event.metal_layer = Unmanaged.passUnretained(metalLayer).toOpaque()
        let scale = window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 1
        let pixels = convertToBacking(bounds).size
        event.scale = Double(scale)
        // Rounding matches integer IOSurface extents; clamp protects the FFI conversion.
        event.width_px = TerminalSurfaceGeometry.pixelExtent(pixels.width)
        event.height_px = TerminalSurfaceGeometry.pixelExtent(pixels.height)
        return event
    }

    /// Announce a persistent session before any geometry, visibility, focus or input callbacks.
    func emitCreated(title: String) {
        isLive = true
        var event = surfaceEvent(PUZZLE_PANE_CREATED)
        event.code = UInt32(clamping: ShellCoordinator.shared.tab(containing: paneID)?.layout.root.paneIDs.firstIndex(of: paneID) ?? 0)
        ShellBridge.shared.emit(event, text: title)
    }

    /// Final lifecycle event allows Rust to synchronously release borrowed GPU surface resources.
    func emitClosing() {
        guard isLive else { return }
        ShellBridge.shared.emit(surfaceEvent(PUZZLE_PANE_CLOSING))
        isLive = false
        replyDragEvent = nil
        replyDragGesture &+= 1
        resetDropDestination() // No native acceptance or drag pasteboard survives pane teardown.
    }

    /// Unconditional snapshot is used by callback registration and tab selection.
    func emitGeometry() {
        guard isLive else { return }
        ShellBridge.shared.emit(surfaceEvent(PUZZLE_SURFACE_CHANGED))
    }

    /// Synchronously forward backing size/scale/display before CA commits; never touch Puzzle's child or drawableSize.
    func updateGeometry() {
        let scale = window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 1
        let size = convertToBacking(bounds).size
        let display = (window?.screen?.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?.uint64Value ?? 0
        TerminalSurfaceGeometry.configureHostLayer(metalLayer, scale: scale)
        // Report zero extents too: Puzzle suspends presentation during detachment/empty layout.
        // AppKit lays out the layer and placeholder subviews in points without acquiring a drawable.
        if lastGeometry != size || lastScale != scale || lastDisplay != display {
            lastGeometry = size
            lastScale = scale
            lastDisplay = display
            emitGeometry()
        }
    }

    /// Direct frame changes (divider drags and discrete window jumps) must reach Puzzle in this CA transaction.
    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        updateGeometry() // D-60: no asynchronous dispatch between native bounds mutation and surface_resize.
    }

    /// A bounds-only change also affects the retained container, even when its frame is unchanged.
    override func setBoundsSize(_ newSize: NSSize) {
        super.setBoundsSize(newSize)
        updateGeometry()
    }

    /// AppKit invokes layout after constraints or frame/autoresizing changes.
    override func layout() {
        // Nested minima can be smaller than a full-window placeholder; keep decoration inside its own leaf.
        placeholderImage?.isHidden = bounds.height < 170 || bounds.width < 150
        placeholderMessage?.isHidden = bounds.height < 110 || bounds.width < 260
        super.layout()
        updateGeometry()
    }
    /// Reparenting may change backing conversion and inherited contents orientation (D-54).
    /// updateGeometry always reapplies host gravity, even when cached dimensions/scale match.
    override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); updateGeometry() }
    /// Crossing displays updates layer scale and Rust's pixel-size snapshot.
    override func viewDidChangeBackingProperties() { super.viewDidChangeBackingProperties(); updateGeometry() }

    /// Visibility controls future rendering suspension, never destruction of the terminal session.
    func setSessionVisible(_ visible: Bool) {
        guard sessionVisible != visible else { return }
        sessionVisible = visible
        emitVisibility()
    }

    /// Replay current presentation state, including an inactive tab's hidden state.
    func emitVisibility() {
        guard isLive else { return }
        var event = ShellBridge.shared.event(PUZZLE_VISIBILITY_CHANGED, window: windowID, tab: tabID, pane: paneID)
        event.flags = sessionVisible ? 1 : 0
        ShellBridge.shared.emit(event)
    }

    /// Focus must satisfy both first-responder and key-window requirements.
    func emitFocus(force: Bool = true) {
        guard isLive else { return }
        let focused = window?.isKeyWindow == true && window?.firstResponder === self
        guard force || focused != lastFocus else { return }
        lastFocus = focused
        var event = ShellBridge.shared.event(PUZZLE_FOCUS_CHANGED, window: windowID, tab: tabID, pane: paneID)
        event.flags = focused ? 1 : 0
        ShellBridge.shared.emit(event)
    }

    /// Emit focus after AppKit accepts this view as the keyboard responder.
    override func becomeFirstResponder() -> Bool {
        let accepted = super.becomeFirstResponder()
        if accepted {
            onFocus?() // The native responder transition updates the tab model before Rust receives the focus event.
            // AppKit may not yet have installed firstResponder while executing this method.
            lastFocus = window?.isKeyWindow == true
            var event = ShellBridge.shared.event(PUZZLE_FOCUS_CHANGED, window: windowID, tab: tabID, pane: paneID)
            event.flags = lastFocus ? 1 : 0
            if isLive { ShellBridge.shared.emit(event) }
        }
        return accepted
    }

    /// Clear marked text and focus when a user switches sessions or closes the current window.
    override func resignFirstResponder() -> Bool {
        let accepted = super.resignFirstResponder()
        if accepted {
            unmarkText()
            lastFocus = false
            let event = ShellBridge.shared.event(PUZZLE_FOCUS_CHANGED, window: windowID, tab: tabID, pane: paneID)
            if isLive { ShellBridge.shared.emit(event) }
        }
        return accepted
    }
}
