// M18.4: native delivery feedback returns to its creating Puzzle surface; focus follows Rust policy.
import AppKit
#if PUZZLE_ENABLED

/// No OSC metadata, report encoding or notification policy is implemented in Swift.
extension PuzzleRuntime {
    /// Native effects are already deferred past tick and guarded by the originating surface generation.
    func showNotification(_ value: Any?, paneID: UInt64, surface: UInt64) {
        guard let notification = PuzzleNotification(value) else { return }
        notifications.show(notification, surface: surface, isLive: { [weak self] in
            self?.surfaces[paneID] == surface
        }) { [weak self] token, outcome in
            guard let self, self.surfaces[paneID] == surface else { return }
            let result = Self.withBytes(token) { puzzle_surface_notification_feedback(surface, $0, outcome.rawValue) }
            self.report(result, operation: "Complete native notification")
        }
    }

    /// Rust resolves reusable application IDs to immutable delivery tokens before requesting removal.
    func closeNotification(_ value: Any?, surface: UInt64) {
        guard let map = value as? [String: Any], let token = map["token"] as? String else { return }
        notifications.close(token: token, surface: surface)
    }

    /// Resolve the pane's current window after any tab transfer; only a core-approved user activation focuses.
    func focusNotificationPane(_ paneID: UInt64) {
        guard let tab = ShellCoordinator.shared.tab(containing: paneID),
              let owner = ShellCoordinator.shared.owner(of: tab.id) else { return }
        owner.select(tab.id)
        _ = tab.selectPane(paneID)
        owner.window?.deminiaturize(nil)
        owner.window?.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }
}
#endif
