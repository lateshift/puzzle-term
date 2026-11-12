import AppKit

/// One binary native split keeps AppKit's divider tracking, cursor and accessibility behavior.
/// Only geometry policy and painting are customized; no hand-written mouse tracking loop is needed.
@MainActor
final class PaneSplitView: NSSplitView, NSSplitViewDelegate {
    /// Stable identity survives a topology rebuild when another pane is inserted or removed.
    let branchID: UInt64
    /// Store the user's ratio rather than a pixel position, so window resizing retains intent.
    private(set) var fraction: Double
    /// Each child may itself be an entire split subtree with a larger minimum size.
    private let firstMinimum: NSSize
    private let secondMinimum: NSSize
    /// Prevent programmatic proportional layout from overwriting the user's saved ratio.
    private var arranging = false
    /// The owning tab saves only genuine divider changes to its model.
    var onResize: ((UInt64, Double) -> Void)?

    /// Construct a divider around existing views, preserving terminal NSView/CAMetalLayer identity.
    init(id: UInt64, sideBySide: Bool, fraction: Double, first: NSView, second: NSView,
         firstMinimum: NSSize, secondMinimum: NSSize) {
        branchID = id
        self.fraction = fraction
        self.firstMinimum = firstMinimum
        self.secondMinimum = secondMinimum
        super.init(frame: .zero)
        isVertical = sideBySide // AppKit's vertical flag describes the divider, not the child stacking direction.
        dividerStyle = .thin
        delegate = self // The view owns its policy; AppKit retains the native drag implementation.
        addSubview(first)
        addSubview(second)
        setAccessibilityLabel(sideBySide ? "Side-by-side terminal panes" : "Stacked terminal panes")
    }

    /// Pane topology is code-created and cannot be reconstructed from a storyboard.
    required init?(coder: NSCoder) { fatalError("Use the pane-tree initializer") }
    /// Make the first horizontal child the TOP pane, matching the direction API and terminal coordinates.
    override var isFlipped: Bool { true }
    /// Keep the visible separation restrained; native hit testing is expanded independently below.
    override var dividerThickness: CGFloat { PaneSizing.divider }

    /// Paint a quiet neutral line that stays visible against the charcoal terminal placeholder.
    override func drawDivider(in rect: NSRect) {
        NSColor(calibratedWhite: 0.25, alpha: 1).setFill()
        rect.fill()
    }

    /// Use saved proportions when a window or ancestor divider changes this branch's extent.
    func splitView(_ splitView: NSSplitView, resizeSubviewsWithOldSize oldSize: NSSize) { arrangeChildren() }

    /// Assign deterministic child frames, allowing their own native resize hooks to recurse naturally.
    private func arrangeChildren() {
        guard subviews.count == 2 else { return }
        arranging = true
        defer { arranging = false } // Child geometry callbacks must not accidentally change the stored divider proportion.
        let total = isVertical ? bounds.width : bounds.height
        let aMin = isVertical ? firstMinimum.width : firstMinimum.height
        let bMin = isVertical ? secondMinimum.width : secondMinimum.height
        let first = PaneSizing.firstExtent(total: total, fraction: fraction, firstMinimum: aMin, secondMinimum: bMin)
        let second = max(0, total - dividerThickness - first)
        if isVertical {
            subviews[0].frame = NSRect(x: 0, y: 0, width: first, height: bounds.height)
            subviews[1].frame = NSRect(x: first + dividerThickness, y: 0, width: second, height: bounds.height)
        } else {
            subviews[0].frame = NSRect(x: 0, y: 0, width: bounds.width, height: first)
            subviews[1].frame = NSRect(x: 0, y: first + dividerThickness, width: bounds.width, height: second)
        }
    }

    /// Save dragged positions; programmatic layout and zero-size detached trees are ignored.
    func splitViewDidResizeSubviews(_ notification: Notification) {
        guard !arranging, subviews.count == 2 else { return }
        let available = (isVertical ? bounds.width : bounds.height) - dividerThickness
        guard available > 0 else { return }
        let first = isVertical ? subviews[0].frame.width : subviews[0].frame.height
        let expected = PaneSizing.firstExtent(total: available + dividerThickness, fraction: fraction,
                                             firstMinimum: isVertical ? firstMinimum.width : firstMinimum.height,
                                             secondMinimum: isVertical ? secondMinimum.width : secondMinimum.height)
        guard abs(first - expected) > 0.5 else { return } // AppKit may notify after our resize delegate returns; proportional layout must not persist a temporary minimum clamp.
        let next = Double(first / available)
        guard next > 0, next < 1, next != fraction else { return }
        fraction = next
        onResize?(branchID, next) // The model remains authoritative when this native tree is later rebuilt.
    }

    /// A leaf cannot disappear by dragging its divider; close-pane is the explicit destruction path.
    func splitView(_ splitView: NSSplitView, canCollapseSubview subview: NSView) -> Bool { false }
    /// Stop dragging before the complete first subtree becomes smaller than its minimum.
    func splitView(_ splitView: NSSplitView, constrainMinCoordinate proposedMinimumPosition: CGFloat, ofSubviewAt dividerIndex: Int) -> CGFloat {
        isVertical ? firstMinimum.width : firstMinimum.height
    }
    /// Leave enough space after the divider for the complete second subtree.
    func splitView(_ splitView: NSSplitView, constrainMaxCoordinate proposedMaximumPosition: CGFloat, ofSubviewAt dividerIndex: Int) -> CGFloat {
        (isVertical ? bounds.width - secondMinimum.width : bounds.height - secondMinimum.height) - dividerThickness
    }
    /// Expand the thin line to a comfortable nine-point grab area without widening the visual gutter.
    func splitView(_ splitView: NSSplitView, effectiveRect proposedEffectiveRect: NSRect, forDrawnRect drawnRect: NSRect, ofDividerAt dividerIndex: Int) -> NSRect {
        drawnRect.insetBy(dx: isVertical ? -4 : 0, dy: isVertical ? 0 : -4).intersection(bounds)
    }

    /// Explicit Rust resizing uses the same minimum-size clamping as native divider dragging.
    func setFraction(_ value: Double) {
        fraction = value
        arrangeChildren()
        needsDisplay = true
    }
}

