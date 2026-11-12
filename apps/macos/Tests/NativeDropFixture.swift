// Native AppKit drag fixtures: no UI automation, external drag session, file opens or personal pasteboard data.
import AppKit
#if PUZZLE_ENABLED

/// AppKit requests these promised bytes only if production code actually reads a representation.
@MainActor
final class DropDataProvider: NSObject, NSPasteboardItemDataProvider {
    /// Neutral data by native type, including binary bytes that cannot round-trip through String.
    let values: [NSPasteboard.PasteboardType: Data]
    /// Counts prove enter/move/reject never trigger eager user-data access.
    private(set) var reads = 0
    /// A deterministic native ownership race can be injected during the actual drop read.
    var duringRead: (() -> Void)?
    /// Providers are retained by the fixture until the isolated board is released.
    init(_ values: [NSPasteboard.PasteboardType: Data]) { self.values = values }
    /// Implements the OS deferred-data boundary, rather than mocking the Puzzle message path.
    func pasteboard(_ pasteboard: NSPasteboard?, item: NSPasteboardItem, provideDataForType type: NSPasteboard.PasteboardType) {
        reads += 1
        if let data = values[type] { item.setData(data, forType: type) }
        duringRead?()
    }
}

/// Constructible NSDraggingInfo allows native destination methods to be exercised with exact OS facts.
@MainActor
final class NativeDragInfo: NSObject, NSDraggingInfo {
    /// AppKit's destination identity must match the receiving pane's current window.
    var draggingDestinationWindow: NSWindow?
    /// Copy and move use native AppKit bits; tests also restrict them independently.
    var draggingSourceOperationMask: NSDragOperation = [.copy, .move]
    /// Window-base point is converted by the actual flipped destination view.
    var draggingLocation: NSPoint = .zero
    /// Dragging-image geometry is irrelevant to a MIME destination.
    var draggedImageLocation: NSPoint { draggingLocation }
    /// Deprecated image lookup is unused and has no image asset.
    nonisolated var draggedImage: NSImage? { nil }
    /// Unique native board never accesses the user's clipboard.
    let draggingPasteboard = NSPasteboard.withUniqueName()
    /// Nil models an external native source; nonnil exercises rejecting program-originated app drags.
    var draggingSource: Any?
    /// Changing this starts a distinct drag even when types and position are unchanged.
    var draggingSequenceNumber = 101
    /// Required source presentation property; production does not alter images.
    var draggingFormation: NSDraggingFormation = .default
    /// Native image animation is left to AppKit.
    var animatesToDestination = false
    /// Required native item count; no selective image manipulation is needed.
    var numberOfValidItemsForDrop = 0
    /// No spring-loading behavior is implemented by terminal content.
    var springLoadingHighlight: NSSpringLoadingHighlight { .none }
    /// Position the neutral source within the actual first rendered cell.
    init(pane: TerminalSurfaceView) {
        super.init()
        draggingDestinationWindow = pane.window
        let rect = pane.imeCursorRect
        draggingLocation = pane.convert(NSPoint(x: rect.midX, y: rect.midY), to: nil)
    }
    /// Legacy image sliding is irrelevant to these destination-only tests.
    func slideDraggedImage(to screenPoint: NSPoint) {}
    /// Promised files are deliberately not resolved or fetched by the host.
    nonisolated override func namesOfPromisedFilesDropped(atDestination dropDestination: URL) -> [String]? { nil }
    /// No test or production path enumerates dragging images or creates native source sessions.
    func enumerateDraggingItems(options: NSDraggingItemEnumerationOptions = [], for view: NSView?, classes: [AnyClass],
                                searchOptions: [NSPasteboard.ReadingOptionKey: Any] = [:],
                                using block: (NSDraggingItem, Int, UnsafeMutablePointer<ObjCBool>) -> Void) {}
    /// Destination has no spring-loading timer to reset.
    func resetSpringLoading() {}
    /// Release the isolated native resource while still on AppKit's owner thread.
    func close() { draggingPasteboard.releaseGlobally() }
    /// Install lazy neutral alternatives, making premature reads observable.
    func provide(_ values: [NSPasteboard.PasteboardType: Data]) -> DropDataProvider {
        let provider = DropDataProvider(values)
        let item = NSPasteboardItem()
        item.setDataProvider(provider, forTypes: Array(values.keys))
        draggingPasteboard.clearContents()
        precondition(draggingPasteboard.writeObjects([item]))
        return provider
    }
    /// File URLs do not need to exist: a destination may copy their names but must never open them.
    func files(_ paths: [String]) {
        draggingPasteboard.clearContents()
        precondition(draggingPasteboard.writeObjects(paths.map { NSURL(fileURLWithPath: $0) }))
    }
}

/// A real C Session receives native gestures and writes OSC replies only to its original pane.
@MainActor
final class DropWireFixture {
    /// One shared app/render owner, with an actual host-fed terminal for this fixture.
    let runtime: PuzzleRuntime
    /// Separate native window makes closing/reset/sibling effects independent of previous cases.
    let tab: TerminalTab
    /// The original destination remains stable after splitting and tab/window transfers.
    let pane: TerminalSurfaceView
    /// Generation-bearing C handle, never substituted with a sibling's surface.
    let surface: UInt64
    /// Neutral reply bytes retained only in memory.
    private var input = Data()
    /// Other panes must never receive MIME grants or fallback paste input.
    private(set) var otherInput = Data()
    /// ASCII wire metadata/Base64 can be compared without printing payloads.
    var text: String { String(decoding: input, as: UTF8.self) }

    /// Expose a real pane so presented geometry is authoritative for cell-coordinate checks.
    init(_ runtime: PuzzleRuntime) {
        self.runtime = runtime
        tab = ShellCoordinator.shared.createWindow().selectedTab!
        pane = tab.surface
        surface = runtime.surfaces[pane.paneID]!
        runtime.eventObserver = { [weak self] paneID, name, data in
            guard let self else { return }
            NativeTransportWriter.consume(self.runtime, paneID: paneID, name: name, payload: data) {
                if paneID == self.pane.paneID { self.input.append($0) } else { self.otherInput.append($0) }
            }
        }
        // Fix only this neutral fixture's inset, then await its actual caret instead of the view's initial IME placeholder.
        precondition(PuzzleRuntime.withBytes("{\"render\":{\"padding\":4}}") { puzzle_surface_set_config(surface, $0) } == PUZZLE_OK)
        PuzzleRuntimeTests.until("Drop fixture receives presented first-cell geometry") {
            !self.pane.placeholderVisible && runtime.cells[self.pane.paneID] != nil &&
                self.pane.imeCursorRect.minX == 4 && self.pane.imeCursorRect.minY == 4 && self.pane.imeCursorRect.width > 0
        }
    }
    /// Teardown uses the production synchronous close and releases any native acceptance state.
    func close() { ShellCoordinator.shared.owner(of: tab.id)?.window?.close() }
    /// Reset only the observation, preserving actual Session and native drag lifetimes.
    func reset() { input.removeAll(); otherInput.removeAll() }
    /// All small fixture output is accepted through the public host transport API.
    func feed(_ output: String) {
        var accepted = 0
        precondition(PuzzleRuntime.withBytes(output) { puzzle_surface_feed_output(surface, $0, &accepted) } == PUZZLE_OK)
        precondition(accepted == output.utf8.count)
    }
    /// A functional hang guard makes missing asynchronous event delivery fail visibly.
    func wait(_ suffix: String) { PuzzleRuntimeTests.until("Drag protocol reply arrives") { self.text.hasSuffix(suffix) } }
    /// DSR flushes ordered worker input/replies without sleeps or synchronous Session access.
    func barrier(_ output: String = "") { reset(); feed(output + "\u{1b}[5n"); wait("\u{1b}[0n") }
    /// Registration is parsed in Rust; wait for its actual deferred native callback too.
    func register(_ names: String = "text/uri-list text/plain image/png") {
        let before = pane.dropTarget?.generation
        barrier("\u{1b}]72;t=a:i=7;\(names)\u{1b}\\")
        PuzzleRuntimeTests.until("Native registration follows C tick") { self.pane.dropTarget != nil && self.pane.dropTarget?.generation != before }
        reset()
    }
    /// Program decisions pass through the parser and C effect, never direct host policy setters.
    func accept(_ operation: UInt32 = 1, names: String = "") {
        barrier("\u{1b}]72;t=m:o=\(operation);\(names)\u{1b}\\")
        let expected: NSDragOperation = operation == 1 ? .copy : operation == 2 ? .move : []
        PuzzleRuntimeTests.until("Program acceptance reaches AppKit") { self.pane.dropTarget?.operation == expected }
        reset()
    }
    /// Reads exercise the real captured C data path; a hover has no readable representations.
    func read(_ index: Int) -> String {
        barrier("\u{1b}]72;t=r:x=\(index)\u{1b}\\")
        return text
    }
}
#endif
