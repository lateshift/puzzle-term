import AppKit

/// Decide tear-off intent from screen geometry, independently of AppKit windows or a running application.
/// Keeping the rule value-only allows maximized/overlapping-window regressions to be checked headlessly.
enum TabTearOffPolicy {
    /// A deliberate move below the title bar detaches; tiny vertical jitter still belongs to tab-strip interaction.
    static let detachmentMargin: CGFloat = 24

    /// Snapshot one visible shell window in screen coordinates; callers provide front-to-back ordering.
    struct WindowRegion {
        /// Outer bounds identify the foremost window underneath the released pointer.
        let frame: NSRect
        /// Native content geometry accounts for changing toolbar/fullscreen title-bar heights.
        let contentFrame: NSRect
    }

    /// Content is a valid tear-off destination even when its window covers the entire display.
    /// Only the frontmost containing window matters: covered title bars must not block visible content.
    static func isTearOffArea(_ point: NSPoint, windows: [WindowRegion]) -> Bool {
        guard let front = windows.first(where: { $0.frame.contains(point) }) else { return true } // Desktop/outside-window releases remain supported.
        let contentTop = min(front.frame.maxY, max(front.frame.minY, front.contentFrame.maxY))
        return point.y < contentTop - detachmentMargin // Screen coordinates increase upward; dragging down crosses this boundary.
    }

    /// A destination that already committed a move must not also trigger the source's fallback tear-off.
    /// Escape and a still-held button are cancellation, regardless of the release location.
    static func allowsFallback(operation: NSDragOperation, cancelled: Bool, released: Bool) -> Bool {
        operation.isEmpty && !cancelled && released
    }

    /// A detached window should be visibly independent when its source is maximized or fullscreen.
    /// Keep ordinary source dimensions, but use the shell's normal initial size for an expanded source.
    static func prefersNormalWindowSize(source: NSSize, visibleScreen: NSSize?, zoomed: Bool, fullscreen: Bool) -> Bool {
        if zoomed || fullscreen { return true }
        guard let visibleScreen else { return false }
        // Edge resizing can fill a display without setting isZoomed; tolerate small native frame rounding differences.
        return source.width >= visibleScreen.width - 2 && source.height >= visibleScreen.height - 2
    }
}
