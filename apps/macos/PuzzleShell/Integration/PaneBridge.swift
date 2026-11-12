import AppKit

extension PaneNode {
    /// Identify either kind of node without exposing Swift enum representation across FFI.
    var nodeID: UInt64 {
        switch self {
        case .leaf(let id): return id
        case .split(let id, _, _, _, _): return id
        }
    }

    /// Emit plain C records in parent-first order, allowing Rust to rebuild the same binary tree.
    var splitRecords: [PuzzleShellSplit] {
        switch self {
        case .leaf: return []
        case .split(let id, let axis, let fraction, let first, let second):
            var record = PuzzleShellSplit() // Zero initialization guarantees that reserved fields are never uninitialized.
            record.split_id = id
            record.first_id = first.nodeID
            record.second_id = second.nodeID
            record.side_by_side = axis ? 1 : 0
            record.fraction = fraction
            return [record] + first.splitRecords + second.splitRecords
        }
    }
}

/// Copy a value snapshot into caller-owned storage; failure never partially changes the destination.
/// Counts are array elements (not bytes); successful size queries do not require a buffer.
nonisolated func copyPaneRecords<T>(_ records: [T], buffer: UnsafeMutablePointer<T>?, capacity: Int,
                                   count: UnsafeMutablePointer<Int>?) -> Int32 {
    guard let count, capacity >= 0, capacity == 0 || buffer != nil else { return 0 }
    count.pointee = records.count
    if buffer == nil && capacity == 0 { return 1 }
    guard capacity >= records.count else { return -1 }
    if let buffer {
        for (index, record) in records.enumerated() { buffer[index] = record } // Native scalar records contain no borrowed Swift references.
    }
    return 1
}

/// Split a live tab's focused pane; select its tab before using real native content geometry.
@_cdecl("puzzle_shell_split_tab")
public nonisolated func puzzleShellSplitTab(_ tabID: UInt64, _ direction: UInt32) -> UInt64 {
    shellCommand(0) {
        guard let direction = PaneDirection(rawValue: direction), let owner = ShellCoordinator.shared.owner(of: tabID),
              owner.window?.attachedSheet == nil else { return 0 }
        owner.select(tabID)
        return owner.selectedTab?.split(direction) ?? 0
    }
}

/// Resolve current ownership dynamically, preserving pane routing after cross-window tab moves.
@_cdecl("puzzle_shell_select_pane")
public nonisolated func puzzleShellSelectPane(_ paneID: UInt64) -> Int32 {
    shellCommand(0) {
        guard let tab = ShellCoordinator.shared.tab(containing: paneID), let owner = ShellCoordinator.shared.owner(of: tab.id) else { return 0 }
        owner.select(tab.id)
        return tab.selectPane(paneID) ? 1 : 0
    }
}

/// Close a pane through the same lifecycle path as the native Close Pane menu command.
@_cdecl("puzzle_shell_close_pane")
public nonisolated func puzzleShellClosePane(_ paneID: UInt64) -> Int32 {
    shellCommand(0) { ShellCoordinator.shared.tab(containing: paneID)?.closePane(paneID) == true ? 1 : 0 }
}

/// Callback-safe selection queries distinguish focused pane identity from window keyboard focus.
@_cdecl("puzzle_shell_get_active_pane")
public nonisolated func puzzleShellGetActivePane(_ tabID: UInt64) -> UInt64 {
    shellQuery(0) { ShellCoordinator.shared.owner(of: tabID)?.tabs.first(where: { $0.id == tabID })?.layout.activeID ?? 0 }
}

/// Return visual leaf order with two-call query semantics matching the existing title getter.
@_cdecl("puzzle_shell_get_pane_ids")
public nonisolated func puzzleShellGetPaneIDs(_ tabID: UInt64, _ buffer: UnsafeMutablePointer<UInt64>?,
                                            _ capacity: Int, _ count: UnsafeMutablePointer<Int>?) -> Int32 {
    let ids: [UInt64]? = shellQuery(nil) { ShellCoordinator.shared.owner(of: tabID)?.tabs.first(where: { $0.id == tabID })?.layout.root.paneIDs }
    guard let ids else { return 0 } // Invalid ID/thread does not write to any Rust-owned output.
    return copyPaneRecords(ids, buffer: buffer, capacity: capacity, count: count)
}

/// Snapshot the value model before copying C records, allowing this getter during layout callbacks.
@_cdecl("puzzle_shell_get_splits")
public nonisolated func puzzleShellGetSplits(_ tabID: UInt64, _ buffer: UnsafeMutablePointer<PuzzleShellSplit>?,
                                           _ capacity: Int, _ count: UnsafeMutablePointer<Int>?) -> Int32 {
    // Pass a Sendable value tree across the isolation helper rather than caller-owned raw pointers.
    let node: PaneNode? = shellQuery(nil) { ShellCoordinator.shared.owner(of: tabID)?.tabs.first(where: { $0.id == tabID })?.layout.root }
    guard let node else { return 0 }
    let records = MainActor.assumeIsolated { node.splitRecords } // shellQuery has already verified main-thread execution.
    return copyPaneRecords(records, buffer: buffer, capacity: capacity, count: count)
}

/// Resize one divider without rebuilding terminal views or shifting keyboard focus.
@_cdecl("puzzle_shell_resize_split")
public nonisolated func puzzleShellResizeSplit(_ tabID: UInt64, _ splitID: UInt64, _ fraction: Double) -> Int32 {
    shellCommand(0) { ShellCoordinator.shared.owner(of: tabID)?.tabs.first(where: { $0.id == tabID })?.resizeSplit(splitID, fraction: fraction) == true ? 1 : 0 }
}

/// Hide only a particular pane's placeholder when its first renderer frame is ready.
@_cdecl("puzzle_shell_set_pane_placeholder_visible")
public nonisolated func puzzleShellSetPanePlaceholder(_ paneID: UInt64, _ visible: Int32) -> Int32 {
    shellCommand(0) {
        guard let pane = ShellCoordinator.shared.tab(containing: paneID)?.panes[paneID] else { return 0 }
        pane.setPlaceholderVisible(visible != 0)
        return 1
    }
}

/// Route a renderer's caret to the exact leaf's IME client, independently of current tab/pane selection.
@_cdecl("puzzle_shell_set_pane_ime_cursor_rect")
public nonisolated func puzzleShellSetPaneIMECursor(_ paneID: UInt64, _ x: Double, _ y: Double, _ width: Double, _ height: Double) -> Int32 {
    shellCommand(0) {
        guard x.isFinite, y.isFinite, width.isFinite, height.isFinite, width >= 0, height >= 0,
              let pane = ShellCoordinator.shared.tab(containing: paneID)?.panes[paneID] else { return 0 }
        pane.imeCursorRect = NSRect(x: x, y: y, width: width, height: height)
        pane.inputContext?.invalidateCharacterCoordinates()
        return 1
    }
}
