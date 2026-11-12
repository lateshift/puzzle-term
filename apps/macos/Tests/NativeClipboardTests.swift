// M18.3: real pasteboard and C-wire witnesses, with controlled OS failures and revision races.
import AppKit
#if PUZZLE_ENABLED

/// A deterministic OS seam proves failure and ownership races without touching personal clipboard data.
@MainActor
private final class ClipboardRaceBackend: PuzzlePasteboard {
    /// Monotonic test generation changes independently of the data's value.
    var revision: UInt64? = 42
    /// Neutral alternatives used to check preserving the complete request.
    var items = [PuzzleClipboardItem(mime: "text/plain", data: Data("neutral".utf8))]
    /// Force an OS write failure without pretending its attempted update succeeded.
    var writeSucceeds = true
    /// Mutate the generation inside a native read, between the two required observations.
    var duringRead: (() -> Void)?
    /// Count access to prove a pre-read revision mismatch does not fetch any private bytes.
    var reads = 0
    /// MIME metadata does not read contents.
    var names: [String] { items.map(\.mime) }
    /// Simulate an ownership change while the native read is completing.
    func read(_ mime: String) -> Data? {
        reads += 1
        let data = items.first(where: { $0.mime == mime })?.data
        duringRead?()
        return data
    }
    /// A failure leaves the previous content intact and must produce EIO on the wire.
    func replace(_ items: [PuzzleClipboardItem]) -> Bool {
        guard writeSucceeds else { return false }
        self.items = items
        revision = (revision ?? 0) + 1
        return true
    }
}

/// A real host-transport Session drives every callback; only the OS clipboard can be isolated.
@MainActor
enum NativeClipboardTests {
    /// Run before status tests close the existing owner and its final surface.
    static func run(_ runtime: PuzzleRuntime, owner: TerminalWindowController) {
        let delegate = ShellAppDelegate()
        let oldMenu = NSApp.mainMenu, oldWindows = NSApp.windowsMenu
        delegate.installMenus()
        defer { NSApp.mainMenu = oldMenu; NSApp.windowsMenu = oldWindows }
        let board = NSPasteboard.withUniqueName()
        defer { board.releaseGlobally() }
        let native = PuzzleNativePasteboard(board)
        let oldClipboard = runtime.clipboard
        let oldObserver = runtime.eventObserver
        defer { runtime.clipboard = oldClipboard; runtime.eventObserver = oldObserver }
        runtime.clipboard = PuzzleClipboard(native)
        let pane = owner.selectedTab!.surface
        let fixture = ClipboardWireFixture(runtime, pane: pane)
        checkNativeAlternatives(fixture, board: board)
        checkPasteAndGrants(fixture, board: board)
        checkRacesAndFailures(fixture)
        checkClosedSurface(runtime, owner: owner)
        print("PASS: asynchronous MIME completion, native text/HTML/PNG alternatives, Cmd-V paste/listing, single-use grants, revision races, OS failure and close cancellation")
    }

    /// Writes, reads and listings retain exact requested alternatives and binary PNG bytes.
    private static func checkNativeAlternatives(_ fixture: ClipboardWireFixture, board: NSPasteboard) {
        let text = Data("Puzzle MIME clipboard".utf8)
        let html = Data("<b>Puzzle MIME clipboard</b>".utf8)
        // Authored neutral 1×1 PNG; binary zero/non-UTF-8 bytes must survive without decoding to text.
        let png = Data(base64Encoded: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC")!
        fixture.write([("text/plain", text), ("text/html", html), ("image/png", png)], status: "DONE")
        precondition(board.string(forType: .string) == String(data: text, encoding: .utf8))
        precondition(board.data(forType: .html) == html && board.data(forType: .png) == png,
                     "One native item preserves all write alternatives")
        let revision = board.changeCount
        fixture.write([("application/x-unavailable", Data([1])), ("text/plain", Data("discarded".utf8))], status: "ENOSYS")
        precondition(board.changeCount == revision && board.data(forType: .png) == png,
                     "Unsupported alternatives reject the complete write before clearing")
        fixture.write([("text/plain", Data([255]))], status: "EINVAL")
        precondition(board.changeCount == revision, "Invalid UTF-8 is rejected before replacing any alternative")
        fixture.setConfig("{\"clipboard\":{\"osc52Read\":\"allow\"}}")
        let reply = fixture.read(["image/png", "text/html", "text/plain"])
        for bytes in [png, html, text] {
            precondition(reply.contains(bytes.base64EncodedString()), "Every requested read representation survives the C answer")
        }
        let pngOnly = fixture.read(["image/png", "application/x-unavailable"])
        precondition(pngOnly.contains(png.base64EncodedString()) && !pngOnly.contains(html.base64EncodedString()) &&
                     !pngOnly.contains(text.base64EncodedString()), "Read answers contain only requested available alternatives")
        let listing = fixture.read(["."])
        precondition(listing.contains(Data("text/plain text/html image/png".utf8).base64EncodedString()))
        fixture.setConfig("{\"clipboard\":{\"osc52Read\":\"deny\"}}")
        _ = fixture.read(["text/plain"], status: "EPERM")
        fixture.write([], status: "DONE")
        precondition((board.types ?? []).isEmpty, "Empty completed MIME write clears the pasteboard")
    }

    /// The actual native shortcut uses the same core binding in normal, bracketed and paste-event modes.
    private static func checkPasteAndGrants(_ fixture: ClipboardWireFixture, board: NSPasteboard) {
        board.clearContents()
        precondition(board.setString("hello\nworld", forType: .string))
        fixture.barrier("\u{1b}[?1000l\u{1b}[?2004h")
        fixture.reset()
        fixture.pasteKey()
        fixture.wait("\u{1b}[201~")
        precondition(fixture.text == "\u{1b}[200~hello\rworld\u{1b}[201~", "Normal paste keeps Puzzle's sanitizing/bracketing path")
        fixture.barrier("\u{1b}[?5522h")
        let password = fixture.grant()
        let reply = fixture.read(["text/plain"], password: password)
        precondition(reply.contains(Data("hello\nworld".utf8).base64EncodedString()))
        _ = fixture.read(["text/plain"], password: password, status: "EPERM")
        // Restoring the same contents still increments changeCount and cannot revive an old grant.
        let stale = fixture.grant()
        board.clearContents()
        precondition(board.setString("changed", forType: .string))
        board.clearContents()
        precondition(board.setString("hello\nworld", forType: .string))
        _ = fixture.read(["text/plain"], password: stale, status: "EPERM")
        let inputExpired = fixture.grant()
        fixture.reset()
        precondition(PuzzleRuntime.withBytes("x") { puzzle_surface_text(fixture.surface, $0) } == PUZZLE_OK)
        fixture.wait("x")
        _ = fixture.read(["text/plain"], password: inputExpired, status: "EPERM")
        fixture.barrier("\u{1b}[?5522l\u{1b}[?2004l")
    }

    /// Controlled backend races prove both checks, and native failure reaches the actual protocol reply.
    private static func checkRacesAndFailures(_ fixture: ClipboardWireFixture) {
        let backend = ClipboardRaceBackend()
        fixture.runtime.clipboard = PuzzleClipboard(backend)
        backend.writeSucceeds = false
        fixture.write([("text/plain", Data("must fail".utf8))], status: "EIO")
        precondition(backend.items.first?.data == Data("neutral".utf8))
        fixture.barrier("\u{1b}[?5522h")
        let before = fixture.grant()
        backend.revision = 43
        let reads = backend.reads
        _ = fixture.read(["text/plain"], password: before, status: "EPERM")
        precondition(backend.reads == reads, "Mismatched grant denies before reading native bytes")
        let during = fixture.grant()
        backend.duringRead = { backend.revision = 44 }
        let answer = fixture.read(["text/plain"], password: during, status: "EPERM")
        precondition(!answer.contains(Data("neutral".utf8).base64EncodedString()), "Post-I/O mismatch discloses no data")
        backend.duringRead = nil
        // The ordinary-text snapshot also refuses a race rather than creating a grant for mixed generations.
        backend.duringRead = { backend.revision = (backend.revision ?? 0) + 1 }
        precondition(fixture.runtime.clipboard.pasteSnapshot() == nil)
        backend.duringRead = nil
        fixture.barrier("\u{1b}[?5522l")
    }

    /// A callback queued for a closed pane must not perform OS work or complete against a reused handle.
    private static func checkClosedSurface(_ runtime: PuzzleRuntime, owner: TerminalWindowController) {
        let backend = ClipboardRaceBackend()
        runtime.clipboard = PuzzleClipboard(backend)
        let tab = owner.addTab()
        let pane = tab.surface
        let surface = runtime.surfaces[pane.paneID]!
        let request: [String: Any] = ["token": "18446744073709551614", "operation": "write", "primary": false,
                                    "revision": NSNull(), "items": [["type": "text/plain", "data": [65]]]]
        runtime.clipboardMime(request, paneID: pane.paneID, surface: surface)
        precondition(backend.items.first?.data == Data("neutral".utf8), "Async enqueue must not execute native write inline")
        precondition(puzzle_shell_close_pane(pane.paneID) == 1)
        var drained = false
        DispatchQueue.main.async { drained = true }
        PuzzleRuntimeTests.until("Deferred clipboard work drains after close") { drained }
        precondition(backend.items.first?.data == Data("neutral".utf8), "Closed-surface work is cancelled before native mutation")
    }
}

/// Captures neutral wire bytes only in memory; no credential or private clipboard bytes reach logs.
@MainActor
private final class ClipboardWireFixture {
    /// Existing real runtime, with a replaceable isolated OS backend.
    let runtime: PuzzleRuntime
    /// Native responder used for the actual Cmd-V key route.
    let pane: TerminalSurfaceView
    /// Generation-bearing surface selected before tests begin.
    let surface: UInt64
    /// Only this fixture surface's transport output, never printed even on assertion failure.
    private var input = Data()
    /// UTF-8 protocol replies and neutral paste fallback for assertions.
    var text: String { String(decoding: input, as: UTF8.self) }

    /// Observe actual C callbacks; host effects still run through production deferred dispatch.
    init(_ runtime: PuzzleRuntime, pane: TerminalSurfaceView) {
        self.runtime = runtime
        self.pane = pane
        surface = runtime.surfaces[pane.paneID]!
        runtime.eventObserver = { [weak self] paneID, name, data in
            guard let self else { return }
            NativeTransportWriter.consume(self.runtime, paneID: paneID, name: name, payload: data) {
                if paneID == self.pane.paneID { self.input.append($0) }
            }
        }
    }
    /// Separate replies without changing the Session's pending clipboard work.
    func reset() { input.removeAll() }
    /// Functional timeout is a hang guard, not a clipboard timing budget.
    func wait(_ suffix: String) { PuzzleRuntimeTests.until("Clipboard protocol completion arrives") { self.text.hasSuffix(suffix) } }
    /// Feed a bounded fixture in full through the public host transport API.
    func feed(_ text: String) {
        var accepted = 0
        precondition(PuzzleRuntime.withBytes(text) { puzzle_surface_feed_output(surface, $0, &accepted) } == PUZZLE_OK)
        precondition(accepted == text.utf8.count)
    }
    /// DSR is an ordered worker barrier: modes/output cannot be overtaken by a control-channel gesture.
    func barrier(_ output: String = "") { reset(); feed(output + "\u{1b}[5n"); wait("\u{1b}[0n"); reset() }
    /// Settings enter before the later DSR proves the session has advanced.
    func setConfig(_ patch: String) {
        precondition(PuzzleRuntime.withBytes(patch) { puzzle_surface_set_config(surface, $0) } == PUZZLE_OK)
        barrier()
    }
    /// Cocoa's ordinary Cmd-V goes through Puzzle bindings, rather than calling the paste API directly.
    func pasteKey() {
        let event = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [.command], timestamp: 0,
                                    windowNumber: pane.window!.windowNumber, context: nil, characters: "v",
                                    charactersIgnoringModifiers: "v", isARepeat: false, keyCode: 9)!
        pane.window!.makeKeyAndOrderFront(nil)
        pane.window!.makeFirstResponder(pane)
        precondition(pane.performKeyEquivalent(with: event), "Cmd-V follows the native nil-target Edit menu into terminal paste:")
        let release = NSEvent.keyEvent(with: .keyUp, location: .zero, modifierFlags: [.command], timestamp: 0,
                                      windowNumber: pane.window!.windowNumber, context: nil, characters: "v",
                                      charactersIgnoringModifiers: "v", isARepeat: false, keyCode: 9)!
        pane.keyUp(with: release) // Match the real responder route, including pressed-key cleanup.
    }
    /// Request a mode-5522 listing and keep embed's credential only in this test's memory.
    func grant() -> String {
        reset(); pasteKey(); wait("\u{1b}]5522;type=read:status=DONE\u{1b}\\")
        precondition(!text.contains("hello\nworld"), "Paste-event mode must not inject fallback text")
        precondition(text.contains(Data("text/plain".utf8).base64EncodedString()))
        let start = text.range(of: ":pw=")!.upperBound
        let end = text[start...].firstIndex(of: "\u{1b}")!
        return String(text[start..<end])
    }
    /// Split each representation to exercise core chunk assembly, then await native OS completion.
    func write(_ items: [(String, Data)], status: String) {
        reset()
        var output = "\u{1b}]5522;type=write:id=host\u{1b}\\"
        for (mime, bytes) in items {
            // OSC chunks carry one continuous Base64 stream; padding may occur only at its end.
            let encoded = bytes.base64EncodedString()
            let split = encoded.index(encoded.startIndex, offsetBy: encoded.count / 8 * 4)
            for chunk in [encoded[..<split], encoded[split...]] {
                output += "\u{1b}]5522;type=wdata:mime=\(Data(mime.utf8).base64EncodedString());\(chunk)\u{1b}\\"
            }
        }
        feed(output + "\u{1b}]5522;type=wdata\u{1b}\\")
        wait("status=\(status):id=host\u{1b}\\")
    }
    /// General reads use explicit policy; grants bypass denial once for the same native generation.
    @discardableResult
    func read(_ names: [String], password: String? = nil, status: String = "DONE") -> String {
        reset()
        let credential = password.map { ":pw=\($0)" } ?? ""
        let payload = Data(names.joined(separator: " ").utf8).base64EncodedString()
        feed("\u{1b}]5522;type=read:name=UGFzdGU=\(credential);\(payload)\u{1b}\\")
        wait("\u{1b}]5522;type=read:status=\(status)\u{1b}\\")
        return text
    }
}
#endif
