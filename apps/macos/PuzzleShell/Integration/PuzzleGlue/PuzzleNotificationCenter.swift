// M18.4: UserNotifications is an asynchronous OS adapter, not a second notification protocol owner.
import AppKit
@preconcurrency import UserNotifications
#if PUZZLE_ENABLED

/// Native posting is lazy so ordinary app use and unbundled native tests create no permission prompt.
final class PuzzleNotificationCenter: NSObject, PuzzleNotificationPosting, UNUserNotificationCenterDelegate {
    /// All delegate responses leave this adapter as copied identity plus a closed outcome tag.
    var response: ((String, PuzzleNotificationOutcome) -> Void)?
    /// Retain the center after first use; it holds its delegate weakly.
    private var center: UNUserNotificationCenter?
    /// A category with custom dismiss action requests real user-dismissal delegate callbacks.
    private static let category = "PuzzleTerminalNotification"

    /// Request permission only for an actual admitted post; a denied prompt is delivery failure.
    func post(_ notification: PuzzleNotification, identifier: String, isCurrent: @escaping @MainActor @Sendable () -> Bool,
              completion: @escaping @MainActor @Sendable (Bool) -> Void) {
        guard let center = notificationCenter() else { completion(false); return }
        center.requestAuthorization(options: [.alert, .sound]) { granted, error in
            // OS completion handlers may run off-main. Copy only the result across to the UI owner.
            let allowed = granted && error == nil
            DispatchQueue.main.async {
                guard allowed, isCurrent() else { completion(false); return }
                let content = UNMutableNotificationContent()
                content.title = notification.title
                content.body = notification.body
                content.categoryIdentifier = Self.category
                // Core already evaluated occasion against surface focus/visibility. OS preferences
                // govern native interruption; time-sensitive presentation never bypasses those settings.
                switch notification.urgency {
                case 0: content.interruptionLevel = .passive
                case 2: content.interruptionLevel = .timeSensitive
                default: content.interruptionLevel = .active
                }
                let request = UNNotificationRequest(identifier: identifier, content: content, trigger: nil)
                center.add(request) { error in
                    let succeeded = error == nil
                    DispatchQueue.main.async {
                        // The runtime may have been destroyed while add was in flight. Retain only
                        // this OS center until completion so cancellation still removes a late post.
                        if !isCurrent() {
                            center.removePendingNotificationRequests(withIdentifiers: [identifier])
                            center.removeDeliveredNotifications(withIdentifiers: [identifier])
                        }
                        completion(succeeded)
                    }
                }
            }
        }
    }

    /// Remove both namespaces; a pending request must not appear after its surface has closed.
    func remove(_ identifiers: [String]) {
        guard let center, !identifiers.isEmpty else { return }
        center.removePendingNotificationRequests(withIdentifiers: identifiers)
        center.removeDeliveredNotifications(withIdentifiers: identifiers)
    }

    /// current() requires a bundle identity; unbundled test executables must fail without an exception.
    private func notificationCenter() -> UNUserNotificationCenter? {
        guard Bundle.main.bundleIdentifier != nil else { return nil }
        if let center { return center }
        let center = UNUserNotificationCenter.current()
        center.delegate = self
        // No custom buttons are advertised by Puzzle; only default activation and native dismissal.
        center.setNotificationCategories([UNNotificationCategory(identifier: Self.category, actions: [],
                                                                  intentIdentifiers: [], options: [.customDismissAction])])
        self.center = center
        return center
    }

    /// Always-occasion notifications may be shown while the app is active; Puzzle owns occasion policy.
    nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification,
                                           withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void) {
        completionHandler([.banner, .list])
    }

    /// Copy delegate identifiers before returning and send feedback on Puzzle's main-thread owner.
    nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse,
                                           withCompletionHandler completionHandler: @escaping () -> Void) {
        let identifier = response.notification.request.identifier
        let action = response.actionIdentifier
        // Unknown custom actions cannot be mistaken for a body activation or invent reports.
        let outcome: PuzzleNotificationOutcome?
        switch action {
        case UNNotificationDefaultActionIdentifier: outcome = .activated
        case UNNotificationDismissActionIdentifier: outcome = .closed
        default: outcome = nil
        }
        if let outcome {
            DispatchQueue.main.async { [weak self] in self?.response?(identifier, outcome) }
        }
        completionHandler() // No OS delegate callback waits for Session, rendering or native window focus.
    }
}
#endif
