// Cocoa interpretation produces one key/text transaction; Puzzle owns terminal encoding and bindings.
import AppKit

/// Encode native keyboard events without reading properties unsupported by their event type.
/// This value-only step can be regression-tested without constructing a window or terminal view.
enum TerminalInputEncoding {
    /// Map AppKit modifier flags to the platform-independent bits in the Rust bridge contract.
    static func modifiers(_ flags: NSEvent.ModifierFlags) -> UInt64 {
        var result: UInt64 = 0
        for (flag, bit): (NSEvent.ModifierFlags, UInt64) in [(.shift, 1), (.control, 2), (.option, 4), (.command, 8), (.capsLock, 16), (.function, 32)] {
            if flags.contains(flag) { result |= bit }
        }
        return result
    }

    /// Choose the bridge kind before accessing key-only properties, preventing AppKit assertions.
    static func keyboardEvent(_ event: NSEvent, windowID: UInt64, tabID: UInt64, paneID: UInt64 = 0) -> (record: PuzzleShellEvent, text: String?)? {
        let kind: PuzzleShellEventKind
        let repeatDetail: UInt32
        let text: String?
        switch event.type {
        case .keyDown, .keyUp:
            kind = event.type == .keyDown ? PUZZLE_KEY_DOWN : PUZZLE_KEY_UP
            repeatDetail = event.isARepeat ? 1 : 0 // AppKit permits this property only on actual key events.
            text = event.charactersIgnoringModifiers // Informational characters; committed input remains separate.
        case .flagsChanged:
            kind = PUZZLE_MODIFIERS_CHANGED
            repeatDetail = 0 // A modifier transition has neither a repeat payload nor a character payload.
            text = nil // Reading charactersIgnoringModifiers here also raises an AppKit exception.
        default:
            return nil // Reject non-key events before reading keyCode or keyboard-specific fields.
        }
        var record = ShellBridge.shared.event(kind, window: windowID, tab: tabID, pane: paneID)
        record.code = UInt32(event.keyCode) // Hardware key codes are supported on all three accepted event types.
        record.flags = modifiers(event.modifierFlags)
        record.detail = repeatDetail
        return (record, text)
    }
}

/// Keep Cocoa text-input interpretation separate from rendering-surface lifetime management.
/// No PTY encoding is performed here: ordinary key commits travel with their key; IME commits are separate.
extension TerminalSurfaceView: NSTextInputClient {
    /// Translate modifier flags to fixed bridge bits rather than exposing AppKit's internal mask.
    private func modifiers(_ event: NSEvent) -> UInt64 {
        TerminalInputEncoding.modifiers(event.modifierFlags)
    }

    /// Raw hardware events are for control keys; text insertion comes only from insertText.
    private func emitKey(_ event: NSEvent) {
        guard isLive, let payload = TerminalInputEncoding.keyboardEvent(event, windowID: windowID, tabID: tabID, paneID: paneID) else { return }
        ShellBridge.shared.emit(payload.record, text: payload.text)
    }

    /// Interpret dead keys/IME before sending a combined key message with its printable commit.
    override func keyDown(with event: NSEvent) {
        let wasComposing = hasMarkedText()
        interpretingKey = true
        interpretedText = nil
        interpretKeyEvents([event])
        interpretingKey = false
        emitKey(event) // The optional observer still sees hardware keys; it never owns Puzzle input.
        if wasComposing || hasMarkedText() {
            // Candidate navigation and the composition-ending key belong entirely to the IME.
            if let text = interpretedText {
                let record = ShellBridge.shared.event(PUZZLE_TEXT_COMMITTED, window: windowID, tab: tabID, pane: paneID)
                PuzzleGlue.shared.runtime?.commitText(record, text: text)
            }
        } else {
            let input = PuzzleKeyInput(event: event, committed: interpretedText)
            pressedKeys[event.keyCode] = input
            PuzzleGlue.shared.nativeKey(paneID: paneID, input: input)
        }
        interpretedText = nil
    }

    /// Releases are forwarded independently; they must never insert duplicate text.
    override func keyUp(with event: NSEvent) {
        emitKey(event)
        if let press = pressedKeys.removeValue(forKey: event.keyCode) {
            PuzzleGlue.shared.nativeKey(paneID: paneID, input: PuzzleKeyInput(releasing: press, event: event))
        }
    }

    /// Native menus own window/tab shortcuts; remaining Command chords belong to Puzzle bindings.
    /// AppKit otherwise routes these through key-equivalent dispatch without calling keyDown.
    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        guard isLive, window?.firstResponder === self, event.type == .keyDown,
              event.modifierFlags.contains(.command), PuzzleGlue.shared.runtime != nil else {
            return super.performKeyEquivalent(with: event)
        }
        if NSApp.mainMenu?.performKeyEquivalent(with: event) == true { return true }
        keyDown(with: event)
        return true
    }
    /// Modifier-only transitions allow Rust to track keys without an accompanying character.
    override func flagsChanged(with event: NSEvent) { emitKey(event) }

    /// AppKit invokes commands for arrows, escape, return, and editing actions.
    /// Rust already receives their hardware keys, so swallowing this avoids Cocoa's error beep.
    override func doCommand(by selector: Selector) {}

    /// Commit IME/dead-key output as UTF-8; never send marked composition text to the PTY.
    func insertText(_ string: Any, replacementRange: NSRange) {
        let text = (string as? NSAttributedString)?.string ?? (string as? String) ?? ""
        unmarkText()
        guard isLive, !text.isEmpty else { return }
        if interpretingKey { interpretedText = (interpretedText ?? "") + text }
        ShellBridge.shared.emit(ShellBridge.shared.event(PUZZLE_TEXT_COMMITTED, window: windowID, tab: tabID, pane: paneID), text: text)
    }

    /// Cache AppKit's composition and send its provisional buffer to Puzzle's preedit overlay.
    func setMarkedText(_ string: Any, selectedRange: NSRange, replacementRange: NSRange) {
        markedText = (string as? NSAttributedString) ?? NSAttributedString(string: (string as? String) ?? "")
        markedSelection = selectedRange
        emitComposition()
    }

    /// Clearing preedit is observable even when composition was cancelled without committing.
    func unmarkText() {
        guard markedText.length > 0 else { return }
        markedText = NSAttributedString(string: "")
        markedSelection = NSRange(location: NSNotFound, length: 0)
        emitComposition()
    }

    /// Scope the temporary composition buffer using the same synchronous bridge lifetime as text.
    private func emitComposition() {
        guard isLive else { return }
        var event = ShellBridge.shared.event(PUZZLE_COMPOSITION_CHANGED, window: windowID, tab: tabID, pane: paneID)
        event.code = markedSelection.location == NSNotFound ? UInt32.max : UInt32(clamping: markedSelection.location)
        event.detail = UInt32(clamping: markedSelection.length)
        ShellBridge.shared.emit(event, text: markedText.string)
    }

    /// AppKit uses this to choose between ongoing composition and fresh input.
    func hasMarkedText() -> Bool { markedText.length > 0 }
    /// The only Cocoa document range exposed by a terminal view is its current preedit buffer.
    func markedRange() -> NSRange { NSRange(location: hasMarkedText() ? 0 : NSNotFound, length: markedText.length) }
    /// Selection coordinates are UTF-16, as required by NSTextInputClient.
    func selectedRange() -> NSRange { markedSelection }
    /// Puzzle draws composition styling; the shell supplies plain preedit text and its selection.
    func validAttributesForMarkedText() -> [NSAttributedString.Key] { [] }

    /// Provide bounded preedit text for input methods without exposing terminal scrollback.
    func attributedSubstring(forProposedRange range: NSRange, actualRange: NSRangePointer?) -> NSAttributedString? {
        guard range.location != NSNotFound, range.location <= markedText.length else { return nil }
        let bounded = NSRange(location: range.location, length: min(range.length, markedText.length - range.location))
        actualRange?.pointee = bounded
        return markedText.attributedSubstring(from: bounded)
    }

    /// Convert Rust's latest caret rect to global screen coordinates for native IME candidates.
    /// Until Rust supplies geometry, the shell uses a small placeholder rect near the top-left.
    func firstRect(forCharacterRange range: NSRange, actualRange: NSRangePointer?) -> NSRect {
        actualRange?.pointee = markedRange()
        // A null Puzzle rectangle denotes an offscreen caret, not the pane's top-left screen point.
        guard imeCursorRect != .zero else { return .zero }
        return window?.convertToScreen(convert(imeCursorRect, to: nil)) ?? .zero
    }

    /// A terminal is not a Cocoa editable document, so point-to-document lookup is unavailable.
    func characterIndex(for point: NSPoint) -> Int { NSNotFound }

    /// Track hover within the terminal region even when no mouse button is pressed.
    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        // This view owns all its tracking areas; recreate them with the visible rect after layout.
        for area in trackingAreas { removeTrackingArea(area) }
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseMoved, .mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect], owner: self, userInfo: nil))
    }

    /// Convert window-space pointer data to the shell's documented top-left view coordinates.
    private func emitPointer(_ code: UInt32, event: NSEvent) {
        guard isLive else { return }
        var value = ShellBridge.shared.event(PUZZLE_POINTER, window: windowID, tab: tabID, pane: paneID)
        let point = convert(event.locationInWindow, from: nil)
        preserveReplyDrag(code, event: event, point: point) // Capture before Rust can request an asynchronous export.
        value.code = code
        value.detail = UInt32(clamping: event.buttonNumber)
        value.flags = modifiers(event)
        value.x = point.x
        value.y = point.y
        if code == 0 || code == 1 { pointerClicks = UInt32(clamping: min(event.clickCount, 255)) }
        if code == 0 { heldPointerButtons.insert(event.buttonNumber) }
        if code == 1 { heldPointerButtons.remove(event.buttonNumber) }
        ShellBridge.shared.emit(value)
    }

    /// Clicking content focuses the active terminal before forwarding its left-button press.
    override func mouseDown(with event: NSEvent) { window?.makeFirstResponder(self); emitPointer(0, event: event) }
    /// Complete the matching left-button gesture.
    override func mouseUp(with event: NSEvent) { emitPointer(1, event: event) }
    /// Forward motion while the left button is held.
    override func mouseDragged(with event: NSEvent) { emitPointer(2, event: event) }
    /// Right clicks are terminal input, rather than a shell-owned context menu.
    override func rightMouseDown(with event: NSEvent) { window?.makeFirstResponder(self); emitPointer(0, event: event) }
    /// Complete the matching right-button gesture.
    override func rightMouseUp(with event: NSEvent) { emitPointer(1, event: event) }
    /// Forward motion while the right button is held.
    override func rightMouseDragged(with event: NSEvent) { emitPointer(2, event: event) }
    /// Middle and auxiliary buttons share the same fixed bridge record.
    override func otherMouseDown(with event: NSEvent) { window?.makeFirstResponder(self); emitPointer(0, event: event) }
    /// Complete a middle/auxiliary button gesture.
    override func otherMouseUp(with event: NSEvent) { emitPointer(1, event: event) }
    /// Forward motion with a middle/auxiliary button held.
    override func otherMouseDragged(with event: NSEvent) { emitPointer(2, event: event) }
    /// Hover and drag use one motion kind, with buttonNumber available in detail.
    override func mouseMoved(with event: NSEvent) { emitPointer(2, event: event) }
    /// Leaving clears renderer overlay hover without inventing a terminal mouse-button report.
    override func mouseExited(with event: NSEvent) { emitPointer(3, event: event) }

    /// Preserve precise/line units and momentum phases; Rust decides terminal scroll policy.
    override func scrollWheel(with event: NSEvent) {
        guard isLive else { return }
        var value = ShellBridge.shared.event(PUZZLE_SCROLL, window: windowID, tab: tabID, pane: paneID)
        let point = convert(event.locationInWindow, from: nil)
        value.x = point.x
        value.y = point.y
        value.flags = modifiers(event)
        value.delta_x = event.scrollingDeltaX
        value.delta_y = event.scrollingDeltaY
        value.detail = event.hasPreciseScrollingDeltas ? 1 : 0
        value.phase = UInt64(event.phase.rawValue)
        value.momentum_phase = UInt64(event.momentumPhase.rawValue)
        ShellBridge.shared.emit(value)
    }
}
