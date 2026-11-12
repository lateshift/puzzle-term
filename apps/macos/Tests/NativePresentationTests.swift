// Exercise the real kind-5 retained-container contract, without judging resize appearance (D-60/R15).
import AppKit
import QuartzCore
#if PUZZLE_ENABLED

/// Real C surfaces install an autoresizing IOSurface child; the app controls only its parent container.
@MainActor
enum NativePresentationTests {
    /// Keep the child after returning so the caller can verify synchronous detach when the pane closes.
    static func run(_ runtime: PuzzleRuntime, pane: TerminalSurfaceView) -> CALayer {
        let container = pane.metalLayer
        let handle = runtime.surfaces[pane.paneID]!
        // Placeholder layers have no image contents or bounds-triggered display. Locate the live presentation child by behavior.
        let children = (container.sublayers ?? []).filter { $0.contents != nil && $0.needsDisplayOnBoundsChange }
        precondition(children.count == 1, "Exactly one Puzzle image child is installed under the borrowed container")
        let child = children[0]
        precondition(child.autoresizingMask.contains([.layerWidthSizable, .layerHeightSizable]))
        let original = pane.frame.size
        let drawable = container.drawableSize
        for size in [NSSize(width: 333, height: 222), NSSize(width: 617, height: 411), original] {
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            pane.setFrameSize(size)
            pane.layoutSubtreeIfNeeded()
            container.layoutIfNeeded()
            // Force the same native display callback CA invokes before commit. A timeout is allowed by D-60.
            child.displayIfNeeded()
            precondition(child.frame == container.bounds, "Puzzle's child follows native container bounds before CA commits")
            precondition(child.contentsScale == pane.window!.backingScaleFactor, "Backing scale reaches Puzzle before native display")
            precondition(child.superlayer === container && pane.layer === container && runtime.surfaces[pane.paneID] == handle)
            precondition(container.drawableSize == drawable, "IOSurface presentation never configures CAMetalLayer drawables")
            CATransaction.commit()
        }
        print("PASS: kind-5 IOSurface child, bounds/scale before CA commit, retained container/surface identity and untouched drawableSize; appearance pending user review")
        return child
    }
}
#endif
