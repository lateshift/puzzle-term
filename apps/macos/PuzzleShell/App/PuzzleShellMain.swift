import AppKit

/// AppKit owns the event loop; no SwiftUI scene or winit loop is involved.
@main
struct PuzzleShellMain {
    /// Keep the delegate alive for the entire event loop because NSApplication holds it weakly.
    @MainActor static func main() {
        let application = NSApplication.shared
        let delegate = ShellAppDelegate()
        application.setActivationPolicy(.regular) // Show the Dock icon and standard menu bar.
        application.delegate = delegate
        withExtendedLifetime(delegate) { application.run() } // Run until the user quits.
    }
}
