import AppKit

/// Vector equivalents of the Photoshop mockup's charcoal fill and fading hairline stroke.
/// Drawing is shared by live tabs and offscreen rendering; no PSD or bitmap is shipped in the app.
@MainActor
enum TabChrome {
    /// The mockup's 368-pixel tab at 2× corresponds to the existing fixed 184-point width.
    static let cornerRadius: CGFloat = 7

    /// Explicit sRGB avoids calibrated-white conversion subtly changing the measured PSD colors.
    static func rgb(_ red: CGFloat, _ green: CGFloat, _ blue: CGFloat, alpha: CGFloat = 1) -> NSColor {
        NSColor(srgbRed: red / 255, green: green / 255, blue: blue / 255, alpha: alpha)
    }

    /// Follow the native window appearance; the light counterpart retains the same contrast hierarchy.
    static func isDark(_ appearance: NSAppearance) -> Bool {
        appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
    }

    /// The PSD samples #262A36 behind the tabs; its light equivalent stays quietly cool-toned.
    static func titlebarColor(dark: Bool) -> NSColor {
        dark ? rgb(38, 42, 54) : rgb(232, 234, 240)
    }

    /// Construct a top-rounded, square-bottom silhouette, closing only when filling its interior.
    /// An open stroke deliberately omits the bottom edge so the tab meets the content unobstructed.
    static func tabPath(in rect: CGRect, radius: CGFloat = cornerRadius, closed: Bool) -> CGPath {
        let radius = max(0, min(radius, min(rect.width / 2, rect.height))) // Keep narrow/transitional frames valid.
        let path = CGMutablePath()
        let handle = radius * 0.5522847498 // Cubic Bézier approximation of a quarter circle.
        path.move(to: CGPoint(x: rect.minX, y: rect.minY))
        path.addLine(to: CGPoint(x: rect.minX, y: rect.maxY - radius))
        path.addCurve(to: CGPoint(x: rect.minX + radius, y: rect.maxY),
                      control1: CGPoint(x: rect.minX, y: rect.maxY - radius + handle),
                      control2: CGPoint(x: rect.minX + radius - handle, y: rect.maxY))
        path.addLine(to: CGPoint(x: rect.maxX - radius, y: rect.maxY))
        path.addCurve(to: CGPoint(x: rect.maxX, y: rect.maxY - radius),
                      control1: CGPoint(x: rect.maxX - radius + handle, y: rect.maxY),
                      control2: CGPoint(x: rect.maxX, y: rect.maxY - radius + handle))
        path.addLine(to: CGPoint(x: rect.maxX, y: rect.minY))
        if closed { path.closeSubpath() } // Filling needs a bottom; the border must remain open there.
        return path
    }

    /// Clip a vertical gradient to a vector shape; explicit top/bottom points avoid flipped-view ambiguity.
    private static func gradient(_ colors: [NSColor], locations: [CGFloat], in rect: CGRect,
                                 clipping path: CGPath, context: CGContext) {
        let space = CGColorSpace(name: CGColorSpace.sRGB)!
        guard let gradient = CGGradient(colorsSpace: space, colors: colors.map(\.cgColor) as CFArray,
                                        locations: locations) else { return }
        context.saveGState() // The shape clip must not leak into subsequent text, accent or close-button drawing.
        context.addPath(path)
        context.clip()
        context.drawLinearGradient(gradient, start: CGPoint(x: rect.midX, y: rect.maxY),
                                   end: CGPoint(x: rect.midX, y: rect.minY),
                                   options: [.drawsBeforeStartLocation, .drawsAfterEndLocation])
        context.restoreGState()
    }

    /// Paint the PSD's flat upper charcoal and subtle lower-quarter gradient, then its fading stroke.
    /// A scale-dependent one-pixel stroke stays crisp at both standard and Retina resolutions.
    static func drawTab(in bounds: CGRect, selected: Bool, hovered: Bool, keyWindow: Bool,
                        dark: Bool, scale: CGFloat, increasedContrast: Bool, context: CGContext) {
        guard bounds.width > 0, bounds.height > 0, selected || hovered else { return }
        let pixel = 1 / max(1, scale)
        // Inset only the top/sides by half a physical pixel; keep the bottom flush with content.
        let rect = CGRect(x: bounds.minX + pixel / 2, y: bounds.minY,
                          width: bounds.width - pixel, height: bounds.height - pixel / 2)
        guard rect.width > 0, rect.height > 0 else { return }
        let fill: [NSColor]
        let stroke: [NSColor]
        if selected {
            // Measured PSD stops: #1E1E1E → #191919; top stroke #6D6D6D → slate #272A35.
            fill = dark ? [rgb(30, 30, 30), rgb(30, 30, 30), rgb(25, 25, 25)]
                        : [rgb(250, 250, 252), rgb(250, 250, 252), rgb(226, 228, 234)]
            stroke = dark ? [rgb(109, 109, 109), rgb(109, 109, 109), rgb(39, 42, 53)]
                          : [rgb(156, 160, 170), rgb(156, 160, 170), rgb(212, 215, 224)]
        } else {
            // Hover preserves the slate context, reserving the darker connected surface for selection.
            fill = dark ? [rgb(47, 51, 63), rgb(47, 51, 63), rgb(40, 44, 56)]
                        : [rgb(244, 245, 249), rgb(244, 245, 249), rgb(232, 234, 240)]
            stroke = dark ? [rgb(77, 81, 93), rgb(77, 81, 93), titlebarColor(dark: true)]
                          : [rgb(183, 187, 198), rgb(183, 187, 198), titlebarColor(dark: false)]
        }
        gradient(fill, locations: [0, 0.75, 1], in: rect,
                 clipping: tabPath(in: rect, closed: true), context: context)
        let borderColors = increasedContrast ? [stroke[0], stroke[0], stroke[0]] : stroke
        let opacity: CGFloat = keyWindow || increasedContrast ? 1 : 0.55 // Background-window selection remains identifiable.
        let outline = tabPath(in: rect, closed: false).copy(strokingWithWidth: pixel,
            lineCap: .butt, lineJoin: .round, miterLimit: 2)
        gradient(borderColors.map { $0.withAlphaComponent(opacity) }, locations: [0, 0.58, 1],
                 in: rect, clipping: outline, context: context)
    }

    /// Match the mockup's small circular add button with a restrained fill and border gradient.
    static func drawAddButton(in bounds: CGRect, highlighted: Bool, dark: Bool, scale: CGFloat, context: CGContext) {
        let pixel = 1 / max(1, scale)
        let diameter = min(bounds.width, bounds.height) - 2 // Leave room for a native keyboard focus ring.
        guard diameter > 0 else { return }
        let rect = CGRect(x: bounds.midX - diameter / 2, y: bounds.midY - diameter / 2,
                          width: diameter, height: diameter).insetBy(dx: pixel / 2, dy: pixel / 2)
        let body = CGPath(ellipseIn: rect, transform: nil)
        let fill = dark ? [rgb(70, 76, 90), rgb(55, 61, 73)] : [rgb(253, 253, 255), rgb(215, 219, 229)]
        gradient(highlighted ? fill.reversed() : fill, locations: [0, 1], in: rect, clipping: body, context: context)
        let edge = body.copy(strokingWithWidth: pixel, lineCap: .butt, lineJoin: .round, miterLimit: 2)
        let stroke = dark ? [rgb(114, 121, 136, alpha: 0.6), rgb(83, 90, 106, alpha: 0.35)]
                          : [rgb(155, 164, 181, alpha: 0.6), rgb(188, 196, 212, alpha: 0.35)]
        gradient(stroke, locations: [0, 1], in: rect, clipping: edge, context: context)
    }
}

/// Opaque color beneath AppKit's transparent titlebar; the terminal stays below its content layout guide.
/// Keeping this separate from the renderer avoids modifying its layer, dimensions or ownership.
@MainActor
final class TitlebarBackgroundView: NSView {
    /// This view paints every pixel of its background; the terminal container covers the content portion.
    override var isOpaque: Bool { true }
    /// Native empty titlebar space remains usable for moving the window.
    override var mouseDownCanMoveWindow: Bool { true }
    /// Draw an exact mockup color instead of relying on a translucent system toolbar material.
    override func draw(_ dirtyRect: NSRect) {
        TabChrome.titlebarColor(dark: TabChrome.isDark(effectiveAppearance)).setFill()
        dirtyRect.fill()
    }
    /// Redraw the light/dark palette when macOS changes appearance.
    override func viewDidChangeEffectiveAppearance() { super.viewDidChangeEffectiveAppearance(); needsDisplay = true }
}

/// Keep native NSButton target/action, accessibility and keyboard behavior with a custom circular bezel.
@MainActor
final class NewTabButton: NSButton {
    /// Prevent the toolbar from sizing the circle to the smaller intrinsic SF Symbol dimensions.
    override var intrinsicContentSize: NSSize { NSSize(width: 28, height: 28) }

    /// Paint only the bezel; AppKit still draws the SF Symbol and its native focus indication.
    override func draw(_ dirtyRect: NSRect) {
        if let context = NSGraphicsContext.current?.cgContext {
            // NSButton drawing is flipped; translate its graphics state so shared gradient geometry points upward.
            context.saveGState()
            if isFlipped { context.translateBy(x: 0, y: bounds.height); context.scaleBy(x: 1, y: -1) }
            TabChrome.drawAddButton(in: bounds, highlighted: isHighlighted, dark: TabChrome.isDark(effectiveAppearance),
                                   scale: window?.backingScaleFactor ?? 2, context: context)
            context.restoreGState()
        }
        super.draw(dirtyRect)
    }
    /// The bezel uses explicit colors, so an appearance change must invalidate it as well as the symbol.
    override func viewDidChangeEffectiveAppearance() { super.viewDidChangeEffectiveAppearance(); needsDisplay = true }
    /// Recompute the one-pixel border after moving between standard and Retina displays.
    override func viewDidChangeBackingProperties() { super.viewDidChangeBackingProperties(); needsDisplay = true }
}
