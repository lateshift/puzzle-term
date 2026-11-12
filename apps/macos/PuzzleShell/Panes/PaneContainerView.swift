import AppKit

/// A stable root hosts the selected tab's entire tree; inactive roots remain retained by their tabs.
@MainActor
final class PaneContainerView: NSView {
    /// The installed root is either one leaf host or a nested native split view.
    private var tree: NSView?
    /// Top-left layout makes stacked split directions consistent throughout the tree.
    override var isFlipped: Bool { true }

    /// Swap topology only; persistent surfaces are reparented by the factory before this call.
    func install(_ view: NSView) {
        tree?.removeFromSuperview()
        tree = view
        view.frame = bounds
        view.autoresizingMask = [.width, .height]
        addSubview(view)
        if let split = view as? PaneSplitView { split.adjustSubviews() } // Resolve first layout even when the root's frame has not changed.
    }
}

/// A two-point inset reserves a subtle focus outline without painting on a renderer-owned Metal layer.
@MainActor
final class PaneHostView: NSView {
    /// The surface stays alive in the tab's pane registry while hosts are rebuilt.
    let surface: TerminalSurfaceView
    /// Show the accent only when a split tab has this pane selected in the key window.
    var highlighted = false { didSet { needsDisplay = true } }

    /// Mount the existing rendering view instead of creating or copying any terminal implementation.
    init(surface: TerminalSurfaceView) {
        self.surface = surface
        super.init(frame: .zero)
        wantsLayer = true
        clipsToBounds = true // Leaf decoration remains inside its own branch when neighboring panes resize.
        layer?.backgroundColor = NSColor(calibratedWhite: 0.065, alpha: 1).cgColor
        addSubview(surface)
    }
    /// Hosts are internal tree nodes, never storyboard objects.
    required init?(coder: NSCoder) { fatalError("Use init(surface:)") }
    /// Share the terminal's top-left coordinate convention.
    override var isFlipped: Bool { true }

    /// Keep the focus frame outside the terminal's drawable area and emit real leaf geometry through layout.
    override func layout() {
        super.layout()
        layoutSurface()
    }

    /// Native divider drags resize frames directly, so update the renderer immediately rather than waiting for constraints.
    override func resizeSubviews(withOldSize oldSize: NSSize) { layoutSurface() }

    /// Both ordinary layout and live resizing share one zero-safe drawable geometry path.
    private func layoutSurface() {
        surface.frame = NSRect(x: 2, y: 2, width: max(0, bounds.width - 4), height: max(0, bounds.height - 4)) // Empty initial layout must remain a valid zero extent.
        surface.updateGeometry() // Rust sees each leaf's new backing extent continuously while its divider moves.
    }

    /// Draw a crisp system-accent outline; inactive panes keep only the neutral gutter.
    override func draw(_ dirtyRect: NSRect) {
        guard highlighted else { return }
        let scale = window?.backingScaleFactor ?? 1
        let stroke = 1 / scale // Match a physical pixel across Retina and ordinary displays.
        NSColor.controlAccentColor.withAlphaComponent(0.65).setStroke()
        let outline = NSBezierPath(rect: bounds.insetBy(dx: stroke / 2, dy: stroke / 2))
        outline.lineWidth = stroke
        outline.stroke()
    }
}
