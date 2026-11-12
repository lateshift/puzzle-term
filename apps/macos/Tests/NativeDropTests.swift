// M18.5: actual NSDraggingDestination methods, native boards and C Sessions prove host acceptance/security/lifetime.
import AppKit
#if PUZZLE_ENABLED

/// No drag source, UI automation or OS acceptance cursor is self-certified; the user reviews the live affordance.
@MainActor
enum NativeDropTests {
    /// Run before later tests close the shared original window; each case creates independent native panes.
    static func run(_ runtime: PuzzleRuntime) {
        let observer = runtime.eventObserver
        defer { runtime.eventObserver = observer }
        checkMimeDrop(runtime)
        checkFilesAndFallback(runtime)
        checkRejectAndLeave(runtime)
        checkReplacementAndClose(runtime)
        checkNativeRace(runtime)
        print("PASS: AppKit MIME drag enter/move/accept/copy/move/drop/leave, no hover reads, preferred alternatives, native revision race, pane isolation, reset/exit/close and quoted-path fallback")
    }

    /// Lazy native data is unreadable during hover; an accepted drop copies immutable ordered binary alternatives.
    private static func checkMimeDrop(_ runtime: PuzzleRuntime) {
        let f = DropWireFixture(runtime)
        defer { f.close() }
        f.register("text/plain image/png")
        let drag = NativeDragInfo(pane: f.pane)
        defer { drag.close() }
        let binary = Data([0, 255, 1, 27, 7])
        let provider = drag.provide([.string: Data("neutral drop".utf8), .png: binary])
        precondition(f.pane.draggingEntered(drag).isEmpty, "First native offer is rejected until the program responds")
        f.wait("\u{1b}\\")
        precondition(f.text.contains("t=m:x=0:y=0:") && f.text.contains("text/plain image/png"))
        precondition(provider.reads == 0 && f.read(1).contains(";EPERM"), "Hover discloses no data or read grant")
        f.accept(1, names: "image/png text/plain")
        precondition(f.pane.draggingUpdated(drag) == .copy && provider.reads == 0)
        precondition(f.pane.prepareForDragOperation(drag) && provider.reads == 0)
        f.reset()
        precondition(f.pane.performDragOperation(drag) && provider.reads > 0)
        f.wait("\u{1b}\\")
        precondition(f.text.contains("t=M:") && f.text.contains("image/png text/plain"), "Drop preserves MIME preference order")
        drag.draggingPasteboard.clearContents()
        drag.draggingPasteboard.setString("changed after drop", forType: .string)
        precondition(f.read(1).contains(binary.base64EncodedString()), "Rust retains the actual drop bytes independently of OS lifetime")
        precondition(f.read(2).contains(Data("neutral drop".utf8).base64EncodedString()))
        f.barrier("\u{1b}]72;t=r:o=1\u{1b}\\")
        precondition(f.read(1).contains(";EPERM"), "Conclusion releases the captured representations")
        // AppKit's successful completion must not synthesize a leave that revokes an already captured drop.
        f.pane.draggingEnded(drag)
        precondition(f.pane.dropTarget?.sequence == nil)
    }

    /// A file offer carries escaped URIs/plain paths, no file opens; after unregister ordinary shell quoting remains.
    private static func checkFilesAndFallback(_ runtime: PuzzleRuntime) {
        let f = DropWireFixture(runtime)
        defer { f.close() }
        f.register("text/uri-list text/plain")
        let drag = NativeDragInfo(pane: f.pane)
        defer { drag.close() }
        let paths = ["/tmp/Puzzle drop's file.txt", "/tmp/第二 file.txt"]
        drag.files(paths)
        precondition(f.pane.draggingEntered(drag).isEmpty)
        f.wait("\u{1b}\\")
        precondition(!f.text.contains("/tmp/"), "Only MIME names travel during hover, never file names")
        f.accept(2, names: "text/uri-list text/plain")
        precondition(f.pane.draggingUpdated(drag) == .move, "AppKit move bit must translate from C operation 2")
        precondition(f.pane.performDragOperation(drag))
        f.wait("\u{1b}\\")
        let uris = paths.map { URL(fileURLWithPath: $0).absoluteString }.joined(separator: "\r\n") + "\r\n"
        precondition(f.read(1).contains(Data(uris.utf8).base64EncodedString()))
        precondition(f.read(2).contains(Data(paths.joined(separator: "\n").utf8).base64EncodedString()))
        f.barrier("\u{1b}]72;t=A\u{1b}\\\u{1b}[?2004h")
        PuzzleRuntimeTests.until("Unregistration restores native fallback") { f.pane.dropTarget == nil }
        f.reset()
        precondition(f.pane.draggingEntered(drag) == .copy && f.pane.performDragOperation(drag))
        f.wait("\u{1b}[201~")
        precondition(f.text == "\u{1b}[200~'/tmp/Puzzle drop'\\''s file.txt' '/tmp/第二 file.txt'\u{1b}[201~",
                     "Unregistered file drop goes through Puzzle's bracketed paste with safe shell quoting")
    }

    /// Rejection, leave and forbidden in-app sources never capture data or fall through to path paste.
    private static func checkRejectAndLeave(_ runtime: PuzzleRuntime) {
        let f = DropWireFixture(runtime)
        defer { f.close() }
        f.register("text/plain")
        let drag = NativeDragInfo(pane: f.pane)
        defer { drag.close() }
        let provider = drag.provide([.string: Data("must remain private".utf8)])
        precondition(f.pane.draggingEntered(drag).isEmpty)
        f.wait("\u{1b}\\")
        precondition(!f.pane.performDragOperation(drag) && provider.reads == 0)
        // Rejection queues a legitimate leave; drain it before checking that no data or fallback paste followed.
        f.wait("\u{1b}]72;t=m:x=-1:y=-1:i=7;\u{1b}\\")
        f.barrier()
        precondition(f.text == "\u{1b}[0n", "Rejected drop produces no DATA or ordinary paste")
        _ = f.pane.draggingEntered(drag)
        f.wait("\u{1b}\\")
        f.accept(1)
        f.accept(0)
        precondition(f.pane.draggingUpdated(drag).isEmpty && provider.reads == 0, "Program rejection updates the native operation without reading bytes")
        f.accept(1)
        f.reset()
        drag.draggingLocation.x += 30
        precondition(f.pane.draggingUpdated(drag) == .copy)
        f.wait("\u{1b}\\")
        precondition(f.text.contains("t=m:") && !f.text.contains("text/plain"), "Movement changes coordinates without repeating unchanged MIME names")
        f.reset()
        f.pane.draggingExited(drag)
        f.wait("\u{1b}]72;t=m:x=-1:y=-1:i=7;\u{1b}\\")
        f.barrier("\u{1b}]72;t=m:o=1;text/plain\u{1b}\\")
        precondition(f.pane.dropTarget?.operation.isEmpty == true && provider.reads == 0, "Late feedback after leave is inert")
        drag.draggingSource = NSObject()
        f.reset()
        precondition(f.pane.draggingEntered(drag).isEmpty && !f.pane.performDragOperation(drag))
        f.barrier()
        precondition(f.text == "\u{1b}[0n" && provider.reads == 0, "Program-originated in-process drags cannot create grants")
    }

    /// Surface registration epochs, native sequence changes, reset/exit and synchronous teardown all revoke acceptance.
    private static func checkReplacementAndClose(_ runtime: PuzzleRuntime) {
        let f = DropWireFixture(runtime)
        defer { f.close() }
        f.register("text/plain")
        let drag = NativeDragInfo(pane: f.pane)
        defer { drag.close() }
        let provider = drag.provide([.string: Data("pane-local".utf8)])
        _ = f.pane.draggingEntered(drag)
        f.wait("\u{1b}\\")
        f.accept(1)
        let old = f.pane.dropTarget!.generation
        f.register("text/plain")
        runtime.applyDropEvent("drop/acceptance", value: ["generation": String(old), "operation": 1, "mimes": []], pane: f.pane)
        precondition(f.pane.draggingEntered(drag).isEmpty && provider.reads == 0)
        f.wait("\u{1b}\\")
        f.accept(1)
        drag.draggingSequenceNumber += 1
        precondition(f.pane.draggingUpdated(drag).isEmpty, "A different native drag cannot inherit accepted status")
        f.wait("\u{1b}\\")
        f.accept(1)
        let sibling = f.tab.split(.right)
        precondition(sibling != 0 && f.tab.panes[sibling]?.dropTarget == nil)
        precondition(f.pane.performDragOperation(drag))
        f.wait("\u{1b}\\")
        precondition(f.read(1).contains(Data("pane-local".utf8).base64EncodedString()) && f.otherInput.isEmpty,
                     "Only the registered original leaf receives drop data")
        f.barrier("\u{1b}c")
        PuzzleRuntimeTests.until("RIS removes native registration") { f.pane.dropTarget == nil }
        f.register("text/plain")
        f.barrier()
        precondition(PuzzleRuntime.withBytes("{\"session\":{\"onExit\":\"hold\"}}") { puzzle_surface_set_config(f.surface, $0) } == PUZZLE_OK)
        precondition(PuzzleRuntime.withBytes("") { puzzle_surface_exited(f.surface, 1, 0, $0, 1000) } == PUZZLE_OK)
        PuzzleRuntimeTests.until("Child exit removes native registration") { f.pane.dropTarget == nil }
        precondition(puzzle_shell_close_pane(f.pane.paneID) == 1)
        precondition(!f.pane.performDragOperation(drag) && !f.pane.isLive, "Closed leaf cannot capture native data")
    }

    /// Native ownership changes during capture refuse the entire grant and never substitute ordinary paste.
    private static func checkNativeRace(_ runtime: PuzzleRuntime) {
        let f = DropWireFixture(runtime)
        defer { f.close() }
        f.register("text/plain")
        let drag = NativeDragInfo(pane: f.pane)
        defer { drag.close() }
        let provider = drag.provide([.string: Data("raced bytes".utf8)])
        _ = f.pane.draggingEntered(drag)
        f.wait("\u{1b}\\")
        f.accept(1)
        provider.duringRead = { drag.draggingPasteboard.clearContents() }
        precondition(!f.pane.performDragOperation(drag), "Generation change while fetching OS bytes rejects the drop atomically")
        precondition(f.read(1).contains(";EPERM"))
    }
}
#endif
