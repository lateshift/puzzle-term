import AppKit
import Foundation
import QuartzCore

/// Copy transient FFI fields into owned values so tests never keep invalid event/text pointers.
private struct CapturedEvent: Sendable {
    let kind: UInt32
    let window: UInt64
    let tab: UInt64
    /// Surface/input records route to a particular persistent leaf.
    let pane: UInt64
    let view: UInt
    let layer: UInt
    let width: UInt32
    let height: UInt32
    let scale: Double
    let flags: UInt64
    let code: UInt32
    let text: String
}

/// Own all callback observations on the main actor, matching the documented bridge contract.
@MainActor
private final class Recorder {
    var events: [CapturedEvent] = []
    var checkReentrancy = true
}

/// This C-compatible function cannot capture Swift state, so the test supplies a borrowed context.
private nonisolated func recordEvent(_ context: UnsafeMutableRawPointer?, _ event: UnsafePointer<PuzzleShellEvent>?) {
    precondition(Thread.isMainThread)
    guard let event, let context else { preconditionFailure("Missing callback payload") }
    let value = event.pointee
    precondition(value.abi_version == 2)
    let captured = CapturedEvent(kind: value.kind, window: value.window_id, tab: value.tab_id, pane: value.pane_id,
                                 view: value.ns_view.map { UInt(bitPattern: $0) } ?? 0,
                                 layer: value.metal_layer.map { UInt(bitPattern: $0) } ?? 0,
                                 width: value.width_px, height: value.height_px, scale: value.scale,
                                 flags: value.flags, code: value.code,
                                 text: String(decoding: UnsafeBufferPointer(start: value.text, count: value.text_length), as: UTF8.self))
    let address = UInt(bitPattern: context) // Carry a scalar into the synchronous actor assertion.
    MainActor.assumeIsolated {
        let recorder = Unmanaged<Recorder>.fromOpaque(UnsafeMutableRawPointer(bitPattern: address)!).takeUnretainedValue()
        recorder.events.append(captured)
        if recorder.checkReentrancy {
            precondition(bridge_probe_reentrant_commands(captured.window, captured.tab) == 1)
        }
    }
}

/// A real-AppKit integration harness validates shell lifecycle without requiring a Rust runtime.
@main
private struct ShellTests {
    /// Fail immediately with a focused diagnostic; successful checks are counted at the end.
    @MainActor static func check(_ condition: @autoclosure () -> Bool, _ message: String) {
        precondition(condition(), message)
    }

    /// Find an event for a specific session, independent of incidental AppKit layout notifications.
    @MainActor static func has(_ recorder: Recorder, _ kind: PuzzleShellEventKind, tab: UInt64) -> Bool {
        recorder.events.contains { $0.kind == kind.rawValue && $0.tab == tab }
    }

    /// Opt-in native checks cover leaf lifecycle, actual divider geometry, transfer and closure.
    /// This method is compiled by --compile-only but is never executed by the headless runner.
    @MainActor static func testPanes(_ recorder: Recorder) {
        PuzzleGlue.shared.start() // Enabled harnesses prove real puzzle.h ownership alongside host events.
        #if PUZZLE_ENABLED
        let runtime = PuzzleGlue.shared.runtime as! PuzzleRuntime
        #endif
        let windowID = puzzle_shell_create_window()
        let owner = ShellCoordinator.shared.windows[windowID]!
        let tab = owner.selectedTab!
        let originalID = tab.layout.activeID
        let originalView = tab.surface
        #if PUZZLE_ENABLED
        check(runtime.surfaces[originalID] != nil, "Initial leaf owns a real Puzzle surface")
        #endif
        var created = [originalID]
        for direction in PaneDirection.allCases {
            check(puzzle_shell_select_pane(originalID) == 1, "Choose the leaf receiving the next directional split")
            let paneID = puzzle_shell_split_tab(tab.id, direction.rawValue)
            check(paneID != 0 && puzzle_shell_get_active_pane(tab.id) == paneID, "Directional split focuses a new native pane")
            created.append(paneID)
            check(tab.panes[originalID] === originalView, "Nested splitting preserves existing native surface identity")
            check(recorder.events.contains { $0.kind == PUZZLE_PANE_CREATED.rawValue && $0.pane == paneID }, "Optional host observer receives each leaf creation")
            #if PUZZLE_ENABLED
            let handle = runtime.surfaces[paneID]!
            check(puzzle_surface_focus(handle, 0) == PUZZLE_OK, "New leaf has a live generation-checked C Puzzle surface")
            check(Set(runtime.surfaces.values).count == created.count, "Each leaf owns a distinct Puzzle surface")
            #endif
            check(tab.panes[paneID]?.bounds.width ?? 0 > 0 && tab.panes[paneID]?.bounds.height ?? 0 > 0, "New leaf has a nonzero drawable area")
        }
        var count = 0
        check(puzzle_shell_get_pane_ids(tab.id, nil, 0, &count) == 1 && count == created.count, "C query reports every native pane")
        var ids = [UInt64](repeating: 0, count: count)
        check(ids.withUnsafeMutableBufferPointer { puzzle_shell_get_pane_ids(tab.id, $0.baseAddress, $0.count, &count) } == 1, "Copy C leaf snapshot")
        check(ids == tab.layout.root.paneIDs, "C leaf order mirrors the mounted native tree")
        let split = tab.content.subviews[0] as! PaneSplitView
        let extent = split.isVertical ? split.bounds.width : split.bounds.height
        split.setPosition((extent - split.dividerThickness) * 0.62, ofDividerAt: 0)
        let ratio = tab.layout.root.splitRecords.first { $0.split_id == split.branchID }!.fraction
        check(abs(ratio - 0.62) < 0.01, "Native divider tracking saves its actual dragged proportion")
        let saved = tab.layout.root.splitRecords.map(\.fraction)
        owner.window?.setContentSize(NSSize(width: 1100, height: 700))
        tab.content.layoutSubtreeIfNeeded()
        check(tab.layout.root.splitRecords.map(\.fraction) == saved, "Window resizing retains nested user proportions")
        let preserved = tab.panes
        #if PUZZLE_ENABLED
        let handles = runtime.surfaces
        check(puzzle_shell_set_event_callback(recordEvent, Unmanaged.passUnretained(recorder).toOpaque()) == 1,
              "Replay the optional host observer while real Puzzle surfaces are live")
        check(runtime.surfaces == handles, "Observer replay never creates duplicate Puzzle surfaces")
        #endif
        let movedWindowID = puzzle_shell_move_tab_to_new_window(tab.id)
        check(movedWindowID != 0 && tab.windowID == movedWindowID, "Entire split tab transfers to a new native window")
        for (id, surface) in preserved {
            check(tab.panes[id] === surface && surface.windowID == movedWindowID, "Each moved leaf preserves its view, layer and ownership")
        }
        check(tab.layout.root.splitRecords.map(\.fraction) == saved, "Tab movement preserves every divider proportion")
        #if PUZZLE_ENABLED
        check(runtime.surfaces == handles, "Moving a split tab preserves all real Puzzle handles")
        #endif
        while tab.panes.count > 1 {
            let paneID = tab.layout.activeID
            recorder.events.removeAll()
            #if PUZZLE_ENABLED
            let handle = runtime.surfaces[paneID]!
            #endif
            check(puzzle_shell_close_pane(paneID) == 1 && tab.panes[paneID] == nil, "Pane closure collapses its parent")
            #if PUZZLE_ENABLED
            check(runtime.surfaces[paneID] == nil && puzzle_surface_focus(handle, 0) != PUZZLE_OK,
                  "Closing releases each real Puzzle surface before returning")
            #endif
            check(recorder.events.contains { $0.kind == PUZZLE_PANE_CLOSING.rawValue && $0.pane == paneID }, "Leaf closing supplies its final native handles")
            check(!has(recorder, PUZZLE_TAB_CLOSING, tab: tab.id), "Closing a non-final leaf preserves the tab")
        }
        check(puzzle_shell_close_pane(tab.layout.activeID) == 1, "Last pane uses the complete tab/window close path")
        check(ShellCoordinator.shared.windows.isEmpty, "Pane test tears down all created native windows")
        #if PUZZLE_ENABLED
        check(runtime.surfaces.isEmpty, "Last pane releases its Puzzle surface")
        #endif
        PuzzleGlue.shared.stop()
    }

    /// Create real windows and invoke the public C ABI; do not substitute mock ownership rules.
    @MainActor static func main() {
        let application = NSApplication.shared
        application.setActivationPolicy(.prohibited) // Test windows must not steal the user's focus.
        check(puzzle_shell_abi_version() == 2, "ABI version")
        let recorder = Recorder()
        let context = Unmanaged.passUnretained(recorder).toOpaque()
        check(puzzle_shell_set_event_callback(recordEvent, context) == 1, "Install callback")
        let firstID = bridge_probe_create_window()
        check(firstID != 0, "Create window through C")
        let first = ShellCoordinator.shared.windows[firstID]!
        let original = first.selectedTab!
        let originalView = UInt(bitPattern: Unmanaged.passUnretained(original.surface).toOpaque())
        let originalLayer = UInt(bitPattern: Unmanaged.passUnretained(original.surface.metalLayer).toOpaque())
        check(has(recorder, PUZZLE_TAB_CREATED, tab: original.id), "Initial tab lifecycle")
        check(recorder.events.first?.kind == PUZZLE_WINDOW_CREATED.rawValue, "Window creation precedes tabs")
        check(first.window?.toolbarStyle == .unifiedCompact && first.window?.tabbingMode == .disallowed, "Native title-bar layout")
        check(recorder.events.contains { $0.kind == PUZZLE_SURFACE_CHANGED.rawValue && $0.width > 0 && $0.height > 0 }, "Nonzero surface dimensions")

        let secondID = puzzle_shell_create_tab(firstID)
        check(first.tabs.count == 2 && first.selectedID == secondID, "Create and select second tab")
        check(original.content.superview == nil, "Inactive surface detached but retained")
        check(puzzle_shell_select_tab(original.id) == 1 && first.selectedID == original.id, "Select by stable ID")
        first.cycleTab(by: -1)
        check(first.selectedID == secondID, "Backward tab wrapping")
        first.cycleTab(by: 1)
        check(first.selectedID == original.id, "Forward tab wrapping")

        let title = Array("Session 🧩".utf8)
        check(title.withUnsafeBufferPointer { puzzle_shell_set_tab_title(original.id, $0.baseAddress, $0.count) } == 1, "UTF-8 title accepted")
        check(original.title == "Session 🧩", "Title copied")
        let invalid: [UInt8] = [0xff]
        check(invalid.withUnsafeBufferPointer { puzzle_shell_set_tab_title(original.id, $0.baseAddress, $0.count) } == 0, "Invalid UTF-8 rejected")
        check(puzzle_shell_set_tab_title(original.id, nil, 1) == 0, "Missing title buffer rejected")
        check(puzzle_shell_set_tab_title(original.id, nil, 4097) == 0, "Oversized title rejected")
        check(puzzle_shell_create_tab(UInt64.max) == 0 && puzzle_shell_select_tab(UInt64.max) == 0, "Unknown IDs rejected")
        check(puzzle_shell_set_placeholder_visible(original.id, 0) == 1, "Renderer can hide placeholder")
        check(puzzle_shell_set_placeholder_visible(original.id, 1) == 1, "Placeholder can be restored")

        // The native toolbar must keep both custom tabs and add visible at the minimum window size.
        let normalFrame = first.window!.frame
        first.window!.setFrame(NSRect(x: normalFrame.minX, y: normalFrame.minY, width: 480, height: 350), display: false)
        first.window!.contentView?.superview?.layoutSubtreeIfNeeded()
        first.window!.displayIfNeeded()
        check(first.window!.toolbar?.visibleItems?.count == 2, "Minimum-width native toolbar retains tabs and add: visible=\(first.window!.toolbar?.visibleItems?.map { $0.itemIdentifier.rawValue } ?? []), items=\(first.window!.toolbar?.items.map { $0.view?.frame.width ?? 0 } ?? [])")
        first.window!.setFrame(normalFrame, display: false)

        // Changing bounds forwards real backing pixels without changing Puzzle's drawable configuration.
        let retainedDrawableSize = original.surface.metalLayer.drawableSize
        // No explicit host flush/layout call: frame mutation itself must deliver geometry before the transaction commits.
        CATransaction.begin()
        original.surface.setFrameSize(NSSize(width: 333, height: 222))
        let resized = recorder.events.last { $0.kind == PUZZLE_SURFACE_CHANGED.rawValue && $0.tab == original.id }!
        check(resized.width == UInt32(333 * resized.scale) && resized.height == UInt32(222 * resized.scale), "Backing scale conversion")
        check(original.surface.metalLayer.drawableSize == retainedDrawableSize, "Host resize leaves drawableSize to Puzzle")
        CATransaction.commit()
        check(original.surface.metalLayer.contentsAreFlipped(), "AppKit's flipped terminal view flips its layer contents")
        check(original.surface.metalLayer.contentsGravity == .bottomLeft, "D-54 pins flipped pane contents to the visual top left")

        check(puzzle_shell_set_ime_cursor_rect(original.id, 40, 50, 8, 18) == 1, "Renderer caret geometry accepted")
        check(original.surface.imeCursorRect.origin.x == 40, "Caret stored in point coordinates")
        check(puzzle_shell_set_ime_cursor_rect(original.id, .nan, 0, 8, 18) == 0, "Non-finite caret rejected")
        check(puzzle_shell_set_ime_cursor_rect(original.id, 0, 0, -1, 18) == 0, "Negative caret size rejected")

        // Composition and committed text must stay separate; a cancelled preedit emits an empty value.
        original.surface.setMarkedText("漢", selectedRange: NSRange(location: 1, length: 0), replacementRange: NSRange(location: NSNotFound, length: 0))
        check(original.surface.hasMarkedText(), "IME preedit state")
        original.surface.insertText("漢字🧩", replacementRange: NSRange(location: NSNotFound, length: 0))
        check(!original.surface.hasMarkedText(), "Commit clears preedit")
        check(recorder.events.last?.kind == PUZZLE_TEXT_COMMITTED.rawValue && recorder.events.last?.text == "漢字🧩", "Committed UTF-8 payload")
        check(recorder.events.contains { $0.kind == PUZZLE_COMPOSITION_CHANGED.rawValue && $0.text.isEmpty }, "Preedit cleared event")
        let key = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [.control, .shift], timestamp: 0,
                                  windowNumber: first.window!.windowNumber, context: nil,
                                  characters: "", charactersIgnoringModifiers: "a", isARepeat: true, keyCode: 0)!
        original.surface.keyDown(with: key)
        check(recorder.events.contains { $0.kind == PUZZLE_KEY_DOWN.rawValue && $0.flags == 3 && $0.code == 0 }, "Hardware key modifiers")
        let mouse = NSEvent.mouseEvent(with: .leftMouseDown, location: NSPoint(x: 20, y: 30), modifierFlags: .option,
                                      timestamp: 0, windowNumber: first.window!.windowNumber, context: nil,
                                      eventNumber: 1, clickCount: 1, pressure: 1)!
        original.surface.mouseDown(with: mouse)
        check(has(recorder, PUZZLE_POINTER, tab: original.id), "Pointer forwarded")

        // A transfer must preserve session/pointer identity and emit no destroy/recreate pair.
        recorder.events.removeAll()
        // Reattachment must restore host gravity even when geometry/scale are unchanged.
        original.surface.metalLayer.contentsGravity = .center
        let movedWindow = puzzle_shell_move_tab_to_new_window(original.id)
        check(movedWindow != 0 && movedWindow != firstID, "Move creates an independent window")
        let moved = ShellCoordinator.shared.windows[movedWindow]!.selectedTab!
        check(moved === original && moved.surface.windowID == movedWindow, "Move preserves session object")
        check(UInt(bitPattern: Unmanaged.passUnretained(moved.surface).toOpaque()) == originalView, "Move preserves NSView")
        check(UInt(bitPattern: Unmanaged.passUnretained(moved.surface.metalLayer).toOpaque()) == originalLayer, "Move preserves Metal layer")
        check(moved.surface.metalLayer.contentsAreFlipped() && moved.surface.metalLayer.contentsGravity == .bottomLeft, "Window transfer reapplies visual top-left gravity")
        check(has(recorder, PUZZLE_TAB_MOVED, tab: original.id), "Move lifecycle event")
        check(!has(recorder, PUZZLE_TAB_CREATED, tab: original.id) && !has(recorder, PUZZLE_TAB_CLOSING, tab: original.id), "No surface churn during transfer")

        // Late registration must replay every live tab's pointers and latest title once.
        recorder.events.removeAll()
        check(puzzle_shell_set_event_callback(recordEvent, context) == 1, "Callback replay")
        check(recorder.events.filter { $0.kind == PUZZLE_TAB_CREATED.rawValue }.count == 2, "Replay includes inactive/live sessions")
        check(recorder.events.contains { $0.kind == PUZZLE_TAB_CREATED.rawValue && $0.tab == original.id && $0.text == "Session 🧩" && $0.view == originalView && $0.layer == originalLayer }, "Replay copies title and pointers")

        // Fixed-width pagination must hide off-page accessibility controls without a scrollbar.
        let strip = TitlebarTabStrip(frame: NSRect(x: 0, y: 0, width: 720, height: 32))
        let testTabs = (0..<12).map { TerminalTab(id: UInt64(1000 + $0), windowID: firstID, title: "Pagination \($0)") }
        strip.update(tabs: testTabs, selected: testTabs[0].id)
        strip.layoutSubtreeIfNeeded()
        check(strip.tabsPerPage == 3 && strip.pageCount == 4, "Whole fixed-width pages")
        strip.page(by: 1, animated: false)
        check(strip.currentPage == 1, "Next strip pagination")
        strip.page(by: -1, animated: false)
        check(strip.currentPage == 0, "Previous strip pagination")
        strip.update(tabs: testTabs, selected: testTabs[11].id)
        strip.layoutSubtreeIfNeeded()
        check(strip.currentPage == 3, "Selection reveals its page")
        strip.setFrameSize(NSSize(width: 480, height: 32))
        strip.layoutSubtreeIfNeeded()
        check(strip.tabsPerPage == 1 && strip.currentPage == 11, "Resize preserves readable fixed tab width")
        strip.page(by: 99, animated: false)
        check(strip.currentPage == 11, "Pagination clamps at last page")

        // Preview geometry stays independent of model ownership and uses complete readable slots.
        let previewStrip = TitlebarTabStrip(frame: NSRect(x: 0, y: 0, width: 720, height: 32))
        previewStrip.update(tabs: Array(testTabs.prefix(3)), selected: testTabs[0].id)
        previewStrip.layoutSubtreeIfNeeded()
        previewStrip.previewDrop(tabID: testTabs[0].id, index: 2)
        check(previewStrip.dropFrames[testTabs[1].id]?.minX == 0, "Local preview closes the old source gap")
        check(previewStrip.dropFrames[testTabs[2].id]?.minX == 192, "Local neighbors make room at the new index")
        check(previewStrip.dropFrames[testTabs[0].id] == nil, "Dragged tab replaced by a reserved slot")
        previewStrip.clearDropPreview()
        check(previewStrip.dropFrames[testTabs[0].id]?.minX == 0 && previewStrip.dropFrames[testTabs[2].id]?.minX == 384, "Cancellation restores original spacing")
        previewStrip.previewDrop(tabID: 999999, index: 1)
        check(previewStrip.dropFrames[testTabs[1].id]?.minX == 384 && previewStrip.dropFrames[testTabs[2].id]?.minX == 576, "Foreign tab opens a complete slot")
        check(previewStrip.dropFrames.values.allSatisfy { $0.width == TitlebarTabStrip.tabWidth }, "Insertion never compresses neighboring tabs")
        check(previewStrip.dropIndex(at: NSPoint(x: 210, y: 15), tabID: 999999) == 1, "Gap hit-testing is stable during animation")
        previewStrip.clearDropPreview()

        // Cross-thread commands fail before accessing actor state or a caller-owned pointer.
        let signal = DispatchSemaphore(value: 0)
        let originalID = original.id // Capture a scalar rather than actor-owned session state.
        DispatchQueue.global().async {
            precondition(!Thread.isMainThread)
            precondition(puzzle_shell_create_window() == 0 && puzzle_shell_select_tab(originalID) == 0 &&
                         puzzle_shell_move_tab(originalID, firstID, 0) == 0 &&
                         puzzle_shell_set_event_callback(nil, nil) == 0 && puzzle_shell_abi_version() == 2)
            signal.signal()
        }
        check(signal.wait(timeout: .now() + 5) == .success, "Off-main-thread rejection")

        recorder.events.removeAll()
        check(puzzle_shell_close_tab(original.id) == 1, "Close moved last tab")
        check(ShellCoordinator.shared.windows[movedWindow] == nil, "Last tab closes window")
        let closing = recorder.events.firstIndex { $0.kind == PUZZLE_TAB_CLOSING.rawValue && $0.tab == original.id }!
        let closed = recorder.events.firstIndex { $0.kind == PUZZLE_WINDOW_CLOSED.rawValue && $0.window == movedWindow }!
        let paneClosing = recorder.events.firstIndex { $0.kind == PUZZLE_PANE_CLOSING.rawValue && $0.tab == original.id }!
        check(paneClosing < closing && closing < closed && recorder.events[paneClosing].view == originalView, "Pane pointers retire before tab and window teardown")
        check(puzzle_shell_close_tab(original.id) == 0, "Stale tab rejected")
        check(puzzle_shell_close_window(firstID) == 1, "Whole window closure")
        check(ShellCoordinator.shared.windows.isEmpty, "All controllers removed")

        // Reordering and transfers share the same C transaction as native drops.
        let dragSourceID = puzzle_shell_create_window()
        let dragSource = ShellCoordinator.shared.windows[dragSourceID]!
        let a = dragSource.selectedTab!
        let bID = puzzle_shell_create_tab(dragSourceID)
        let cID = puzzle_shell_create_tab(dragSourceID)
        let focusedSurface = dragSource.selectedTab!.surface
        recorder.events.removeAll()
        check(puzzle_shell_move_tab(a.id, dragSourceID, 2) == 1, "Forward reorder through C ABI")
        check(dragSource.tabs.map(\.id) == [bID, cID, a.id], "Final index after source removal")
        check(dragSource.selectedID == cID && dragSource.window!.firstResponder === focusedSurface, "Reorder preserves selection and responder")
        check(recorder.events.count == 1 && recorder.events[0].kind == PUZZLE_TAB_REORDERED.rawValue && recorder.events[0].code == 2, "Reorder emits only ordering event")
        check(puzzle_shell_move_tab(a.id, dragSourceID, 0) == 1, "Backward reorder")
        check(dragSource.tabs.map(\.id) == [a.id, bID, cID], "Backward order restored")
        recorder.events.removeAll()
        check(puzzle_shell_move_tab(a.id, dragSourceID, 0) == 1 && recorder.events.isEmpty, "No-op reorder emits nothing")
        check(puzzle_shell_move_tab(a.id, dragSourceID, 3) == 0 && recorder.events.isEmpty, "Invalid local index rejected atomically")

        let dragDestinationID = puzzle_shell_create_window()
        let dragDestination = ShellCoordinator.shared.windows[dragDestinationID]!
        let dID = dragDestination.selectedTab!.id
        let eID = puzzle_shell_create_tab(dragDestinationID)
        recorder.events.removeAll()
        check(puzzle_shell_move_tab(a.id, dragDestinationID, 3) == 0, "Invalid foreign insertion rejected")
        check(puzzle_shell_move_tab(a.id, UInt64.max, 0) == 0, "Unknown destination rejected")
        check(dragSource.tabs.map(\.id) == [a.id, bID, cID] && recorder.events.isEmpty, "Failed transfer never detaches source")
        let preservedLayer = a.surface.metalLayer
        check(puzzle_shell_move_tab(a.id, dragDestinationID, 1) == 1, "Transfer into existing window")
        check(dragDestination.tabs.map(\.id) == [dID, a.id, eID] && dragDestination.selectedID == a.id, "Drop uses exact destination slot")
        check(a.surface.metalLayer === preservedLayer && a.surface.windowID == dragDestinationID, "Layer and session identity preserved")
        check(!has(recorder, PUZZLE_TAB_CLOSING, tab: a.id) && !has(recorder, PUZZLE_TAB_CREATED, tab: a.id), "No surface churn on drop")
        check(recorder.events.contains { $0.kind == PUZZLE_TAB_MOVED.rawValue && $0.tab == a.id && $0.code == 1 }, "Rust receives destination index")
        recorder.events.removeAll()
        check(puzzle_shell_set_event_callback(recordEvent, context) == 1, "Replay after reorder")
        check(recorder.events.contains { $0.kind == PUZZLE_TAB_CREATED.rawValue && $0.tab == a.id && $0.code == 1 }, "Replay carries current tab index")
        check(puzzle_shell_close_window(dragSourceID) == 1 && puzzle_shell_close_window(dragDestinationID) == 1, "Drag fixtures cleanup")

        // AppKit's no-operation result distinguishes deliberate content/desktop tear-off from Escape and title-bar drops.
        let tearSourceID = puzzle_shell_create_window()
        let tearSource = ShellCoordinator.shared.windows[tearSourceID]!
        let tearTab = tearSource.selectedTab!
        let openSpace = NSPoint(x: -50000, y: -50000)
        let sourceCenter = NSPoint(x: tearSource.window!.frame.midX, y: tearSource.window!.frame.midY)
        check(TabDragInteraction.finish(id: tearTab.id, at: openSpace, operation: [], cancelled: true, released: true) == nil, "Escape cannot tear off")
        check(TabDragInteraction.finish(id: tearTab.id, at: openSpace, operation: [], cancelled: false, released: false) == nil, "Held button cannot tear off")
        let sourceTitlebar = NSPoint(x: tearSource.window!.frame.midX, y: tearSource.window!.frame.maxY - 10)
        check(TabDragInteraction.finish(id: tearTab.id, at: sourceTitlebar, operation: [], cancelled: false, released: true) == nil, "Title-bar drops cannot accidentally tear off")
        let target = tearSource.window!.contentView!.subviews.first as! TabTearOffTargetView
        check(target.registeredDraggedTypes.contains(TabDragInteraction.pasteboardType), "Content is an actual native destination for local tab drags")
        check(TabDragInteraction.finish(id: tearTab.id, at: openSpace, operation: .move, cancelled: false, released: true) == nil, "Successful local drop cannot also tear off")
        check(ShellCoordinator.shared.windows.count == 1, "Rejected/cancelled drags leave windows unchanged")
        recorder.events.removeAll()
        let tearDestinationID = TabDragInteraction.finish(id: tearTab.id, at: sourceCenter, operation: [], cancelled: false, released: true)!
        let tearDestination = ShellCoordinator.shared.windows[tearDestinationID]!
        check(tearDestination.tabs.count == 1 && tearDestination.selectedTab === tearTab, "Single-tab tear-off preserves original session")
        check(ShellCoordinator.shared.windows[tearSourceID] == nil, "Single-tab tear-off closes empty source")
        check(!has(recorder, PUZZLE_TAB_CLOSING, tab: tearTab.id) && !has(recorder, PUZZLE_TAB_CREATED, tab: tearTab.id), "Tear-off keeps native surface alive")
        let visible = tearDestination.window!.screen!.visibleFrame
        check(visible.contains(tearDestination.window!.frame), "Torn-off window is clamped onto display")
        let anotherID = puzzle_shell_create_tab(tearDestinationID)
        let splitWindowID = TabDragInteraction.finish(id: tearTab.id, at: openSpace, operation: [], cancelled: false, released: true)!
        check(tearDestination.tabs.map(\.id) == [anotherID] && ShellCoordinator.shared.windows[splitWindowID]?.selectedTab === tearTab, "Multi-tab tear-off retains nonempty source")
        check(puzzle_shell_close_window(tearDestinationID) == 1 && puzzle_shell_close_window(splitWindowID) == 1, "Tear-off fixtures cleanup")

        // Option-click has a separate modifier path; normal clicks keep native target/action selection.
        let optionSourceID = puzzle_shell_create_window()
        let optionSource = ShellCoordinator.shared.windows[optionSourceID]!
        let optionTab = optionSource.selectedTab!
        let optionButton = TabDragButton()
        var optionDestination: TerminalWindowController?
        optionButton.onOptionClick = { optionDestination = ShellCoordinator.shared.moveToNewWindow(optionTab.id) }
        optionButton.activateClick(modifiers: [])
        check(optionDestination == nil && ShellCoordinator.shared.windows[optionSourceID] != nil, "Ordinary click does not tear off")
        optionButton.activateClick(modifiers: [.option])
        check(optionDestination?.selectedTab === optionTab && ShellCoordinator.shared.windows[optionSourceID] == nil, "Option-click moves sole existing session into new window")
        check(puzzle_shell_close_window(optionDestination!.id) == 1, "Option-click fixture cleanup")

        // Moving the sole remaining session must close its empty source without destroying the tab.
        let soleWindow = puzzle_shell_create_window()
        let soleTab = ShellCoordinator.shared.windows[soleWindow]!.selectedTab!.id
        let destination = puzzle_shell_move_tab_to_new_window(soleTab)
        check(ShellCoordinator.shared.windows[soleWindow] == nil && destination != 0, "Move sole tab closes source")
        check(puzzle_shell_close_window(destination) == 1, "Close sole moved tab")
        // Keep observation installed through testPanes. Previously it was detached before that
        // test, so its first split assertion necessarily failed despite correct lifecycle emission.
        testPanes(recorder) // Run only in the explicitly invoked native integration harness.
        check(puzzle_shell_set_event_callback(nil, nil) == 1, "Detach callback")
        PuzzleRuntimeTests.run()
        #if PUZZLE_ENABLED
        HostExtensionTests.closed() // A neutral executable tests the approved PTY launch after the shared runtime closes.
        #endif
        print("PASS: native pane splitting/resizing/lifecycle, C ABI, lifecycle, surface identity, replay, input, geometry, fixed-width pagination, insertion/cancellation, reorder/transfer/tear-off/Option-click, native minimum-width toolbar, thread and reentrancy checks")
    }
}
