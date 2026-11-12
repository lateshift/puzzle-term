import AppKit

/// Reject commands off the main thread before accessing any actor-isolated AppKit state.
/// assumeIsolated is safe here only because Thread.isMainThread is checked first.
nonisolated func shellCommand<T: Sendable>(_ failure: T, _ body: @MainActor () -> T) -> T {
    guard Thread.isMainThread else { return failure }
    return MainActor.assumeIsolated {
        guard !ShellBridge.shared.isDelivering else { return failure }
        return body()
    }
}

/// Public Swift visibility also prevents whole-module optimization from removing the C thunk.
/// ABI queries are pure and may be used before AppKit initialization, from any thread.
@_cdecl("puzzle_shell_abi_version")
public nonisolated func puzzleShellABIVersion() -> UInt32 { UInt32(PUZZLE_SHELL_ABI_VERSION) }

/// Attach a callback after the host initializes AppKit; replay makes late attachment deterministic.
@_cdecl("puzzle_shell_set_event_callback")
public nonisolated func puzzleShellSetCallback(_ callback: PuzzleShellEventCallback?, _ context: UnsafeMutableRawPointer?) -> Int32 {
    // Capture the address as a Sendable scalar; no pointer ownership or thread hop occurs.
    let address = context.map { UInt(bitPattern: $0) }
    return shellCommand(0) {
        ShellBridge.shared.install(callback, context: address.flatMap { UnsafeMutableRawPointer(bitPattern: $0) })
        return 1
    }
}

/// Return a stable process-local ID for a window containing one new placeholder tab.
@_cdecl("puzzle_shell_create_window")
public nonisolated func puzzleShellCreateWindow() -> UInt64 {
    shellCommand(0) { ShellCoordinator.shared.createWindow().id }
}

/// Create an independently owned surface in an existing window and select it.
@_cdecl("puzzle_shell_create_tab")
public nonisolated func puzzleShellCreateTab(_ windowID: UInt64) -> UInt64 {
    shellCommand(0) { ShellCoordinator.shared.windows[windowID]?.addTab().id ?? 0 }
}

/// Select a live tab by ID without requiring Rust to remember its current window.
@_cdecl("puzzle_shell_select_tab")
public nonisolated func puzzleShellSelectTab(_ tabID: UInt64) -> Int32 {
    shellCommand(0) {
        guard let owner = ShellCoordinator.shared.owner(of: tabID) else { return 0 }
        owner.select(tabID)
        return 1
    }
}

/// Close every pane before structural tab teardown; each PANE_CLOSING is its final safe native-pointer callback.
@_cdecl("puzzle_shell_close_tab")
public nonisolated func puzzleShellCloseTab(_ tabID: UInt64) -> Int32 {
    shellCommand(0) {
        guard let owner = ShellCoordinator.shared.owner(of: tabID) else { return 0 }
        owner.closeTab(tabID)
        return 1
    }
}

/// Close every session in a window through the same lifecycle path used by its traffic light.
@_cdecl("puzzle_shell_close_window")
public nonisolated func puzzleShellCloseWindow(_ windowID: UInt64) -> Int32 {
    shellCommand(0) {
        guard let owner = ShellCoordinator.shared.windows[windowID] else { return 0 }
        owner.window?.close()
        return 1
    }
}

/// Move a tab without destroying its renderer-facing surface pointers.
@_cdecl("puzzle_shell_move_tab_to_new_window")
public nonisolated func puzzleShellMoveTab(_ tabID: UInt64) -> UInt64 {
    shellCommand(0) { ShellCoordinator.shared.moveToNewWindow(tabID)?.id ?? 0 }
}

/// Copy and validate Rust-provided UTF-8 before changing any native title UI.
@_cdecl("puzzle_shell_set_tab_title")
public nonisolated func puzzleShellSetTitle(_ tabID: UInt64, _ bytes: UnsafePointer<UInt8>?, _ count: Int) -> Int32 {
    // Copy the borrowed buffer synchronously before entering actor-isolated code.
    // The main-thread check precedes dereferencing; failed off-thread commands read no Rust data.
    guard Thread.isMainThread, count >= 0, count <= 4096, count == 0 || bytes != nil,
          let title = String(bytes: UnsafeBufferPointer(start: bytes, count: count), encoding: .utf8) else { return 0 }
    return shellCommand(0) {
        guard let owner = ShellCoordinator.shared.owner(of: tabID) else { return 0 }
        return owner.rename(tabID, title: title) ? 1 : 0
    }
}

/// Hide only the placeholder overlay, leaving the renderer-owned layer untouched.
@_cdecl("puzzle_shell_set_placeholder_visible")
public nonisolated func puzzleShellSetPlaceholder(_ tabID: UInt64, _ visible: Int32) -> Int32 {
    shellCommand(0) {
        guard let tab = ShellCoordinator.shared.owner(of: tabID)?.tabs.first(where: { $0.id == tabID }) else { return 0 }
        tab.surface.setPlaceholderVisible(visible != 0)
        return 1
    }
}

/// Anchor native IME candidates to a renderer-provided caret without sharing Rust internals.
@_cdecl("puzzle_shell_set_ime_cursor_rect")
public nonisolated func puzzleShellSetIMECursor(_ tabID: UInt64, _ x: Double, _ y: Double, _ width: Double, _ height: Double) -> Int32 {
    shellCommand(0) {
        guard x.isFinite, y.isFinite, width.isFinite, height.isFinite, width >= 0, height >= 0,
              let tab = ShellCoordinator.shared.owner(of: tabID)?.tabs.first(where: { $0.id == tabID }) else { return 0 }
        tab.surface.imeCursorRect = NSRect(x: x, y: y, width: width, height: height)
        tab.surface.inputContext?.invalidateCharacterCoordinates() // Reposition an already-open candidate panel.
        return 1
    }
}

/// Allow Rust to request the same identity-preserving transfer/reorder used by native drag/drop.
@_cdecl("puzzle_shell_move_tab")
public nonisolated func puzzleShellMoveTabToWindow(_ tabID: UInt64, _ windowID: UInt64, _ index: UInt32) -> Int32 {
    shellCommand(0) { ShellCoordinator.shared.moveTab(tabID, to: windowID, at: Int(index)) ? 1 : 0 }
}
