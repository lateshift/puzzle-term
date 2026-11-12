// Headless contracts for native metadata, input, geometry and worker-to-main tick scheduling.
import Foundation
import AppKit
import QuartzCore

/// Observe that host geometry configuration never writes Metal drawable size without constructing any view/window.
/// Match CALayer's nonisolated construction; the harness serializes all accesses on the main thread.
private nonisolated final class DrawableSizeRecorder: CAMetalLayer {
    /// Only the guarded setter below writes this test-only history; checks read it on the same thread.
    var assignedSizes: [NSSize] = []
    /// Keep setter observations local instead of forwarding test writes to Metal's native property.
    private var recordedSize: NSSize = .zero

    /// Fail on an invalid native assignment, rather than relying on Metal's warning or ignored-setter behavior.
    nonisolated override var drawableSize: CGSize {
        get { recordedSize }
        set {
            precondition(Thread.isMainThread, "The regression harness mutates layers only on the main thread")
            precondition(newValue.width > 0 && newValue.height > 0, "Zero extents must never reach Metal")
            assignedSizes.append(newValue)
            recordedSize = newValue // Capture setter intent; the opt-in native integration harness covers the real driver.
        }
    }
}

/// A synchronous C callback proves that read-only queries work while mutation guards are active.
private nonisolated func metadataCallback(_ context: UnsafeMutableRawPointer?, _ event: UnsafePointer<PuzzleShellEvent>?) {
    precondition(event?.pointee.kind == PUZZLE_TAB_METADATA_CHANGED.rawValue)
    // shellQuery must enter the actor even during delivery; commands must never execute their bodies.
    precondition(shellQuery(false) { ShellBridge.shared.isDelivering })
    precondition(shellCommand(false) { true } == false)
    precondition(bridge_probe_reentrant_commands(0, 0) == 1) // C linkage checks every mutation, including new metadata commands.
}

/// Test metadata, keyboard encoding, Metal geometry and the C contract without GUI interaction.
@main
private struct MetadataTests {
    /// Count successful assertions and stop immediately on a regression with a focused description.
    private static var checks = 0

    /// Evaluate actor-isolated model checks on the same main thread used by the exported bridge.
    static func check(_ condition: @autoclosure () -> Bool, _ message: String) {
        precondition(condition(), message)
        checks += 1
    }

    /// Keep runtime titles and custom names separate, including changes hidden beneath overrides.
    static func testNames() {
        var metadata = TabMetadata(fallbackName: "Terminal 7")
        check(metadata.effectiveTitle == "Terminal 7", "Initial fallback")
        metadata.automaticTitle = "server 🧩"
        check(metadata.effectiveTitle == "server 🧩", "Automatic title")
        metadata.setCustomName("  Production  ")
        metadata.automaticTitle = "new process"
        check(metadata.effectiveTitle == "Production", "Custom name survives runtime update")
        check(metadata.title(kind: PUZZLE_TITLE_AUTOMATIC.rawValue, displayTitle: nil) == "new process", "Raw runtime getter")
        check(metadata.title(kind: PUZZLE_TITLE_CUSTOM.rawValue, displayTitle: nil) == "Production", "Custom getter")
        check(metadata.title(kind: PUZZLE_TITLE_EFFECTIVE.rawValue, displayTitle: nil) == "Production", "Effective getter")
        check(metadata.title(kind: PUZZLE_TITLE_DISPLAY.rawValue, displayTitle: "Production · 2") == "Production · 2", "Display getter")
        check(metadata.title(kind: PUZZLE_TITLE_FALLBACK.rawValue, displayTitle: nil) == "Terminal 7", "Fallback getter")
        check(metadata.title(kind: UInt32.max, displayTitle: nil) == nil, "Unknown kind rejected")
        metadata.setCustomName("  ")
        check(metadata.effectiveTitle == "new process" && metadata.customName == nil, "Blank custom name resets")
        check(metadata.title(kind: PUZZLE_TITLE_CUSTOM.rawValue, displayTitle: nil) == "", "Absent custom name returns empty")
        metadata.automaticTitle = " "
        check(metadata.effectiveTitle == "Terminal 7", "Blank automatic uses stable fallback")
        check(TabMetadata.validTitle("👩‍💻 東京"), "Joined emoji and non-Latin names accepted")
        // Count UTF-8 bytes, so 1024 four-byte emoji fit exactly and one extra byte fails.
        let boundary = String(repeating: "🧩", count: 1024)
        check(TabMetadata.validTitle(boundary) && !TabMetadata.validTitle(boundary + "x"), "UTF-8 byte limit")
        for invalid in ["a\nb", "a\rb", "a\tb", "a\0b", "a\u{0085}b", "a\u{2028}b", "a\u{2029}b"] {
            check(!TabMetadata.validTitle(invalid), "Reject single-line violation")
        }
        metadata.colorRGBA = 0
        check(metadata.colorRGBA != nil, "Transparent black is a color, not absence")
        metadata.colorRGBA = 0x12345678
        check(metadata.colorRGBA == 0x12345678, "RGBA bytes preserve alpha")
        let copied = metadata
        metadata.colorRGBA = nil
        check(copied.colorRGBA == 0x12345678 && metadata.colorRGBA == nil, "Owned snapshot and reset")
        let coordinator = ShellCoordinator.shared
        check(coordinator.allocateFallbackName() == "Terminal 1" && coordinator.allocateFallbackName() == "Terminal 2", "Global fallback sequence")
    }

    /// Duplicate suffixes must stay unique, stable across reorder, and separate from stored raw names.
    static func testDisambiguation() {
        let names: [(id: UInt64, title: String)] = [(8, "server"), (2, "server"), (4, "server · 1"), (9, "unique")]
        let labels = TabMetadata.displayTitles(names)
        check(labels[2] == "server · 2" && labels[8] == "server · 3", "Reserved-name collisions skipped")
        check(labels[4] == "server · 1" && labels[9] == "unique", "Real names preserved")
        check(TabMetadata.displayTitles(Array(names.reversed())) == labels, "Reordering does not renumber duplicates")
        check(TabMetadata.displayTitles([(8, "server")])[8] == "server", "Suffix disappears when no duplicate remains")
        // Exercise nested suffix-looking names and many duplicates, not just two identical labels.
        let crowded: [(id: UInt64, title: String)] = (1...180).map { index in
            (UInt64(index), index % 3 == 0 ? "server · \(index / 3)" : "server")
        }
        let crowdedLabels = TabMetadata.displayTitles(crowded)
        check(Set(crowdedLabels.values).count == crowded.count, "All crowded labels unique")
        check(TabMetadata.displayTitles(Array(crowded.reversed())) == crowdedLabels, "Crowded labels stable")
        check(names[0].title == "server", "Generated suffix never changes raw title")
    }

    /// Collection excludes destination tabs, sorts windows by creation identity, and preserves source order.
    static func testCollection() {
        let windows: [(id: UInt64, tabs: [UInt64])] = [(30, [36, 32]), (10, [15, 11]), (20, [22, 21]), (40, [])]
        check(TabMetadata.collectionPlan(destination: 20, windows: windows) == [15, 11, 36, 32], "Collection order")
        check(TabMetadata.collectionPlan(destination: 20, windows: Array(windows.reversed())) == [15, 11, 36, 32], "Registry order irrelevant")
        check(TabMetadata.collectionPlan(destination: 20, windows: [(20, [22, 21])]).isEmpty, "Single window no-op")
        check(TabMetadata.collectionPlan(destination: 20, windows: []).isEmpty, "Empty registry plan")
    }

    /// Verify exact byte lengths, two-call queries, untouched insufficient buffers, and no NUL writes.
    static func testBuffers() {
        let title = "東京 🧩"
        let bytes = Array(title.utf8)
        var length = -1
        check(copyTitleUTF8(title, buffer: nil, capacity: 0, length: &length) == 1 && length == bytes.count, "Size query counts UTF-8")
        var output = [UInt8](repeating: 0xAB, count: bytes.count + 1)
        let copied = output.withUnsafeMutableBufferPointer { copyTitleUTF8(title, buffer: $0.baseAddress, capacity: bytes.count, length: &length) }
        check(copied == 1 && Array(output.prefix(bytes.count)) == bytes, "Exact-capacity copy")
        check(output.last == 0xAB, "No terminator or out-of-bounds write")
        output = [UInt8](repeating: 0xAB, count: bytes.count + 1)
        let small = output.withUnsafeMutableBufferPointer { copyTitleUTF8(title, buffer: $0.baseAddress, capacity: bytes.count - 1, length: &length) }
        check(small == -1 && length == bytes.count && output.allSatisfy { $0 == 0xAB }, "Too-small buffer reports size without partial write")
        length = 99
        check(copyTitleUTF8(title, buffer: nil, capacity: 1, length: &length) == 0 && length == 99, "Null buffer with capacity invalid")
        check(copyTitleUTF8(title, buffer: nil, capacity: -1, length: &length) == 0 && length == 99, "Invalid capacity leaves length untouched")
        check(copyTitleUTF8(title, buffer: nil, capacity: 0, length: nil) == 0, "Length output required")
        check(copyTitleUTF8("", buffer: nil, capacity: 0, length: &length) == 1 && length == 0, "Empty string size query")
    }

    /// Reproduce modifier transitions using real NSEvent objects, without an application or native window.
    static func testKeyboardEvents() {
        // FlagsChanged does not support isARepeat or charactersIgnoringModifiers. The old shared
        // key handler throws an Objective-C exception on these fixtures before assertions can run.
        for flags: NSEvent.ModifierFlags in [.command, [.command, .option], []] {
            let event = NSEvent.keyEvent(with: .flagsChanged, location: .zero, modifierFlags: flags, timestamp: 0,
                                        windowNumber: 0, context: nil, characters: "", charactersIgnoringModifiers: "",
                                        isARepeat: false, keyCode: 55)!
            let payload = TerminalInputEncoding.keyboardEvent(event, windowID: 7, tabID: 9, paneID: 42)!
            check(payload.record.kind == PUZZLE_MODIFIERS_CHANGED.rawValue, "Modifier event kind")
            check(payload.record.code == 55 && payload.record.window_id == 7 && payload.record.tab_id == 9 && payload.record.pane_id == 42, "Modifier event identity and hardware code")
            check(payload.record.detail == 0 && payload.text == nil, "Modifiers have neither repeat nor character payload")
            let expectedFlags: UInt64 = flags.contains(.command) ? (flags.contains(.option) ? 12 : 8) : 0
            check(payload.record.flags == expectedFlags, "Command/Option press and release flags")
        }
        for type: NSEvent.EventType in [.keyDown, .keyUp] {
            let event = NSEvent.keyEvent(with: type, location: .zero, modifierFlags: [.control, .shift], timestamp: 0,
                                        windowNumber: 0, context: nil, characters: "A", charactersIgnoringModifiers: "a",
                                        isARepeat: type == .keyDown, keyCode: 0)!
            let payload = TerminalInputEncoding.keyboardEvent(event, windowID: 7, tabID: 9, paneID: 42)!
            check(payload.record.kind == (type == .keyDown ? PUZZLE_KEY_DOWN : PUZZLE_KEY_UP).rawValue, "Physical key kind")
            check(payload.record.flags == 3 && payload.record.code == 0 && payload.text == "a", "Physical key fields retained")
            check(payload.record.detail == (type == .keyDown ? 1 : 0), "Key repeat information retained")
        }
        let other = NSEvent.otherEvent(with: .applicationDefined, location: .zero, modifierFlags: [], timestamp: 0,
                                      windowNumber: 0, context: nil, subtype: 0, data1: 0, data2: 0)!
        check(TerminalInputEncoding.keyboardEvent(other, windowID: 7, tabID: 9) == nil, "Non-key events rejected before keyboard accessors")
    }

    /// Host layer properties may change across displays, but only Puzzle may configure drawableSize.
    static func testDrawableGeometry() {
        let layer = DrawableSizeRecorder()
        // A persistent pane can inherit a different orientation after reparenting.
        let upright = CALayer()
        let flipped = CALayer()
        flipped.isGeometryFlipped = true
        for (host, scale, gravity): (CALayer, CGFloat, CALayerContentsGravity) in [
            (upright, 1, .topLeft), (flipped, 2, .bottomLeft), (upright, 1, .topLeft)
        ] {
            host.addSublayer(layer)
            TerminalSurfaceGeometry.configureHostLayer(layer, scale: scale)
            check(layer.contentsScale == scale, "Host updates point-to-pixel scale")
            check(layer.contentsGravity == gravity, "D-54 preserves the visual top left across inherited flips")
        }
        check(layer.assignedSizes.isEmpty, "Host configuration never writes drawableSize")
        check(TerminalSurfaceGeometry.pixelExtent(1279.6) == 1280, "Backing extent rounds to integer pixels")
        for invalid: CGFloat in [0, 0.4, -1, .nan, .infinity] {
            check(TerminalSurfaceGeometry.pixelExtent(invalid) == 0, "Empty/invalid geometry reports suspension")
        }
        check(TerminalSurfaceGeometry.pixelExtent(.greatestFiniteMagnitude) == UInt32.max, "Oversized geometry clamps without conversion trap")
    }

    /// UTF-16 surrogate pairs and layout identities cross different ABI coordinate systems.
    static func testPuzzleInputMapping() {
        let text = "漢🧩e\u{301}"
        for (units, bytes): (UInt32, Int) in [(0, 0), (1, 3), (3, 7), (4, 8), (5, 10)] {
            check(PuzzleComposition.byteOffset(text, utf16: units) == bytes, "Preedit caret maps to a UTF-8 boundary")
        }
        for invalid: UInt32 in [2, 6, UInt32.max] {
            check(PuzzleComposition.byteOffset(text, utf16: invalid) == -1, "Unknown/interior/out-of-range caret uses SIZE_MAX")
        }
        let german = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: .option, timestamp: 0,
                                      windowNumber: 0, context: nil, characters: "|", charactersIgnoringModifiers: "7",
                                      isARepeat: false, keyCode: 26)!
        let mapped = PuzzleKeyInput(event: german, committed: "|")
        check(mapped.logical == "|" && mapped.text == "|" && mapped.physical == "Digit7", "Option layout text stays separate from physical identity")
        check(mapped.modifiers == 18 && mapped.location == 0, "Option side and ABI modifier bits")
        let up = NSEvent.keyEvent(with: .keyUp, location: .zero, modifierFlags: [], timestamp: 0,
                                 windowNumber: 0, context: nil, characters: "7", charactersIgnoringModifiers: "7",
                                 isARepeat: false, keyCode: 26)!
        let released = PuzzleKeyInput(releasing: mapped, event: up)
        check(released.logical == "|" && !released.pressed && released.modifiers == 0, "Release retains its press layout after modifier changes")
        let dead = PuzzleKeyInput(event: german, committed: nil)
        check(dead.kind == 2 && PuzzleKeyInput(releasing: dead, event: up).kind == 2, "Dead key release never invents a kitty character")
        let arrow = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [], timestamp: 0,
                                     windowNumber: 0, context: nil, characters: "\u{f702}", charactersIgnoringModifiers: "\u{f702}",
                                     isARepeat: false, keyCode: 123)!
        let named = PuzzleKeyInput(event: arrow, committed: nil)
        check(named.kind == 1 && named.logical == "ArrowLeft" && named.location == 0, "Arrow is a named standard key, not a left-side modifier")
        check(!PuzzleKeyInput.printable("\u{3}") && PuzzleKeyInput.printable("漢字"), "C0 belongs to the key encoder, Unicode commits remain intact")
    }

    /// A concurrent wake burst gets one queued consumer, and a wake during tick gets its successor.
    static func testTickCoalescing() {
        puzzle_tick_clear()
        puzzle_tick_measure(1)
        let wakeStart = puzzle_tick_wakeups()
        let blockStart = puzzle_tick_blocks()
        // The C counters are atomic, so this test needs no shared Swift mutable capture.
        DispatchQueue.concurrentPerform(iterations: 128) { _ in
            puzzle_tick_wakeup()
            if puzzle_tick_claim() != 0 { puzzle_tick_enqueued() }
        }
        check(puzzle_tick_wakeups() - wakeStart == 128, "All concurrent wake attempts observed")
        check(puzzle_tick_blocks() - blockStart == 1, "Concurrent wakes enqueue one pending tick")
        // Production clears at block ENTRY. A producer arriving while tick delivers events
        // must be admitted; clearing only at exit would silently lose this notification.
        puzzle_tick_clear()
        check(puzzle_tick_claim() == 1 && puzzle_tick_claim() == 0, "Wake during delivery admits one successor")
        puzzle_tick_clear()
        puzzle_tick_measure(0)
        puzzle_tick_wakeup()
        puzzle_tick_enqueued()
        check(puzzle_tick_wakeups() - wakeStart == 128 && puzzle_tick_blocks() - blockStart == 1,
              "Ordinary launches do not increment diagnostic counters")
    }

    /// Link through the C declarations, reject unknown IDs, and verify callback and thread policy.
    static func testBridge() {
        check(puzzle_shell_abi_version() == 2 && MemoryLayout<PuzzleShellEvent>.size == 144, "Pane-aware v2 event ABI")
        check(puzzle_shell_set_tab_title(0, nil, 0) == 0, "Invalid title ID")
        check(puzzle_shell_set_tab_custom_name(0, nil, 0) == 0, "Invalid custom-name ID")
        check(puzzle_shell_set_tab_color(0, 0x12345678, 1) == 0 && puzzle_shell_collect_tabs(0) == 0, "Invalid metadata/collection IDs")
        var length = 99
        check(puzzle_shell_get_tab_title(0, PUZZLE_TITLE_EFFECTIVE.rawValue, nil, 0, &length) == 0 && length == 99, "Invalid getter leaves output untouched")
        var rgba: UInt32 = 123
        var enabled: Int32 = 123
        check(puzzle_shell_get_tab_color(0, &rgba, &enabled) == 0 && rgba == 123 && enabled == 123, "Invalid color query leaves outputs untouched")
        check(puzzle_shell_set_event_callback(metadataCallback, nil) == 1, "Install callback with empty registry")
        ShellBridge.shared.emit(ShellBridge.shared.event(PUZZLE_TAB_METADATA_CHANGED)) // No native application or window is needed.
        check(!ShellBridge.shared.isDelivering, "Delivery guard restored")
        check(puzzle_shell_set_event_callback(nil, nil) == 1, "Detach callback")
        let semaphore = DispatchSemaphore(value: 0)
        DispatchQueue.global().async {
            // Off-thread failures happen before actor access or caller-output writes.
            precondition(shellQuery(false) { true } == false && shellCommand(false) { true } == false)
            var offThreadLength = 99
            precondition(puzzle_shell_get_tab_title(0, PUZZLE_TITLE_DISPLAY.rawValue, nil, 0, &offThreadLength) == 0 && offThreadLength == 99)
            precondition(puzzle_shell_set_tab_custom_name(0, nil, 0) == 0 && puzzle_shell_collect_tabs(0) == 0)
            semaphore.signal()
        }
        check(semaphore.wait(timeout: .now() + 5) == .success, "Off-thread bridge checks completed")
    }

    /// Run only deterministic headless checks; do not initialize NSApplication or instantiate any NSWindow.
    static func main() {
        testNames()
        testDisambiguation()
        testCollection()
        testBuffers()
        testKeyboardEvents()
        testDrawableGeometry()
        testPuzzleInputMapping()
        testTickCoalescing()
        testBridge()
        checks += HostExtensionTests.metadata()
        checks += PaneTests.run() // New split checks exercise the production model and ABI without creating native views.
        checks += TabDragPolicyTests.run() // Screen-space tear-off rules are tested with value snapshots, without windows.
        print("PASS: \(checks) headless metadata/input/geometry/pane/drag checks; no windows or computer-use tests.")
    }
}
