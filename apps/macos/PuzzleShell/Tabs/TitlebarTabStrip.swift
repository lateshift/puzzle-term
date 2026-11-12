import AppKit
import QuartzCore

/// Fixed-width title-bar tabs are clipped into complete pages, never squeezed to fit.
/// A compact trailing pager replaces the scrollbar while preserving horizontal motion.
@MainActor
final class TitlebarTabStrip: NSView {
    /// Keep interaction policy in the window controller; this view owns presentation only.
    var onSelect: ((UInt64) -> Void)?
    /// Closing from the strip uses the exact same teardown path as the bridge and Cmd-W.
    var onClose: ((UInt64) -> Void)?
    /// Commit an insertion only after a successful native drop; hover never changes model ownership.
    var onDrop: ((UInt64, Int) -> Bool)?
    /// Option-click moves an existing session into an independent window without starting a drag.
    var onTearOff: ((UInt64) -> Void)?
    /// Window-owned rename sheets can target a tab without changing the selected terminal.
    var onRename: ((UInt64) -> Void)?
    /// Clear a custom name independently of runtime title updates.
    var onResetName: ((UInt64) -> Void)?
    /// Native preset choices use the same optional RGBA metadata as the Rust bridge.
    var onSetColor: ((UInt64, UInt32?) -> Void)?
    /// Native drag sessions and hover paging are kept separate from strip geometry.
    lazy var dragInteraction = TabDragInteraction(strip: self)
    /// Reuse controls by identity so insertion animation continues from their current positions.
    private var controls: [UInt64: TabControl] = [:]
    /// The drag source disappears beneath the floating native drag preview until completion.
    private var sourceDragID: UInt64?
    /// A preview consists of a tab identity and its final index in the remaining destination list.
    private var dropPreview: (id: UInt64, index: Int)?
    /// Stable target frames avoid hit-testing against partially animated presentation positions.
    private(set) var dropFrames: [UInt64: NSRect] = [:]
    /// A subtle dashed outline occupies a complete tab slot while neighbors glide aside.
    private let dropSlot = TabDropSlot()
    /// Keep animation intent until AppKit performs the next layout pass.
    private var animateNextLayout = false
    /// Every tab has the same readable width at every window size, including overflow.
    static let tabWidth: CGFloat = 184
    /// The gap separates native-looking tab surfaces without adding heavy dividers.
    private static let gap: CGFloat = 8
    /// Pagination reserves a compact control group at the trailing edge only when needed.
    private static let pagerWidth: CGFloat = 96
    /// Native toolbar padding varies by macOS version; extend the tab silhouette to the real content edge.
    private var contentEdge: CGFloat = 0
    /// Keep the original label's vertical center while the body extends down through native toolbar padding.
    private var tabHeight: CGFloat { max(28, bounds.height - 2 - contentEdge) }
    /// A clipping view, not NSScrollView, ensures that a scrollbar can never be introduced.
    private let viewport = NSView()
    /// The document contains persistent fixed-size controls and moves horizontally as one strip.
    private let document = NSView()
    /// The pager combines previous/next actions and a quiet current-page indicator.
    private let pager = NSView()
    /// Previous/next always advance by a complete strip of tabs, never by arbitrary pixels.
    private let previousButton = NSButton()
    /// The right chevron is the discoverable affordance for the next strip of tabs.
    private let nextButton = NSButton()
    /// Monospaced digits keep the counter stable as page numbers change.
    private let pageLabel = NSTextField(labelWithString: "")
    /// Stable IDs preserve manual page browsing when titles change or tabs are added elsewhere.
    private var tabIDs: [UInt64] = []
    /// Selection changes reveal the corresponding page; resizing also keeps it visible.
    private var selectedID: UInt64?
    /// Zero-based current page; it can differ from the selected tab's page during manual browsing.
    private(set) var currentPage = 0
    /// Recomputed from the available width, with at least one complete tab on every page.
    private(set) var tabsPerPage = 1
    /// Exposed internally for meaningful overflow tests, not part of the public Rust ABI.
    private(set) var pageCount = 1
    /// Remember geometry to distinguish a true resize from a layout pass during page animation.
    private var previousCapacity = 0
    /// Preserve browsing across title-only updates; selection changes deliberately reveal its page.
    private var revealSelection = true
    /// Accumulate trackpad movement until a deliberate horizontal gesture crosses the page threshold.
    private var gestureDistance: CGFloat = 0
    /// Prevent a single gesture plus its momentum from racing through several pages.
    private var gesturePaged = false
    /// Ordinary mouse wheels lack gesture phases, so a brief idle gap starts a new gesture.
    private var lastGestureTime: TimeInterval = 0

    /// Configure clipped pages and native controls above the window's independently painted chrome.
    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
        clipsToBounds = false // Allow the viewport to meet content below the toolbar item's own padded bounds.
        layer?.masksToBounds = false // Match view clipping on older layer-backed AppKit versions as well.
        viewport.wantsLayer = true
        viewport.layer?.masksToBounds = true // Off-page tabs cannot draw over the pagination controls.
        viewport.addSubview(document)
        document.addSubview(dropSlot)
        dropSlot.isHidden = true
        registerForDraggedTypes([TabDragInteraction.pasteboardType]) // Accept only PuzzleShell tab sessions.
        addSubview(viewport)
        addSubview(pager)
        pager.wantsLayer = true
        pager.layer?.cornerRadius = 8
        configureArrow(previousButton, symbol: "chevron.left", label: "Previous page of tabs", action: #selector(previousPage(_:)))
        configureArrow(nextButton, symbol: "chevron.right", label: "Next page of tabs", action: #selector(nextPage(_:)))
        pageLabel.font = .monospacedDigitSystemFont(ofSize: 10, weight: .medium)
        pageLabel.textColor = .secondaryLabelColor
        pageLabel.alignment = .center
        pager.addSubview(pageLabel)
        setAccessibilityLabel("Terminal tabs")
        // Auto Layout gives the toolbar a flexible width and a fixed vertical height without
        // deprecated NSToolbarItem minSize/maxSize, which can distort custom views on newer macOS.
        widthAnchor.constraint(greaterThanOrEqualToConstant: Self.tabWidth).isActive = true
        widthAnchor.constraint(lessThanOrEqualToConstant: 10000).isActive = true
        heightAnchor.constraint(equalToConstant: 32).isActive = true
        setContentHuggingPriority(.defaultLow, for: .horizontal)
        setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
    }

    /// All tab controls require programmatic construction and stable session IDs.
    required init?(coder: NSCoder) { fatalError("Use init(frame:) for the title-bar strip") }
    /// A preferred width fills the native title bar; its minimum remains one readable tab.
    override var intrinsicContentSize: NSSize { NSSize(width: 600, height: 32) }
    /// Empty title-bar space remains a native window-drag region.
    override var mouseDownCanMoveWindow: Bool { true }

    /// Keep chevrons native and accessible, with restrained typography matching the window chrome.
    private func configureArrow(_ button: NSButton, symbol: String, label: String, action: Selector) {
        button.image = NSImage(systemSymbolName: symbol, accessibilityDescription: label)
        button.symbolConfiguration = NSImage.SymbolConfiguration(pointSize: 11, weight: .semibold)
        button.isBordered = false
        button.bezelStyle = .inline
        button.target = self
        button.action = action
        button.toolTip = label
        button.setAccessibilityLabel(label)
        pager.addSubview(button)
    }

    /// Reconcile controls by stable ID so animated insertion does not recreate every neighbor.
    func update(tabs: [TerminalTab], selected: UInt64?, displayTitles: [UInt64: String] = [:]) {
        revealSelection = revealSelection || selectedID != selected
        selectedID = selected
        tabIDs = tabs.map(\.id)
        for id in Array(controls.keys) where !tabIDs.contains(id) {
            controls.removeValue(forKey: id)?.removeFromSuperview()
        }
        for (index, tab) in tabs.enumerated() {
            let label = displayTitles[tab.id] ?? tab.title // Suffixes are presentation only, never written into session metadata.
            if let control = controls[tab.id] { control.configure(tab: tab, selected: tab.id == selected, displayTitle: label) }
            else {
                let control = TabControl(tab: tab, selected: tab.id == selected, displayTitle: label,
                                         select: { [weak self] in self?.onSelect?(tab.id) },
                                         close: { [weak self] in self?.onClose?(tab.id) },
                                         drag: { [weak self] event in self?.startDrag(tab.id, event: event) },
                                         tearOff: { [weak self] in self?.onTearOff?(tab.id) },
                                         rename: { [weak self] in self?.onRename?(tab.id) },
                                         resetName: { [weak self] in self?.onResetName?(tab.id) },
                                         setColor: { [weak self] in self?.onSetColor?(tab.id, $0) })
                control.frame = NSRect(x: CGFloat(index) * (Self.tabWidth + Self.gap), y: 0, width: Self.tabWidth, height: tabHeight)
                controls[tab.id] = control
                document.addSubview(control)
            }
        }
        animateNextLayout = true
        needsLayout = true
    }

    /// Snapshot a full tab at its native backing resolution for AppKit's floating drag image.
    private func startDrag(_ id: UInt64, event: NSEvent) {
        guard let control = controls[id], let bitmap = control.bitmapImageRepForCachingDisplay(in: control.bounds) else { return }
        control.cacheDisplay(in: control.bounds, to: bitmap)
        let image = NSImage(size: control.bounds.size)
        image.addRepresentation(bitmap)
        dragInteraction.begin(id: id, image: image, frame: convert(control.bounds, from: control), event: event)
    }

    /// Fade the source control out beneath its floating preview without selecting another terminal.
    func setDraggedTab(_ id: UInt64?) {
        sourceDragID = id
        applyDropFrames(animated: true)
        updatePager() // Restore visible-page accessibility after the source finishes or cancels a drag.
    }

    /// Count stable destination centers to choose an insertion boundary, excluding the source tab.
    func dropIndex(at point: NSPoint, tabID: UInt64) -> Int {
        let x = convert(point, to: document).x
        return tabIDs.filter { $0 != tabID }.reduce(0) { count, id in
            count + ((dropFrames[id] ?? controls[id]?.frame ?? .zero).midX < x ? 1 : 0)
        }
    }

    /// Reserve a complete slot and animate neighboring fixed-width controls into their new positions.
    func previewDrop(tabID: UInt64, index: Int) {
        let remaining = tabIDs.filter { $0 != tabID }.count
        let bounded = min(max(0, index), remaining)
        guard dropPreview?.id != tabID || dropPreview?.index != bounded else { return }
        dropPreview = (tabID, bounded)
        animateNextLayout = true
        needsLayout = true
        layoutSubtreeIfNeeded()
    }

    /// Cancelled/exited drags restore spacing; committed drops let the model animate into the gap.
    func clearDropPreview(restore: Bool = true) {
        dropPreview = nil
        dropSlot.isHidden = true
        if restore {
            animateNextLayout = true
            needsLayout = true
            layoutSubtreeIfNeeded()
        }
    }

    /// Animate from current control positions using a custom ease-out curve, respecting Reduce Motion.
    private func applyDropFrames(animated: Bool) {
        let remaining = tabIDs.filter { $0 != dropPreview?.id }
        var targets: [UInt64: NSRect] = [:]
        for (index, id) in remaining.enumerated() {
            let slot = index + ((dropPreview != nil && index >= dropPreview!.index) ? 1 : 0)
            targets[id] = NSRect(x: CGFloat(slot) * (Self.tabWidth + Self.gap), y: 0, width: Self.tabWidth, height: tabHeight)
        }
        dropFrames = targets
        dropSlot.isHidden = dropPreview == nil
        if let preview = dropPreview {
            dropSlot.frame = NSRect(x: CGFloat(preview.index) * (Self.tabWidth + Self.gap), y: 0, width: Self.tabWidth, height: tabHeight)
        }
        let move = {
            for id in self.tabIDs {
                guard let control = self.controls[id] else { continue }
                let hidden = id == self.sourceDragID || id == self.dropPreview?.id
                let target = targets[id] ?? self.dropSlot.frame
                control.setFrameSize(target.size) // Resize vertically immediately; only horizontal insertion motion is animated.
                if animated && !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion {
                    control.animator().setFrameOrigin(target.origin)
                    control.animator().alphaValue = hidden ? 0 : 1
                } else {
                    control.setFrameOrigin(target.origin)
                    control.alphaValue = hidden ? 0 : 1
                }
            }
        }
        NSAnimationContext.runAnimationGroup { context in
            context.duration = 0.24
            context.timingFunction = CAMediaTimingFunction(controlPoints: 0.22, 0.85, 0.25, 1)
            move()
        }
    }

    /// Lay out whole tabs and the pager without shrinking, cutting labels, or exposing scrollbars.
    override func layout() {
        super.layout()
        if let window {
            // The public contentLayoutRect is in window coordinates. Converting its top edge to
            // strip coordinates accounts for AppKit's toolbar margins and fullscreen presentation.
            contentEdge = min(0, convert(NSPoint(x: 0, y: window.contentLayoutRect.maxY), from: nil).y)
        } else { contentEdge = 0 }
        let stride = Self.tabWidth + Self.gap
        // A foreign tab adds a temporary slot; local reordering replaces the source slot.
        let previewCount = tabIDs.count + ((dropPreview != nil && !tabIDs.contains(dropPreview!.id)) ? 1 : 0)
        let totalWidth = max(0, CGFloat(previewCount) * stride - Self.gap)
        let overflows = totalWidth > bounds.width
        let available = max(Self.tabWidth, bounds.width - (overflows ? Self.pagerWidth + 12 : 0))
        tabsPerPage = max(1, Int((available + Self.gap) / stride))
        pageCount = max(1, (previewCount + tabsPerPage - 1) / tabsPerPage)
        if dropPreview == nil && (revealSelection || previousCapacity != tabsPerPage) {
            if let selectedIndex = tabIDs.firstIndex(where: { $0 == selectedID }) { currentPage = selectedIndex / tabsPerPage }
            revealSelection = false
        }
        previousCapacity = tabsPerPage
        currentPage = min(currentPage, pageCount - 1) // Closing tabs may remove the last page.
        let width = overflows ? CGFloat(tabsPerPage) * stride - Self.gap : min(totalWidth, bounds.width)
        // Expand the clipping viewport downward while retaining its top and the original toolbar height.
        // Horizontal clipping still hides neighboring pages; the body now meets the terminal content.
        viewport.frame = NSRect(x: 0, y: contentEdge, width: max(0, width), height: bounds.height - contentEdge)
        document.frame = NSRect(x: -CGFloat(currentPage * tabsPerPage) * stride, y: 0, width: totalWidth, height: viewport.bounds.height)
        pager.isHidden = pageCount <= 1
        pager.frame = NSRect(x: bounds.width - Self.pagerWidth, y: 3, width: Self.pagerWidth, height: 26)
        previousButton.frame = NSRect(x: 0, y: 0, width: 26, height: 26)
        pageLabel.frame = NSRect(x: 26, y: 6, width: 44, height: 14)
        nextButton.frame = NSRect(x: 70, y: 0, width: 26, height: 26)
        applyDropFrames(animated: animateNextLayout)
        animateNextLayout = false
        updatePager()
    }

    /// Update both visible and accessible page state, including unavailable edge directions.
    private func updatePager() {
        previousButton.isEnabled = currentPage > 0
        nextButton.isEnabled = currentPage + 1 < pageCount
        previousButton.toolTip = "Previous page of tabs"
        nextButton.toolTip = "Next page of tabs"
        if nextButton.isEnabled {
            let nextFirst = (currentPage + 1) * tabsPerPage + 1
            let nextLast = min(tabIDs.count, nextFirst + tabsPerPage - 1)
            nextButton.toolTip = "Next page · Tabs \(nextFirst)–\(nextLast)"
        }
        pageLabel.stringValue = "\(currentPage + 1) / \(pageCount)"
        pageLabel.setAccessibilityLabel("Tab page \(currentPage + 1) of \(pageCount)")
        pager.layer?.backgroundColor = NSColor.labelColor.withAlphaComponent(0.045).cgColor
        // Off-page controls stay alive for animation but must not receive accessibility focus.
        let first = currentPage * tabsPerPage
        let last = min(tabIDs.count, first + tabsPerPage)
        var visibleControls: [NSView] = []
        for (index, id) in tabIDs.enumerated() {
            guard let view = controls[id] else { continue }
            let shown = index >= first && index < last && id != sourceDragID
            view.setAccessibilityHidden(!shown)
            for child in view.subviews { child.setAccessibilityHidden(!shown) }
            if shown { visibleControls.append(contentsOf: view.subviews) }
        }
        // Explicit children omit off-page controls and the decorative insertion slot from VoiceOver.
        viewport.setAccessibilityElement(true)
        viewport.setAccessibilityRole(.group)
        viewport.setAccessibilityLabel("Visible terminal tabs")
        viewport.setAccessibilityChildren(visibleControls)
    }

    /// Step to an adjacent full strip and animate its translation; no session selection changes.
    func page(by delta: Int, animated: Bool = true) {
        layoutSubtreeIfNeeded() // Resolve capacity before a page action after a window resize.
        let target = min(max(0, currentPage + delta), pageCount - 1)
        guard target != currentPage else { return }
        currentPage = target
        updatePager()
        let origin = NSPoint(x: -CGFloat(currentPage * tabsPerPage) * (Self.tabWidth + Self.gap), y: 0)
        if animated && !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion {
            NSAnimationContext.runAnimationGroup { context in
                context.duration = 0.20
                context.timingFunction = CAMediaTimingFunction(name: .easeInEaseOut)
                document.animator().setFrameOrigin(origin)
            }
        } else { document.setFrameOrigin(origin) }
    }

    /// Horizontal trackpad movement or Shift-wheel snaps one full strip at a time.
    /// Momentum is consumed so a single swipe cannot skip multiple pages unintentionally.
    override func scrollWheel(with event: NSEvent) {
        guard pageCount > 1 else { return }
        guard event.momentumPhase.isEmpty else { return }
        let horizontal = event.scrollingDeltaX
        let shiftedWheel = event.modifierFlags.contains(.shift) && abs(horizontal) < abs(event.scrollingDeltaY)
        guard shiftedWheel || abs(horizontal) > abs(event.scrollingDeltaY) else { return }
        let delta = shiftedWheel ? event.scrollingDeltaY : horizontal
        if event.phase.contains(.began) || event.timestamp - lastGestureTime > 0.35 {
            gestureDistance = 0
            gesturePaged = false
        }
        lastGestureTime = event.timestamp
        guard !gesturePaged else { return }
        gestureDistance += delta
        let threshold: CGFloat = event.hasPreciseScrollingDeltas ? 40 : 2
        if abs(gestureDistance) >= threshold {
            // AppKit reports content motion: negative deltas reveal tabs further to the right.
            page(by: gestureDistance < 0 ? 1 : -1)
            gesturePaged = true
        }
    }

    /// Pager-arrow hit zones stay fixed while neighboring tabs animate underneath the drag.
    func dragPageDirection(at point: NSPoint) -> Int {
        guard !pager.isHidden else { return 0 }
        let local = convert(point, to: pager)
        if previousButton.isEnabled && previousButton.frame.contains(local) { return -1 }
        if nextButton.isEnabled && nextButton.frame.contains(local) { return 1 }
        return 0
    }

    /// The previous chevron steps back one strip without changing terminal focus.
    @objc private func previousPage(_ sender: Any?) { page(by: -1) }
    /// The next chevron reveals the next strip without changing terminal focus.
    @objc private func nextPage(_ sender: Any?) { page(by: 1) }

    /// Key-window changes affect visual emphasis without rebuilding controls or changing metadata.
    func updateWindowAppearance() { for control in controls.values { control.refreshAppearance() } }

    /// Resolve the content boundary again after AppKit attaches the toolbar to a native window.
    override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); needsLayout = true; updateWindowAppearance() }
}
