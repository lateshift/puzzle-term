// M18.4: real C notification lifecycle with a controlled asynchronous OS boundary; no permission prompts.
import AppKit
#if PUZZLE_ENABLED

/// Retains delayed posting/response callbacks so teardown and replacement races are reproducible.
@MainActor
private final class NotificationTestCenter: PuzzleNotificationPosting {
    /// Native delegate route installed by the production delivery manager.
    var response: ((String, PuzzleNotificationOutcome) -> Void)?
    /// One OS attempt, with its immutable presentation fields and delayed completion.
    struct Post {
        /// Already sanitized native content from the actual C show callback.
        let notification: PuzzleNotification
        /// Native identifier namespaces Puzzle's token by its originating surface.
        let identifier: String
        /// Production cancellation gate, checked before the OS would post after authorization.
        let isCurrent: @MainActor @Sendable () -> Bool
        /// Actual OS result delivered after the requested operation finishes.
        let completion: @MainActor @Sendable (Bool) -> Void
    }
    /// Append-only test observations distinguish fresh generations from replacements.
    var posts: [Post] = []
    /// Every requested native removal, including repeats needed for late OS completion.
    var removed: [String] = []
    /// Hold completion to model permission prompts and asynchronous native delivery.
    func post(_ notification: PuzzleNotification, identifier: String, isCurrent: @escaping @MainActor @Sendable () -> Bool,
              completion: @escaping @MainActor @Sendable (Bool) -> Void) {
        posts.append(Post(notification: notification, identifier: identifier, isCurrent: isCurrent, completion: completion))
    }
    /// Record removal without manufacturing a user dismissal callback.
    func remove(_ identifiers: [String]) { removed.append(contentsOf: identifiers) }
    /// Match the real OS adapter's main-queue completion delivery.
    func finish(_ index: Int, succeeds: Bool = true) {
        let post = posts[index]
        DispatchQueue.main.async { post.completion(succeeds) }
    }
    /// Match native delegate response delivery; duplicates deliberately exercise stale-generation guards.
    func respond(_ index: Int, _ outcome: PuzzleNotificationOutcome) {
        let identifier = posts[index].identifier
        DispatchQueue.main.async { self.response?(identifier, outcome) }
    }
}

/// C callbacks, native manager and feedback all participate; these tests certify no OS visual behavior.
@MainActor
enum NativeNotificationTests {
    /// Isolate Notification Center from automated tests while retaining the real Session protocol owner.
    static func run(_ runtime: PuzzleRuntime) {
        let oldManager = runtime.notifications
        let oldObserver = runtime.eventObserver
        let center = NotificationTestCenter()
        runtime.notifications = PuzzleNotifications(center)
        defer { runtime.notifications.removeAll(); runtime.notifications = oldManager; runtime.eventObserver = oldObserver }
        checkActivation(runtime, center: center)
        checkExplicitClose(runtime, center: center)
        checkDismissal(runtime, center: center)
        checkReplacementReset(runtime, center: center)
        checkFailureAndUntracked(runtime, center: center)
        checkPendingTeardown(runtime, center: center)
        checkLegacyOrigins(runtime, center: center)
        print("PASS: C notification posting/feedback, activation focus after tab transfer, surface isolation, dismissal, replacement/RIS/explicit removal, untracked/failure and pending-post teardown")
    }

    /// Activation reports reach only the creating surface and focus its current window/pane after transfer.
    private static func checkActivation(_ runtime: PuzzleRuntime, center: NotificationTestCenter) {
        let fixture = NotificationWireFixture(runtime, center: center)
        defer { fixture.close() }
        let index = fixture.post("\u{1b}]99;i=review:u=2:a=report:c=1:d=0;Build finished\u{1b}\\" +
                                 "\u{1b}]99;i=review:p=body;Click to activate\u{1b}\\")
        let n = center.posts[index].notification
        precondition(n.title == "Build finished" && n.body == "Click to activate" && n.urgency == 2)
        center.finish(index)
        let sibling = fixture.tab.split(.right)
        precondition(sibling != 0 && fixture.tab.surface.paneID == sibling)
        let moved = puzzle_shell_move_tab_to_new_window(fixture.tab.id)
        precondition(moved != 0)
        center.respond(index, .activated)
        fixture.wait("\u{1b}]99;i=review;\u{1b}\\\u{1b}]99;i=review:p=close;\u{1b}\\")
        PuzzleRuntimeTests.until("Notification focuses creating pane in its current window") {
            fixture.tab.surface.paneID == fixture.paneID && fixture.pane.window?.firstResponder === fixture.pane
        }
        precondition(fixture.otherInput.isEmpty, "Activation report cannot reach a sibling surface")
        center.respond(index, .closed)
        fixture.barrier()
        precondition(fixture.text == "\u{1b}[0n", "Duplicate native dismissal cannot report twice")
    }

    /// Explicit protocol close removes the native delivery; its acknowledgement remains Rust-owned.
    private static func checkExplicitClose(_ runtime: PuzzleRuntime, center: NotificationTestCenter) {
        let fixture = NotificationWireFixture(runtime, center: center)
        defer { fixture.close() }
        let index = fixture.post()
        center.finish(index)
        fixture.feed("\u{1b}]99;i=review:p=close;\u{1b}\\")
        fixture.wait("\u{1b}]99;i=review:p=close;\u{1b}\\")
        PuzzleRuntimeTests.until("Explicit close removes native delivery") { center.removed.contains(center.posts[index].identifier) }
        center.respond(index, .activated)
        fixture.barrier()
        precondition(fixture.text == "\u{1b}[0n", "Removed notification activation is inert")
    }

    /// Native user dismissal produces close only, never an activation or a focus event.
    private static func checkDismissal(_ runtime: PuzzleRuntime, center: NotificationTestCenter) {
        let fixture = NotificationWireFixture(runtime, center: center)
        defer { fixture.close() }
        let index = fixture.post()
        center.finish(index)
        center.respond(index, .closed)
        fixture.wait("\u{1b}]99;i=review:p=close;\u{1b}\\")
        precondition(fixture.text == "\u{1b}]99;i=review:p=close;\u{1b}\\")
    }

    /// Replacement invalidates its old token; RIS also removes native work without synthetic reports.
    private static func checkReplacementReset(_ runtime: PuzzleRuntime, center: NotificationTestCenter) {
        let fixture = NotificationWireFixture(runtime, center: center)
        defer { fixture.close() }
        let first = fixture.post()
        // The shared protocol allowance is one delivery/second. Only this test waits for it.
        let admitted = Date().addingTimeInterval(1.05)
        PuzzleRuntimeTests.until("Notification admission interval elapses") { Date() >= admitted }
        let next = fixture.post("\u{1b}]99;i=review:a=report:c=1;Replacement\u{1b}\\")
        precondition(center.posts[first].identifier != center.posts[next].identifier)
        precondition(center.removed.contains(center.posts[first].identifier), "Replacement removes previous native generation")
        center.respond(first, .closed)
        fixture.barrier()
        precondition(fixture.text == "\u{1b}[0n", "Old generation cannot close or acknowledge its replacement")
        fixture.barrier("\u{1b}c")
        PuzzleRuntimeTests.until("RIS removes remaining native delivery") { center.removed.contains(center.posts[next].identifier) }
        center.respond(next, .activated)
        fixture.barrier()
        precondition(fixture.text == "\u{1b}[0n", "Reset generation cannot reactivate")
    }

    /// Posting failures are not closures; unsupported tracking reports once and preserves activation.
    private static func checkFailureAndUntracked(_ runtime: PuzzleRuntime, center: NotificationTestCenter) {
        let failure = NotificationWireFixture(runtime, center: center)
        let failedIndex = failure.post()
        center.finish(failedIndex, succeeds: false)
        PuzzleRuntimeTests.until("Failed OS posting removes delivery") { center.removed.contains(center.posts[failedIndex].identifier) }
        failure.barrier()
        precondition(failure.text == "\u{1b}[0n", "Failure invents no activation or close report")
        failure.close()
        let fixture = NotificationWireFixture(runtime, center: center)
        defer { fixture.close() }
        let index = fixture.post()
        center.finish(index)
        center.respond(index, .untracked)
        fixture.wait("\u{1b}]99;i=review:p=close;untracked\u{1b}\\")
        fixture.reset()
        center.respond(index, .activated)
        fixture.wait("\u{1b}]99;i=review;\u{1b}\\")
        precondition(fixture.text == "\u{1b}]99;i=review;\u{1b}\\", "Untracked generation keeps activation and suppresses duplicate close")
    }

    /// Legacy OSC 9/777 reach the same typed C callback and native manager as OSC 99.
    private static func checkLegacyOrigins(_ runtime: PuzzleRuntime, center: NotificationTestCenter) {
        for (output, title, body) in [("\u{1b}]9;Legacy heading\u{7}", "Legacy heading", ""),
                                      ("\u{1b}]777;notify;Legacy title;Legacy body\u{7}", "Legacy title", "Legacy body")] {
            let fixture = NotificationWireFixture(runtime, center: center)
            let index = fixture.post(output)
            precondition(center.posts[index].notification.title == title && center.posts[index].notification.body == body)
            fixture.close()
        }
    }

    /// Surface/app teardown remove pending work and late successful adds cannot resurrect notifications.
    private static func checkPendingTeardown(_ runtime: PuzzleRuntime, center: NotificationTestCenter) {
        let fixture = NotificationWireFixture(runtime, center: center)
        let index = fixture.post()
        let identifier = center.posts[index].identifier
        fixture.close()
        precondition(center.removed.contains(identifier) && !center.posts[index].isCurrent())
        let removals = center.removed.filter { $0 == identifier }.count
        center.finish(index)
        center.respond(index, .activated)
        PuzzleRuntimeTests.until("Late native add is removed after surface close") {
            center.removed.filter { $0 == identifier }.count > removals
        }
        let other = NotificationWireFixture(runtime, center: center)
        defer { other.close() }
        let pending = other.post()
        runtime.notifications.removeAll()
        precondition(!center.posts[pending].isCurrent() && center.removed.contains(center.posts[pending].identifier),
                     "Application cleanup cancels every pending notification generation")
    }
}

/// A fresh Session per case avoids cross-case rate-policy interference and keeps native pane isolation real.
@MainActor
private final class NotificationWireFixture {
    /// One active runtime serves the original test window and these temporary fixtures.
    let runtime: PuzzleRuntime
    /// Controlled OS boundary retains every requested native post and delayed completion.
    let center: NotificationTestCenter
    /// Stable tab survives the activation test's transfer to another native window.
    let tab: TerminalTab
    /// Creating responder stays alive until fixture teardown synchronously frees its surface.
    let pane: TerminalSurfaceView
    /// Original pane identity remains distinct from the active sibling.
    let paneID: UInt64
    /// C handle whose transport carries activation and close replies.
    let surface: UInt64
    /// Reply bytes from the creating surface only.
    private var input = Data()
    /// Any other surface input proves an isolation bug; never print payloads.
    private(set) var otherInput = Data()
    /// Neutral UTF-8 reports for exact wire assertions.
    var text: String { String(decoding: input, as: UTF8.self) }

    /// Create native layers and observe actual puzzle.h callbacks without mocking Session or Rust effects.
    init(_ runtime: PuzzleRuntime, center: NotificationTestCenter) {
        self.runtime = runtime
        self.center = center
        tab = ShellCoordinator.shared.createWindow().selectedTab!
        pane = tab.surface
        paneID = pane.paneID
        surface = runtime.surfaces[paneID]!
        runtime.eventObserver = { [weak self] paneID, name, bytes in
            guard let self else { return }
            NativeTransportWriter.consume(self.runtime, paneID: paneID, name: name, payload: bytes) {
                if paneID == self.paneID { self.input.append($0) } else { self.otherInput.append($0) }
            }
        }
    }
    /// Close whichever native window currently owns the transferred tab.
    func close() { ShellCoordinator.shared.owner(of: tab.id)?.window?.close() }
    /// Clear transport observations before an independent feedback operation.
    func reset() { input.removeAll(); otherInput.removeAll() }
    /// Small bounded fixtures are accepted in full, then parsed by the real Session worker.
    func feed(_ text: String) {
        var accepted = 0
        precondition(PuzzleRuntime.withBytes(text) { puzzle_surface_feed_output(surface, $0, &accepted) } == PUZZLE_OK)
        precondition(accepted == text.utf8.count)
    }
    /// A timeout catches missing OS/core completion, without asserting a native performance budget.
    func wait(_ suffix: String) { PuzzleRuntimeTests.until("Notification wire feedback arrives") { self.text.hasSuffix(suffix) } }
    /// The DSR barrier flushes previously queued feedback without relying on arbitrary sleeps.
    func barrier(_ prefix: String = "") { reset(); feed(prefix + "\u{1b}[5n"); wait("\u{1b}[0n") }
    /// Wait for the app's actual deferred host dispatch to ask the OS boundary to post.
    func post(_ text: String = "\u{1b}]99;i=review:a=report:c=1;Notification\u{1b}\\") -> Int {
        reset()
        let count = center.posts.count
        feed(text)
        PuzzleRuntimeTests.until("Native notification post follows actual C callback") { self.center.posts.count == count + 1 }
        return count
    }
}
#endif
