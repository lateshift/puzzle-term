// Puzzle's tick callbacks become native services. Borrowed C data has already been copied.
import AppKit
#if PUZZLE_ENABLED

/// Host effects never reenter native pane traversal, create surfaces during tick, or target stale IDs.
extension PuzzleRuntime {
    /// Decode only host events used by this example; terminal facts remain owned by Puzzle.
    func receive(surface: UInt64, name: String, payload: Data) {
        guard let paneID = surfaces.first(where: { $0.value == surface })?.key else { return }
        PuzzleAppProbe.event(name)
        #if PUZZLE_GLUE_TESTS
        eventObserver?(paneID, name, payload)
        #endif
        // Host-fed input is opaque bytes, not JSON. The app's normal PTY path has no such events.
        if name == "transport/input" || name == "transport/reply" { return }
        if HostExtensions.binaryEvent(runtime: self, paneID: paneID, surface: surface, name: name, payload: payload) { return }
        let value = try? JSONSerialization.jsonObject(with: payload, options: [.fragmentsAllowed])
        if name == "surface/inputState" {
            // Null metrics invalidate the diagnostic grid receipt; Puzzle owns all hit-testing.
            cells.removeValue(forKey: paneID)
            if let state = value as? [String: Any], let width = state["cellWidth"] as? NSNumber,
               let height = state["cellHeight"] as? NSNumber,
               let cols = state["cols"] as? NSNumber, let rows = state["rows"] as? NSNumber,
               width.uint32Value > 0, height.uint32Value > 0 {
                cells[paneID] = (width.uint32Value, height.uint32Value, cols.uint32Value, rows.uint32Value)
            }
            return
        }
        // Every queued action rechecks this generation-bearing handle. A close or replay cannot
        // send an old title, caret or clipboard transaction to a newly created leaf.
        PuzzleAppProbe.hostBlock()
        DispatchQueue.main.async { [weak self] in
            guard let self, self.surfaces[paneID] == surface else { return }
            let measuredStart = PuzzleAppProbe.tickStart()
            defer { PuzzleAppProbe.hostEnd(measuredStart, name: name) }
            self.applyHostEvent(paneID: paneID, surface: surface, name: name, value: value)
        }
    }

    /// Perform policy-approved effects after tick returns, where app creation/closure is permitted.
    private func applyHostEvent(paneID: UInt64, surface: UInt64, name: String, value: Any?) {
        let pane = ShellCoordinator.shared.tab(containing: paneID)?.panes[paneID]
        if HostExtensions.fact(paneID: paneID, name: name, value: value) { return }
        switch name {
        case "terminal/titleChanged":
            guard let title = value as? String, let tab = ShellCoordinator.shared.tab(containing: paneID) else { return }
            // Existing validation and metadata notification preserve the user's custom tab name.
            _ = Self.withBytes(title) { puzzle_shell_set_tab_title(tab.id, $0.data, $0.len) }
        case "surface/firstFrame": pane?.setPlaceholderVisible(false)
        case "surface/cursorRect":
            let rect = value as? [Double]
            pane?.imeCursorRect = rect.flatMap { $0.count == 4 ? NSRect(x: $0[0], y: $0[1], width: $0[2], height: $0[3]) : nil } ?? .zero
            pane?.inputContext?.invalidateCharacterCoordinates()
        case "drop/registration", "drop/acceptance", "drop/concluded": applyDropEvent(name, value: value, pane: pane)
        case "notification/show": showNotification(value, paneID: paneID, surface: surface)
        case "notification/close": closeNotification(value, surface: surface)
        case "notification/focus": focusNotificationPane(paneID)
        case "clipboard/write": writeClipboard(value)
        case "clipboard/mime": clipboardMime(value, paneID: paneID, surface: surface)
        case "clipboard/read": pasteClipboard(surface: surface)
        case "link/open":
            if let map = value as? [String: Any], let uri = map["uri"] as? String, let url = URL(string: uri) {
                NSWorkspace.shared.open(url) // Puzzle has already applied its scheme/modifier-click policy.
            }
        case "pointer/shape": setCursor(value as? String ?? "Default", pane: pane)
        case "surface/closeRequested", "session/closeRequested":
            _ = puzzle_shell_close_pane(paneID) // Synchronous surface free runs inside the resulting closing event.
        case "session/exited":
            break // Drained exit is informational. Only closeRequested may bypass a held/abnormal exit screen.
        case "terminal/bell":
            break // Puzzle draws its rate-limited visual bell; the default has no audible bell (design §14.5).
        case "binding/action": applyWindowAction(value, pane: pane)
        default: break // Search/overlays/refresh stay renderer-owned; native find/host refresh are M12 work.
        }
    }

    /// Cocoa publishes both HTML and plain text from the core's policy-authorized copy result.
    private func writeClipboard(_ value: Any?) {
        guard let map = value as? [String: Any], let text = map["text"] as? String else { return }
        let clipboard = NSPasteboard.general
        clipboard.clearContents()
        clipboard.setString(text, forType: .string)
        if let html = map["html"] as? String { clipboard.setString(html, forType: .html) }
    }

    /// Core overlay names and OSC 22 CSS names share native affordances; unknown shapes use text.
    private func setCursor(_ shape: String, pane: TerminalSurfaceView?) {
        switch shape.lowercased() {
        case "pointer": pane?.puzzleCursor = .pointingHand
        case "grab": pane?.puzzleCursor = .openHand
        case "grabbing": pane?.puzzleCursor = .closedHand
        case "crosshair": pane?.puzzleCursor = .crosshair
        case "ew-resize", "col-resize": pane?.puzzleCursor = .resizeLeftRight
        case "ns-resize", "row-resize": pane?.puzzleCursor = .resizeUpDown
        case "not-allowed": pane?.puzzleCursor = .operationNotAllowed
        default: pane?.puzzleCursor = .iBeam
        }
        if let pane { pane.window?.invalidateCursorRects(for: pane) }
    }

    /// Only window actions are native; font/config actions need the normal app-side settings owner (M12).
    private func applyWindowAction(_ value: Any?, pane: TerminalSurfaceView?) {
        guard let map = value as? [String: Any], let name = map["name"] as? String else { return }
        switch name {
        case "newWindow": ShellCoordinator.shared.createWindow()
        case "closeWindow": pane?.window?.performClose(nil)
        case "toggleFullscreen": pane?.window?.toggleFullScreen(nil)
        default: break
        }
    }
}
#endif
