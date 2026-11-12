import AppKit

/// Validate tear-off geometry and native completion semantics without creating an application, window or view.
@MainActor
enum TabDragPolicyTests {
    /// Report these focused regression assertions as part of the existing headless test count.
    private static var checks = 0

    /// Fail with the user-visible behavior that regressed, rather than an implementation-specific diagnostic.
    private static func check(_ condition: @autoclosure () -> Bool, _ message: String) {
        precondition(condition(), message)
        checks += 1
    }

    /// A source filling the display must permit release over content while preserving strip interaction.
    private static func testExpandedWindow() {
        let maximized = TabTearOffPolicy.WindowRegion(frame: NSRect(x: 0, y: 0, width: 1600, height: 1000),
                                                      contentFrame: NSRect(x: 0, y: 0, width: 1600, height: 950))
        check(TabTearOffPolicy.isTearOffArea(NSPoint(x: 800, y: 500), windows: [maximized]), "Maximized window content accepts tear-off without desktop access")
        check(!TabTearOffPolicy.isTearOffArea(NSPoint(x: 800, y: 975), windows: [maximized]), "Tab-strip drops remain insertion/reorder gestures")
        check(!TabTearOffPolicy.isTearOffArea(NSPoint(x: 800, y: 940), windows: [maximized]), "Small movement below chrome cannot accidentally detach")
        let boundary = maximized.contentFrame.maxY - TabTearOffPolicy.detachmentMargin
        check(!TabTearOffPolicy.isTearOffArea(NSPoint(x: 800, y: boundary), windows: [maximized]), "Exact margin remains protected")
        check(TabTearOffPolicy.isTearOffArea(NSPoint(x: 800, y: boundary - 1), windows: [maximized]), "Crossing the deliberate margin offers tear-off")
        check(TabTearOffPolicy.isTearOffArea(NSPoint(x: -20, y: 975), windows: [maximized]), "Dragging sideways beyond the window also supports tear-off")
        check(TabTearOffPolicy.isTearOffArea(NSPoint(x: 800, y: -20), windows: [maximized]), "Desktop releases stay supported")
        check(TabTearOffPolicy.isTearOffArea(.zero, windows: []), "An empty window snapshot permits desktop tear-off")
        let fullscreen = TabTearOffPolicy.WindowRegion(frame: maximized.frame, contentFrame: maximized.frame)
        check(TabTearOffPolicy.isTearOffArea(NSPoint(x: 800, y: 500), windows: [fullscreen]), "Hidden fullscreen chrome does not block content")
        check(!TabTearOffPolicy.isTearOffArea(NSPoint(x: 800, y: 990), windows: [fullscreen]), "A hidden title bar still retains a small upper detachment margin")
    }

    /// Covered background title bars must not block the front window's visible content.
    private static func testOverlappingWindows() {
        let front = TabTearOffPolicy.WindowRegion(frame: NSRect(x: -1600, y: -200, width: 1600, height: 1000),
                                                  contentFrame: NSRect(x: -1600, y: -200, width: 1600, height: 950))
        let back = TabTearOffPolicy.WindowRegion(frame: NSRect(x: -1500, y: -100, width: 1000, height: 640),
                                                 contentFrame: NSRect(x: -1500, y: -100, width: 1000, height: 590))
        let coveredTitlebar = NSPoint(x: -1200, y: 520)
        check(TabTearOffPolicy.isTearOffArea(coveredTitlebar, windows: [front, back]), "Covered background chrome cannot veto visible foreground content")
        check(!TabTearOffPolicy.isTearOffArea(coveredTitlebar, windows: [back, front]), "Frontmost destination title bar retains insertion priority")
        check(TabTearOffPolicy.isTearOffArea(NSPoint(x: -1200, y: 200), windows: [back, front]), "Content in another shell window offers a new window, not an implicit tab merge")
    }

    /// Escape, held-button cancellation and accepted native drops must never create a second window.
    private static func testCompletion() {
        check(TabTearOffPolicy.allowsFallback(operation: [], cancelled: false, released: true), "Unhandled intentional release can detach")
        check(!TabTearOffPolicy.allowsFallback(operation: [], cancelled: true, released: true), "Escape cancels tear-off over content and desktop")
        check(!TabTearOffPolicy.allowsFallback(operation: [], cancelled: false, released: false), "A still-held button cannot trigger tear-off")
        check(!TabTearOffPolicy.allowsFallback(operation: .move, cancelled: false, released: true), "Successful strip/content destination does not detach twice")
        check(!TabTearOffPolicy.allowsFallback(operation: .copy, cancelled: false, released: true), "Other completed operations do not invoke fallback")
    }

    /// A maximized source stays intact while its newly detached window uses normal dimensions.
    private static func testWindowSize() {
        let display = NSSize(width: 1600, height: 1000)
        check(TabTearOffPolicy.prefersNormalWindowSize(source: display, visibleScreen: display, zoomed: false, fullscreen: false), "Manual display-filling source uses normal detached size")
        check(TabTearOffPolicy.prefersNormalWindowSize(source: NSSize(width: 1599, height: 999), visibleScreen: display, zoomed: false, fullscreen: false), "Expanded-size detection tolerates frame rounding")
        check(TabTearOffPolicy.prefersNormalWindowSize(source: NSSize(width: 1200, height: 900), visibleScreen: display, zoomed: true, fullscreen: false), "Native zoom uses normal detached size")
        check(TabTearOffPolicy.prefersNormalWindowSize(source: display, visibleScreen: nil, zoomed: false, fullscreen: true), "Fullscreen detection works without a screen snapshot")
        check(!TabTearOffPolicy.prefersNormalWindowSize(source: NSSize(width: 1000, height: 640), visibleScreen: display, zoomed: false, fullscreen: false), "Ordinary source dimensions are retained")
        check(!TabTearOffPolicy.prefersNormalWindowSize(source: NSSize(width: 1600, height: 640), visibleScreen: display, zoomed: false, fullscreen: false), "A wide ordinary window is not mistaken for a maximized window")
        check(!TabTearOffPolicy.prefersNormalWindowSize(source: NSSize(width: 1000, height: 640), visibleScreen: nil, zoomed: false, fullscreen: false), "Missing screen does not discard ordinary source size")
    }

    /// Return a count to the combined headless runner; no native drag session or computer automation is started.
    static func run() -> Int {
        checks = 0
        testExpandedWindow()
        testOverlappingWindows()
        testCompletion()
        testWindowSize()
        return checks
    }
}
