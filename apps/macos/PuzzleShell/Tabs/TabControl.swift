// Native fixed-width tab controls keep session actions separate from status presentation.
import AppKit

/// A restrained native title control: fixed width, subtle selection, close affordance on hover.
@MainActor
final class TabControl: NSView {
    /// Separate actions ensure the close button never also selects the session.
    private let selectAction: () -> Void
    /// The owning window handles lifetime and bridge events, not the tab's visual control.
    private let closeAction: () -> Void
    /// Metadata actions route to stable session IDs captured by the strip, rather than selection.
    private let renameAction: () -> Void
    /// Resetting the name leaves the latest runtime title and tab color intact.
    private let resetNameAction: () -> Void
    /// A nil accent restores native neutral styling.
    private let setColorAction: (UInt32?) -> Void
    /// The same context command and Option-click both use the coordinator's tear-off path.
    private let tearOffAction: () -> Void
    /// A narrow leading accent provides identification without recoloring the native tab chrome.
    private let colorAccent = NSView()
    /// Presets are packed sRGB values; Rust may supply any color, including translucent colors.
    private static let colorPresets: [(name: String, rgba: UInt32?)] = [
        ("None", nil), ("Red", 0xE66A65FF), ("Orange", 0xD9943FFF),
        ("Green", 0x63A76AFF), ("Blue", 0x6195D5FF), ("Purple", 0xA180C8FF)
    ]
    /// Active sessions remain visually distinct while other tabs use the native title-bar material.
    private var selected: Bool
    /// Hover makes an inactive tab actionable without painting every tab as a heavy button.
    private var hovered = false
    /// A native label button retains keyboard and accessibility behavior.
    private let titleButton = TabDragButton()
    /// Keep close buttons accessible while visually quiet on inactive tabs.
    private let closeButton = NSButton()
    /// Aggregated pane status occupies a small trailing slot, independent of the user's color accent.
    private let statusIndicator = TabStatusIndicator()

    /// Lay out a small terminal glyph, readable title and compact close action within a fixed tab.
    init(tab: TerminalTab, selected: Bool, displayTitle: String, select: @escaping () -> Void,
         close: @escaping () -> Void, drag: @escaping (NSEvent) -> Void, tearOff: @escaping () -> Void,
         rename: @escaping () -> Void, resetName: @escaping () -> Void, setColor: @escaping (UInt32?) -> Void) {
        selectAction = select
        closeAction = close
        renameAction = rename
        resetNameAction = resetName
        setColorAction = setColor
        tearOffAction = tearOff
        self.selected = selected
        super.init(frame: .zero)
        wantsLayer = true
        titleButton.onDrag = drag
        titleButton.onOptionClick = tearOff
        titleButton.title = tab.title
        titleButton.image = NSImage(systemSymbolName: "terminal", accessibilityDescription: nil)
        titleButton.symbolConfiguration = NSImage.SymbolConfiguration(pointSize: 11, weight: .regular)
        titleButton.imagePosition = .imageLeading
        titleButton.imageHugsTitle = true
        titleButton.target = self
        titleButton.action = #selector(selectTab(_:))
        titleButton.bezelStyle = .inline
        titleButton.isBordered = false
        titleButton.font = .systemFont(ofSize: 12, weight: selected ? .semibold : .regular)
        titleButton.alignment = .left
        titleButton.lineBreakMode = .byTruncatingTail
        titleButton.toolTip = "\(tab.title)\nOption-click to move to a new window"
        titleButton.setAccessibilityLabel("\(tab.title)\(selected ? ", selected tab" : ", tab")")
        closeButton.image = NSImage(systemSymbolName: "xmark", accessibilityDescription: "Close Tab")
        closeButton.symbolConfiguration = NSImage.SymbolConfiguration(pointSize: 9, weight: .medium)
        closeButton.target = self
        closeButton.action = #selector(closeTab(_:))
        closeButton.isBordered = false
        closeButton.bezelStyle = .inline
        closeButton.toolTip = "Close \(tab.title)"
        closeButton.setAccessibilityLabel("Close \(tab.title)")
        addSubview(titleButton)
        addSubview(closeButton)
        addSubview(statusIndicator)
        colorAccent.wantsLayer = true
        colorAccent.layer?.cornerRadius = 1.5
        colorAccent.setAccessibilityElement(false) // Color supplements the name; it is not an interactive control.
        addSubview(colorAccent)
        configure(tab: tab, selected: selected, displayTitle: displayTitle)
    }

    /// Update selection and text without interrupting an in-flight neighbor animation.
    func configure(tab: TerminalTab, selected: Bool, displayTitle: String) {
        self.selected = selected
        titleButton.title = displayTitle
        titleButton.font = .systemFont(ofSize: 12, weight: selected ? .semibold : .regular)
        let sourceHint = tab.metadata.customName == nil ? "" : "\nAutomatic title: \(tab.metadata.automaticName)"
        let statusHint = tab.extensionStatus.tooltip.isEmpty ? "" : "\n\n\(tab.extensionStatus.tooltip)"
        titleButton.toolTip = "\(displayTitle)\(sourceHint)\(statusHint)\nOption-click to move to a new window"
        titleButton.setAccessibilityLabel("\(displayTitle)\(selected ? ", selected tab" : ", tab")\(tab.extensionStatus.summary.map { ", \($0.label)" } ?? "")")
        statusIndicator.configure(tab.extensionStatus)
        closeButton.toolTip = "Close \(displayTitle)"
        closeButton.setAccessibilityLabel("Close \(displayTitle)")
        colorAccent.isHidden = tab.metadata.colorRGBA == nil
        if let rgba = tab.metadata.colorRGBA {
            // Each byte is an sRGB component; preserve Rust's alpha instead of substituting an opaque accent.
            colorAccent.layer?.backgroundColor = NSColor(srgbRed: CGFloat((rgba >> 24) & 255) / 255,
                green: CGFloat((rgba >> 16) & 255) / 255, blue: CGFloat((rgba >> 8) & 255) / 255,
                alpha: CGFloat(rgba & 255) / 255).cgColor
        }
        installContextMenu(metadata: tab.metadata)
        updateAppearance()
        needsLayout = true // Status changes title capacity, never tab width or pagination.
    }

    /// Nib construction cannot provide the required session actions.
    required init?(coder: NSCoder) { fatalError("Tab controls require session actions") }
    /// Fixed manual frames avoid intrinsic-title widths silently compressing or expanding tabs.
    override func layout() {
        super.layout()
        // Position controls from the top so extending the body down to content does not move the label.
        let statusWidth = statusIndicator.isHidden ? 0 : statusIndicator.preferredWidth + 4
        titleButton.frame = NSRect(x: 10, y: bounds.height - 25, width: bounds.width - 42 - statusWidth, height: 22)
        statusIndicator.frame = NSRect(x: bounds.width - 32 - statusWidth + 4, y: bounds.height - 22,
                                       width: max(0, statusWidth - 4), height: 16)
        closeButton.frame = NSRect(x: bounds.width - 27, y: bounds.height - 23, width: 18, height: 18)
        colorAccent.frame = NSRect(x: 3, y: bounds.height - 21, width: 3, height: 14) // Retain the independent Rust/user color accent.
    }
    /// Controls consume clicks; the unused native title-bar area remains draggable.
    override var mouseDownCanMoveWindow: Bool { false }

    /// Track only this tab's visible region; off-page tabs cannot accidentally highlight.
    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        for area in trackingAreas { removeTrackingArea(area) }
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect], owner: self, userInfo: nil))
    }
    /// Hover reveals the close affordance and a light native material-compatible background.
    override func mouseEntered(with event: NSEvent) { hovered = true; updateAppearance() }
    /// Restore quiet inactive styling when the pointer leaves the tab.
    override func mouseExited(with event: NSEvent) { hovered = false; updateAppearance() }
    /// Resolve dynamic AppKit colors again when the user changes light/dark appearance.
    override func viewDidChangeEffectiveAppearance() { super.viewDidChangeEffectiveAppearance(); updateAppearance() }

    /// Redraw vector chrome and keep native button text in step with selection and window activation.
    private func updateAppearance() {
        needsDisplay = true // The gradient is custom drawn, rather than a flat CALayer fill or solid border.
        titleButton.contentTintColor = selected && window?.isKeyWindow != false ? .labelColor : .secondaryLabelColor
        closeButton.contentTintColor = .secondaryLabelColor
        closeButton.alphaValue = selected || hovered ? 1 : 0
    }

    /// Share activation refresh with the strip without exposing metadata or interaction internals.
    func refreshAppearance() { updateAppearance() }

    /// Render a top-rounded, open-bottom gradient panel using physical-pixel border widths.
    override func draw(_ dirtyRect: NSRect) {
        guard let context = NSGraphicsContext.current?.cgContext else { return }
        TabChrome.drawTab(in: bounds, selected: selected, hovered: hovered, keyWindow: window?.isKeyWindow ?? true,
                          dark: TabChrome.isDark(effectiveAppearance), scale: window?.backingScaleFactor ?? 2,
                          increasedContrast: NSWorkspace.shared.accessibilityDisplayShouldIncreaseContrast, context: context)
    }

    /// Repaint hairlines when moving between displays with different backing scales.
    override func viewDidChangeBackingProperties() { super.viewDidChangeBackingProperties(); needsDisplay = true }
    /// Delegate a title click to stable-ID selection in the window controller.
    @objc private func selectTab(_ sender: Any?) { selectAction() }
    /// Delegate close to the controller, which guarantees the surface-lifetime callback.
    @objc private func closeTab(_ sender: Any?) { closeAction() }

    /// Right-click anywhere on the tab, including its label, edits that tab without selecting it.
    private func installContextMenu(metadata: TabMetadata) {
        let context = NSMenu()
        context.autoenablesItems = false // Availability is derived from this specific tab, not the responder chain.
        context.addItem(contextItem("Rename Tab…", action: #selector(renameTab(_:))))
        let reset = contextItem("Use Automatic Name", action: #selector(resetName(_:)))
        reset.isEnabled = metadata.customName != nil
        context.addItem(reset)
        let color = NSMenuItem(title: "Tab Color", action: nil, keyEquivalent: "")
        let choices = NSMenu()
        choices.autoenablesItems = false
        for (index, preset) in Self.colorPresets.enumerated() {
            let item = contextItem(preset.name, action: #selector(chooseColor(_:)))
            item.tag = index // Tags select a known preset; RGBA remains independent of menu representation.
            item.state = metadata.colorRGBA == preset.rgba ? .on : .off
            choices.addItem(item)
        }
        color.submenu = choices
        context.addItem(color)
        context.addItem(.separator())
        context.addItem(contextItem("Move Tab to New Window", action: #selector(tearOffTab(_:))))
        context.addItem(contextItem("Close Tab", action: #selector(closeTab(_:))))
        menu = context
        titleButton.menu = context // NSButton owns the label hit region and receives its right-clicks.
        closeButton.menu = context
    }

    /// Give context actions an explicit target so inactive tabs never edit the active session accidentally.
    private func contextItem(_ title: String, action: Selector) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: action, keyEquivalent: "")
        item.target = self
        return item
    }

    /// Open the owning window's native rename sheet.
    @objc private func renameTab(_ sender: Any?) { renameAction() }
    /// Clear only the custom override through the same metadata path used by Rust.
    @objc private func resetName(_ sender: Any?) { resetNameAction() }
    /// Route the menu's preset to optional packed RGBA metadata.
    @objc private func chooseColor(_ sender: NSMenuItem) {
        guard Self.colorPresets.indices.contains(sender.tag) else { return }
        setColorAction(Self.colorPresets[sender.tag].rgba)
    }
    /// Preserve the session and surface when moving into a new independent window.
    @objc private func tearOffTab(_ sender: Any?) { tearOffAction() }
}
