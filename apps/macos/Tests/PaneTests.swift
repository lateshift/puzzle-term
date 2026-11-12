import AppKit

/// Exercise production topology/sizing/configuration and C linkage without creating any view or window.
@MainActor
enum PaneTests {
    /// Count assertions separately so the existing headless harness can report one combined total.
    private static var checks = 0

    /// Stop at the first violated model/ABI invariant with a useful onboarding-friendly description.
    private static func check(_ condition: @autoclosure () -> Bool, _ message: String) {
        precondition(condition(), message)
        checks += 1
    }

    /// All four directions must preserve the existing leaf and focus the new one at the requested edge.
    private static func testDirections() {
        for direction in PaneDirection.allCases {
            var layout = PaneLayout(initialPaneID: 1)
            check(layout.split(direction: direction, paneID: 2, branchID: 3), "Directional insertion succeeds")
            check(layout.root.paneIDs == (direction.newPaneFirst ? [2, 1] : [1, 2]), "New pane occupies its requested edge")
            check(layout.activeID == 2 && layout.root.branchIDs == [3], "New pane focused and stable divider allocated")
            let record = layout.root.splitRecords[0]
            check(record.side_by_side == (direction.sideBySide ? 1 : 0) && record.fraction == 0.5, "Correct axis and initial equal halves")
            check(record.first_id == layout.root.paneIDs[0] && record.second_id == layout.root.paneIDs[1], "C child order matches visual order")
            check(layout.close(2) && layout.root == .leaf(1) && layout.activeID == 1, "Closing new leaf promotes old leaf")
            check(!layout.close(1), "Final leaf remains until the tab lifecycle closes it")
        }
    }

    /// Repeated mixed-axis nesting must preserve order, uniqueness, proportions and valid selection.
    private static func testNestedMutations() {
        var layout = PaneLayout(initialPaneID: 1)
        var expected: [UInt64] = [1]
        for index in 1...96 {
            let position = (index * 7) % expected.count
            let target = expected[position]
            let paneID = UInt64(index * 2)
            let branchID = paneID + 1
            let direction = PaneDirection.allCases[index % 4]
            check(layout.select(target), "Select a live nested leaf")
            let beforeRecords = layout.root.splitRecords
            check(layout.split(direction: direction, paneID: paneID, branchID: branchID), "Nested split succeeds")
            expected.insert(paneID, at: position + (direction.newPaneFirst ? 0 : 1))
            check(layout.root.paneIDs == expected && layout.activeID == paneID, "Local insertion leaves every neighboring subtree in place")
            check(Set(layout.root.allIDs).count == layout.root.allIDs.count, "Leaf and divider IDs never overlap")
            check(layout.root.branchIDs.count == expected.count - 1, "Full binary tree has one fewer branch than leaves")
            for record in beforeRecords {
                check(layout.root.splitRecords.first(where: { $0.split_id == record.split_id })?.fraction == record.fraction, "Neighbor divider ratio survives a topology rebuild")
            }
            let ratio = 0.2 + Double(index % 5) * 0.1
            check(layout.resize(branchID: branchID, fraction: ratio), "Resize a nested divider")
            check(layout.root.splitRecords.first(where: { $0.split_id == branchID })?.fraction == ratio, "Dragged proportion saved in the value model")
        }
        let snapshot = layout.root
        check(!layout.select(9999) && layout.root == snapshot, "Stale selection changes nothing")
        check(!layout.split(direction: .left, paneID: 1, branchID: 9999) && layout.root == snapshot, "Duplicate leaf rejected")
        check(!layout.split(direction: .left, paneID: 9999, branchID: 3) && layout.root == snapshot, "Duplicate branch rejected")
        check(!layout.split(direction: .left, paneID: 0, branchID: 9999), "Zero identity rejected")
        for fraction in [Double.nan, .infinity, -0.1, 0, 1, 1.1] {
            check(!layout.resize(branchID: 3, fraction: fraction) && layout.root == snapshot, "Invalid ratio leaves topology untouched")
        }
        check(!layout.resize(branchID: 9999, fraction: 0.5), "Stale divider rejected")
        check(!layout.close(9999) && layout.root == snapshot, "Stale close changes nothing")
        _ = layout.select(expected[0])
        layout.cycle(by: -1)
        check(layout.activeID == expected.last, "Backward visual traversal wraps")
        layout.cycle(by: 1)
        check(layout.activeID == expected.first, "Forward visual traversal wraps")
        // Close in an order different from creation: every sibling promotion must leave a valid tree.
        while expected.count > 1 {
            let position = expected.count / 2
            let removed = expected.remove(at: position)
            let activeBefore = layout.activeID
            check(layout.close(removed), "Close an arbitrary deeply nested pane")
            check(layout.root.paneIDs == expected, "Collapse retains traversal order")
            check(layout.root.branchIDs.count == expected.count - 1, "Collapsed tree contains no redundant branches")
            check(expected.contains(layout.activeID), "Focus always resolves to a surviving pane")
            if activeBefore != removed { check(layout.activeID == activeBefore, "Closing another pane preserves focus") }
        }
        check(layout.root == .leaf(expected[0]), "Final tree is one full-content leaf")
    }

    /// Resizing honors complete subtree minima and preferred fractions, including temporary zero layout.
    private static func testSizing() {
        let tree = PaneNode.split(id: 9, sideBySide: true, fraction: 0.25, first: .leaf(1),
                                  second: .split(id: 8, sideBySide: false, fraction: 0.7, first: .leaf(2), second: .leaf(3)))
        check(tree.minimumSize == NSSize(width: 241, height: 181), "Nested minimum adds only along each branch's axis")
        check(PaneSizing.firstExtent(total: 1001, fraction: 0.25, firstMinimum: 120, secondMinimum: 120) == 250, "Saved ratio used when space is sufficient")
        check(PaneSizing.firstExtent(total: 2001, fraction: 0.25, firstMinimum: 120, secondMinimum: 120) == 500, "Window resize preserves ratio rather than fixed pixels")
        check(PaneSizing.firstExtent(total: 301, fraction: 0.01, firstMinimum: 120, secondMinimum: 120) == 120, "First minimum clamps extreme dragging")
        check(PaneSizing.firstExtent(total: 301, fraction: 0.99, firstMinimum: 120, secondMinimum: 120) == 180, "Second minimum clamps extreme dragging")
        check(PaneSizing.firstExtent(total: 0, fraction: 0.5, firstMinimum: 120, secondMinimum: 120) == 0, "Detached zero layout never creates negative extents")
        check(PaneSizing.firstExtent(total: 91, fraction: 0.5, firstMinimum: 120, secondMinimum: 240) == 30, "Transient undersized ancestors scale complete minima proportionally")
        let records = tree.splitRecords
        check(records.map(\.split_id) == [9, 8] && records[0].second_id == 8, "C topology references nested branch IDs parent-first")
        check(records[0].reserved == 0 && records[1].reserved == 0, "Reserved C fields initialized")
    }

    /// Overrides accept readable arrows and explicit modifier names, while malformed input falls back safely.
    private static func testShortcuts() {
        let defaults = [ShellShortcuts.splitLeftDefault, ShellShortcuts.splitRightDefault,
                        ShellShortcuts.splitTopDefault, ShellShortcuts.splitBottomDefault]
        check(Set(defaults.map(\.key)).count == 4 && defaults.allSatisfy { $0.modifiers == [.control, .command] }, "All split edges have distinct defaults")
        check(ShellShortcuts.nextTab.modifiers == [.option, .command], "Existing tab switching stays available")
        check(ShellShortcuts.numberedTabs.map(\.key) == (1...9).map(String.init), "Numbered shortcuts exclude zoomReset's zero")
        check(ShellShortcuts.numberedTabs.allSatisfy { $0.modifiers == .command }, "Numbered shortcuts use Command alone")
        for (count, expected): (Int, [Int?]) in [(3, [0, nil, nil, 2]), (10, [0, 4, 7, 9])] {
            for (number, index) in zip([1, 5, 8, 9], expected) {
                check(ShellShortcuts.tabIndex(number: number, count: count) == index, "Numbered tab destination at three/ten tabs")
            }
        }
        for number in [0, 1, 9, 10] {
            check(ShellShortcuts.tabIndex(number: number, count: 0) == nil, "Empty windows have no numbered destination")
        }
        let fallback = ShellShortcuts.splitLeftDefault
        let custom = ShellShortcuts.configured(key: "RIGHT", modifiers: ["Command", "Option", "Shift"], fallback: fallback)
        check(custom.key == ShellShortcuts.splitRightDefault.key && custom.modifiers == [.command, .option, .shift], "Named arrow and modifier overrides")
        check(ShellShortcuts.configured(key: "K", modifiers: ["control"], fallback: fallback).key == "k", "Printable custom binding normalized")
        for key in ["", "multiple", "\n"] {
            check(ShellShortcuts.configured(key: key, modifiers: ["command"], fallback: fallback).key == fallback.key, "Malformed key uses default")
        }
        check(ShellShortcuts.configured(key: "k", modifiers: ["unknown"], fallback: fallback).key == fallback.key, "Unknown modifier uses default")
        check(ShellShortcuts.configured(key: "k", modifiers: ["shift"], fallback: fallback).key == fallback.key, "Plain typing cannot become a shell command")
    }

    /// Caller-owned arrays follow all-or-nothing copying; C entry points reject unknown pane/tab IDs.
    private static func testBridge() {
        var count = 99
        check(copyPaneRecords([UInt64(2), 1], buffer: nil, capacity: 0, count: &count) == 1 && count == 2, "Pane array size query")
        var buffer: [UInt64] = [123, 123, 123]
        let small = buffer.withUnsafeMutableBufferPointer { copyPaneRecords([UInt64(2), 1], buffer: $0.baseAddress, capacity: 1, count: &count) }
        check(small == -1 && count == 2 && buffer == [123, 123, 123], "Insufficient pane array never partially writes")
        let copied = buffer.withUnsafeMutableBufferPointer { copyPaneRecords([UInt64(2), 1], buffer: $0.baseAddress, capacity: 2, count: &count) }
        check(copied == 1 && buffer == [2, 1, 123], "Exact pane capacity preserves sentinel")
        count = 99
        check(copyPaneRecords([UInt64(2)], buffer: nil, capacity: 1, count: &count) == 0 && count == 99, "Invalid null/capacity pair leaves count untouched")
        check(copyPaneRecords([UInt64](), buffer: nil, capacity: 0, count: &count) == 1 && count == 0, "Single-pane split-array size is zero")
        check(MemoryLayout<PuzzleShellSplit>.size == 40 && MemoryLayout<PuzzleShellEvent>.size == 144, "Pane-aware C record sizes")
        check(puzzle_shell_split_tab(0, PUZZLE_SPLIT_LEFT.rawValue) == 0 && puzzle_shell_split_tab(0, UInt32.max) == 0, "Unknown tab/direction rejected through C")
        check(puzzle_shell_select_pane(0) == 0 && puzzle_shell_close_pane(0) == 0 && puzzle_shell_get_active_pane(0) == 0, "Stale pane commands and selection query")
        count = 99
        check(puzzle_shell_get_pane_ids(0, nil, 0, &count) == 0 && count == 99, "Invalid pane array query leaves output untouched")
        check(puzzle_shell_get_splits(0, nil, 0, &count) == 0 && count == 99, "Invalid topology query leaves output untouched")
        check(puzzle_shell_resize_split(0, 0, 0.5) == 0 && puzzle_shell_set_pane_placeholder_visible(0, 0) == 0, "Stale renderer controls rejected")
        check(puzzle_shell_set_pane_ime_cursor_rect(0, 0, 0, 8, 18) == 0, "Stale pane IME command rejected")
        let semaphore = DispatchSemaphore(value: 0)
        DispatchQueue.global().async {
            var offThreadCount = 99
            precondition(puzzle_shell_get_pane_ids(0, nil, 0, &offThreadCount) == 0 && offThreadCount == 99)
            precondition(puzzle_shell_get_splits(0, nil, 0, &offThreadCount) == 0 && offThreadCount == 99)
            precondition(puzzle_shell_get_active_pane(0) == 0 && puzzle_shell_split_tab(0, 0) == 0)
            precondition(puzzle_shell_select_pane(0) == 0 && puzzle_shell_close_pane(0) == 0)
            precondition(puzzle_shell_resize_split(0, 0, 0.5) == 0 && puzzle_shell_set_pane_placeholder_visible(0, 0) == 0)
            precondition(puzzle_shell_set_pane_ime_cursor_rect(0, 0, 0, 8, 18) == 0)
            semaphore.signal()
        }
        check(semaphore.wait(timeout: .now() + 5) == .success, "All pane API families enforce main-thread policy")
    }

    /// Return the assertion count to the parent headless runner; never start NSApplication.
    static func run() -> Int {
        checks = 0
        testDirections()
        testNestedMutations()
        testSizing()
        testShortcuts()
        testBridge()
        return checks
    }
}
