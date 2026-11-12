import Foundation

/// Read-only FFI operations may inspect state during callbacks, unlike shellCommand mutations.
/// No dispatch occurs: the main-thread check makes the actor assertion safe and synchronous.
nonisolated func shellQuery<T: Sendable>(_ failure: T, _ body: @MainActor () -> T) -> T {
    guard Thread.isMainThread else { return failure }
    return MainActor.assumeIsolated { body() }
}

/// Copy into Rust-owned memory without exposing a Swift String's temporary storage.
/// The size-query and insufficient-capacity paths never touch the destination bytes.
nonisolated func copyTitleUTF8(_ title: String, buffer: UnsafeMutablePointer<UInt8>?, capacity: Int,
                              length: UnsafeMutablePointer<Int>?) -> Int32 {
    guard capacity >= 0, let length, buffer != nil || capacity == 0 else { return 0 }
    let bytes = Array(title.utf8) // Counts bytes, not grapheme clusters or UTF-16 units.
    length.pointee = bytes.count
    if buffer == nil { return 1 } // A null buffer with zero capacity is a successful size query.
    guard capacity >= bytes.count else { return -1 }
    if !bytes.isEmpty {
        bytes.withUnsafeBufferPointer { source in
            buffer?.update(from: source.baseAddress!, count: source.count) // No trailing NUL is written.
        }
    }
    return 1
}

/// Preserve runtime title updates under a custom name; an empty name removes the override.
@_cdecl("puzzle_shell_set_tab_custom_name")
public nonisolated func puzzleShellSetCustomName(_ tabID: UInt64, _ bytes: UnsafePointer<UInt8>?, _ count: Int) -> Int32 {
    // Validate before trimming so controls cannot disappear into an otherwise accepted name.
    guard Thread.isMainThread, count >= 0, count <= 4096, count == 0 || bytes != nil,
          let name = String(bytes: UnsafeBufferPointer(start: bytes, count: count), encoding: .utf8) else { return 0 }
    return shellCommand(0) {
        guard TabMetadata.validTitle(name), let owner = ShellCoordinator.shared.owner(of: tabID) else { return 0 }
        return owner.updateMetadata(tabID) { $0.setCustomName(name) } ? 1 : 0
    }
}

/// Return a copied naming layer, including display-only disambiguation when explicitly requested.
@_cdecl("puzzle_shell_get_tab_title")
public nonisolated func puzzleShellGetTitle(_ tabID: UInt64, _ kind: UInt32, _ buffer: UnsafeMutablePointer<UInt8>?,
                                           _ capacity: Int, _ length: UnsafeMutablePointer<Int>?) -> Int32 {
    // Only Sendable String snapshots leave the actor closure; output pointers remain on the caller's stack.
    let title: String? = shellQuery(nil) {
        guard let owner = ShellCoordinator.shared.owner(of: tabID), let tab = owner.tabs.first(where: { $0.id == tabID }) else { return nil }
        // Derive from live ownership, including the instant a TAB_MOVED callback runs.
        let display = TabMetadata.displayTitles(owner.tabs.map { ($0.id, $0.title) })[tabID]
        return tab.metadata.title(kind: kind, displayTitle: display)
    }
    guard let title else { return 0 }
    return copyTitleUTF8(title, buffer: buffer, capacity: capacity, length: length)
}

/// Set the optional sRGB accent without conflating transparent black with neutral styling.
@_cdecl("puzzle_shell_set_tab_color")
public nonisolated func puzzleShellSetColor(_ tabID: UInt64, _ rgba: UInt32, _ enabled: Int32) -> Int32 {
    shellCommand(0) {
        guard enabled == 0 || enabled == 1, let owner = ShellCoordinator.shared.owner(of: tabID) else { return 0 }
        return owner.updateMetadata(tabID) { $0.colorRGBA = enabled == 1 ? rgba : nil } ? 1 : 0
    }
}

/// Query color presence and value; both required output pointers belong to the caller.
@_cdecl("puzzle_shell_get_tab_color")
public nonisolated func puzzleShellGetColor(_ tabID: UInt64, _ rgba: UnsafeMutablePointer<UInt32>?,
                                           _ enabled: UnsafeMutablePointer<Int32>?) -> Int32 {
    guard let rgba, let enabled else { return 0 }
    let metadata: TabMetadata? = shellQuery(nil) {
        ShellCoordinator.shared.owner(of: tabID)?.tabs.first(where: { $0.id == tabID })?.metadata
    }
    guard let metadata else { return 0 } // Invalid IDs or threads leave outputs untouched.
    rgba.pointee = metadata.colorRGBA ?? 0
    enabled.pointee = metadata.colorRGBA == nil ? 0 : 1
    return 1
}

/// Use the same collection path as the Window menu, keeping all Rust sessions and surfaces alive.
@_cdecl("puzzle_shell_collect_tabs")
public nonisolated func puzzleShellCollectTabs(_ windowID: UInt64) -> Int32 {
    shellCommand(0) { ShellCoordinator.shared.collectTabs(into: windowID) ? 1 : 0 }
}
