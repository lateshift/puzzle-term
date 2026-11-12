// M18.4: native delivery generations belong to surfaces; OSC policy and reports stay in Puzzle.
import AppKit
#if PUZZLE_ENABLED

/// Closed feedback vocabulary from puzzle.h, unrelated to application-supplied OSC identifiers.
enum PuzzleNotificationOutcome: UInt32, Sendable {
    /// The user chose the native notification's default action.
    case activated = 0
    /// The user dismissed a native delivery.
    case closed = 1
    /// Delivery works but the host cannot observe closure reliably.
    case untracked = 2
    /// OS permission or native posting failed; never fabricate a dismissal.
    case failed = 3
}

/// Owned, already policy-approved callback data; the host does not parse terminal output.
struct PuzzleNotification: Sendable {
    /// Opaque Puzzle generation, copied before the C callback returns.
    let token: String
    /// Sanitized heading supplied by Puzzle's shared notification policy.
    let title: String
    /// Sanitized body, independent of the heading and safe to hand to native UI.
    let body: String
    /// Native interruption hint: low, normal or urgent; OS preferences still take precedence.
    let urgency: Int

    /// Only native presentation fields are interpreted; occasion/actions/report options remain Rust-owned.
    init?(_ value: Any?) {
        guard let map = value as? [String: Any], let token = map["token"] as? String,
              !token.isEmpty, token.utf8.count <= 64, let title = map["title"] as? String,
              let body = map["body"] as? String, let urgency = map["urgency"] as? Int,
              (0...2).contains(urgency) else { return nil }
        self.token = token
        self.title = title
        self.body = body
        self.urgency = urgency
    }
}

/// Native OS seam permits deterministic delivery failures and delayed callbacks in app tests.
@MainActor
protocol PuzzleNotificationPosting: AnyObject {
    /// Called on the main queue after copying the OS notification identifier and user action.
    var response: ((String, PuzzleNotificationOutcome) -> Void)? { get set }
    /// Native authorization and posting finish asynchronously; cancellation is checked before posting.
    func post(_ notification: PuzzleNotification, identifier: String, isCurrent: @escaping @MainActor @Sendable () -> Bool,
              completion: @escaping @MainActor @Sendable (Bool) -> Void)
    /// Remove both pending and delivered notifications, including a post racing with teardown.
    func remove(_ identifiers: [String])
}

/// Routes delayed OS responses to exactly the live surface that created a generation.
final class PuzzleNotifications {
    /// Retained until native dismissal/removal; never contains a borrowed C pointer.
    private struct Delivery {
        /// Puzzle generation echoed through the C feedback method.
        let token: String
        /// Original surface handle includes its generation; moves do not change this identity.
        let surface: UInt64
        /// Liveness check rejects closures and callbacks after native pane teardown.
        let isLive: () -> Bool
        /// The original runtime's feedback route, never whichever pane currently has focus.
        let feedback: (String, PuzzleNotificationOutcome) -> Void
    }
    /// Per-runtime prefix prevents another app run's persisted notifications from aliasing this table.
    private let prefix = "puzzle-" + UUID().uuidString + "-"
    /// Native implementations own authorization/posting details, never OSC grammar.
    private let center: any PuzzleNotificationPosting
    /// Native identifiers name immutable delivery generations across every pane/window.
    private var deliveries: [String: Delivery] = [:]

    /// Notification Center is not contacted until the first admitted notification is posted.
    init(_ center: any PuzzleNotificationPosting = PuzzleNotificationCenter()) {
        self.center = center
        center.response = { [weak self] identifier, outcome in self?.receive(identifier, outcome: outcome) }
    }

    /// Retain the creating surface before starting asynchronous OS permission/posting work.
    func show(_ notification: PuzzleNotification, surface: UInt64, isLive: @escaping () -> Bool,
              feedback: @escaping (String, PuzzleNotificationOutcome) -> Void) {
        guard isLive() else { return }
        let identifier = nativeIdentifier(notification.token, surface: surface)
        deliveries[identifier] = Delivery(token: notification.token, surface: surface, isLive: isLive, feedback: feedback)
        center.post(notification, identifier: identifier, isCurrent: { [weak self] in
            self?.deliveries[identifier]?.isLive() == true
        }) { [weak self] succeeded in
            guard let self else { return }
            // An add can finish after close/reset. Remove again so its late completion cannot resurrect it.
            guard let delivery = self.deliveries[identifier], delivery.isLive() else {
                self.deliveries.removeValue(forKey: identifier)
                self.center.remove([identifier])
                return
            }
            if !succeeded { self.receive(identifier, outcome: .failed) }
        }
    }

    /// Explicit Rust close/replacement/reset removes OS work without inventing feedback or replies.
    func close(token: String, surface: UInt64) {
        let identifier = nativeIdentifier(token, surface: surface)
        deliveries.removeValue(forKey: identifier)
        center.remove([identifier])
    }

    /// Pane teardown removes every delivery even when native posting has not completed yet.
    func remove(surface: UInt64) {
        let identifiers = deliveries.filter { $0.value.surface == surface }.map(\.key)
        for identifier in identifiers { deliveries.removeValue(forKey: identifier) }
        center.remove(identifiers)
    }

    /// App shutdown must also remove notifications for surfaces retained after an earlier close error.
    func removeAll() {
        let identifiers = Array(deliveries.keys)
        deliveries.removeAll()
        center.remove(identifiers)
    }

    /// Duplicate/stale OS responses cannot report against a replacement, a new pane or a closed surface.
    private func receive(_ identifier: String, outcome: PuzzleNotificationOutcome) {
        guard let delivery = deliveries[identifier], delivery.isLive() else { return }
        if outcome != .untracked {
            deliveries.removeValue(forKey: identifier)
            center.remove([identifier])
        }
        delivery.feedback(delivery.token, outcome) // Rust decides reports, focus and protocol lifetime.
    }

    /// Surface plus Puzzle token is distinct even if two sessions reuse the same application ID.
    private func nativeIdentifier(_ token: String, surface: UInt64) -> String { prefix + "\(surface)-\(token)" }
}
#endif
