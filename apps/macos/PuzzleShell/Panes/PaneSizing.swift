import AppKit

/// Shared sizing policy protects usable terminal areas while keeping resizing proportional.
enum PaneSizing {
    /// Minimum placeholder/terminal size in points; a future cell-grid adapter can change this policy.
    static let leafMinimum = NSSize(width: 120, height: 90)
    /// A thin visible divider has a larger invisible hit area supplied by the native delegate.
    static let divider: CGFloat = 1

    /// Clamp the preferred fraction against BOTH subtrees, accommodating transient undersized frames.
    static func firstExtent(total: CGFloat, fraction: Double, firstMinimum: CGFloat, secondMinimum: CGFloat) -> CGFloat {
        let available = max(0, total - divider) // Zero layout never produces negative child dimensions.
        let required = firstMinimum + secondMinimum
        guard available >= required else {
            return required > 0 ? available * firstMinimum / required : available / 2 // Preserve relative minima until layout catches up.
        }
        return min(max(available * CGFloat(fraction), firstMinimum), available - secondMinimum)
    }
}

extension PaneNode {
    /// Nested minimum sizes add along the split axis and take the larger cross-axis requirement.
    var minimumSize: NSSize {
        switch self {
        case .leaf: return PaneSizing.leafMinimum
        case .split(_, let axis, _, let first, let second):
            let a = first.minimumSize
            let b = second.minimumSize
            return axis ? NSSize(width: a.width + b.width + PaneSizing.divider, height: max(a.height, b.height))
                        : NSSize(width: max(a.width, b.width), height: a.height + b.height + PaneSizing.divider)
        }
    }
}

