import AppKit

/// Native sheets keep renaming attached to the containing window while other windows remain usable.
extension TerminalWindowController {
    /// Editing an inactive tab does not change terminal selection or send input to another session.
    func requestRename(_ tabID: UInt64) {
        guard !ShellBridge.shared.isDelivering, let window, window.attachedSheet == nil,
              let tab = tabs.first(where: { $0.id == tabID }) else { return }
        let alert = NSAlert()
        alert.messageText = "Rename Tab"
        alert.informativeText = "Choose a custom name. Leave it empty to use the automatic title."
        let renameButton = alert.addButton(withTitle: "Rename")
        alert.addButton(withTitle: "Cancel")
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 340, height: 24))
        field.stringValue = tab.metadata.customName ?? tab.title
        field.placeholderString = tab.metadata.automaticName // Show the name that clearing the override will reveal.
        field.setAccessibilityLabel("Custom tab name")
        alert.accessoryView = field
        let validation = TabNameValidation(button: renameButton)
        field.delegate = validation // NSTextField delegates are weak; the completion retains validation below.
        alert.window.initialFirstResponder = field
        alert.beginSheetModal(for: window) { [validation] response in
            withExtendedLifetime(validation) { // Keep the weak text-field delegate alive until the sheet completes.
                guard response == .alertFirstButtonReturn, TabMetadata.validTitle(field.stringValue) else { return }
                // A tab can be moved by a command while its old window's sheet is open; route by stable ID.
                ShellCoordinator.shared.owner(of: tabID)?.updateMetadata(tabID) { $0.setCustomName(field.stringValue) }
            }
        }
        field.selectText(nil) // Typing replaces the whole initial label, matching native rename behavior.
    }
}

/// Prevent invalid names at entry time; no failing rename is silently discarded after confirmation.
@MainActor
private final class TabNameValidation: NSObject, NSTextFieldDelegate {
    /// The default sheet action becomes unavailable for multiline/control or oversized input.
    private let button: NSButton

    /// Keep only the button; the sheet owns the text field and the completion owns this delegate.
    init(button: NSButton) { self.button = button; super.init() }

    /// Validate pasted text as well as typed edits; blank input is an intentional reset.
    func controlTextDidChange(_ notification: Notification) {
        guard let field = notification.object as? NSTextField else { return }
        button.isEnabled = TabMetadata.validTitle(field.stringValue)
        field.toolTip = button.isEnabled ? nil : "Use a single-line name of at most 4096 UTF-8 bytes."
    }
}
