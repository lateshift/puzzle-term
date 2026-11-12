// Shared native field-editor policy for terminal-window dialogs and producer sheets.
import AppKit

/// Plain fields use a complete standard AppKit text system, native clipboard actions and undo.
@MainActor
enum ShellFieldEditor {
    /// init(frame:) creates the TextKit storage/layout/container network; a bare nil container cannot edit text.
    static func make() -> NSTextView {
        let editor = NSTextView(frame: .zero)
        editor.isFieldEditor = true
        editor.isRichText = false
        editor.allowsUndo = true
        editor.isAutomaticQuoteSubstitutionEnabled = false
        editor.isAutomaticDashSubstitutionEnabled = false
        editor.isAutomaticTextReplacementEnabled = false
        return editor
    }
}
