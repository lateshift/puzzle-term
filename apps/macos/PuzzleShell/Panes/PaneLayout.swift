import Foundation

/// The requested edge refers to the NEW pane; the existing pane keeps the other half.
/// Raw values are shared with PuzzleShellSplitDirection in the public C header.
enum PaneDirection: UInt32, CaseIterable {
    case left = 0, right = 1, top = 2, bottom = 3

    /// A vertical divider separates left/right panes; a horizontal divider separates top/bottom.
    var sideBySide: Bool { self == .left || self == .right }
    /// Tree order is always left-to-right or top-to-bottom, independent of insertion direction.
    var newPaneFirst: Bool { self == .left || self == .top }
}

/// A value-only binary tree records topology independently of native views and Rust sessions.
/// Branch IDs identify draggable dividers; leaf IDs identify persistent terminal surfaces.
indirect enum PaneNode: Equatable, Sendable {
    /// A leaf owns exactly one session; its ID matches the native surface registry.
    case leaf(UInt64)
    /// Each branch has two children, an axis, and a saved share of space before its divider.
    case split(id: UInt64, sideBySide: Bool, fraction: Double, first: PaneNode, second: PaneNode)

    /// Read panes in visual traversal order, useful for keyboard cycling and deterministic replay.
    var paneIDs: [UInt64] {
        switch self {
        case .leaf(let id): return [id]
        case .split(_, _, _, let first, let second): return first.paneIDs + second.paneIDs
        }
    }

    /// Replace exactly the focused leaf, preserving all neighboring branches and divider fractions.
    func inserting(_ newID: UInt64, beside target: UInt64, direction: PaneDirection, branchID: UInt64) -> PaneNode {
        switch self {
        case .leaf(let id):
            guard id == target else { return self } // Unrelated leaves preserve their identity and position.
            let old = PaneNode.leaf(id)
            let new = PaneNode.leaf(newID)
            // Left/top insertions precede the existing leaf; right/bottom insertions follow it.
            return .split(id: branchID, sideBySide: direction.sideBySide, fraction: 0.5,
                          first: direction.newPaneFirst ? new : old, second: direction.newPaneFirst ? old : new)
        case .split(let id, let axis, let fraction, let first, let second):
            // Recurse through both children while retaining this parent's identity, axis and proportion.
            return .split(id: id, sideBySide: axis, fraction: fraction,
                          first: first.inserting(newID, beside: target, direction: direction, branchID: branchID),
                          second: second.inserting(newID, beside: target, direction: direction, branchID: branchID))
        }
    }

    /// Remove one leaf and collapse its now-redundant parent into the surviving sibling.
    func removing(_ target: UInt64) -> PaneNode? {
        switch self {
        case .leaf(let id): return id == target ? nil : self
        case .split(let id, let axis, let fraction, let first, let second):
            let a = first.removing(target)
            let b = second.removing(target)
            guard let a else { return b } // Removing the first child promotes the complete second subtree.
            guard let b else { return a } // Removing the second child promotes the complete first subtree.
            return .split(id: id, sideBySide: axis, fraction: fraction, first: a, second: b)
        }
    }

    /// Persist a dragged divider without rebuilding the native tree or disturbing keyboard focus.
    func settingFraction(_ fraction: Double, branchID: UInt64) -> PaneNode {
        switch self {
        case .leaf: return self
        case .split(let id, let axis, let old, let first, let second):
            // Only the addressed branch changes its saved value; the rest of the value tree is preserved.
            return .split(id: id, sideBySide: axis, fraction: id == branchID ? fraction : old,
                          first: first.settingFraction(fraction, branchID: branchID),
                          second: second.settingFraction(fraction, branchID: branchID))
        }
    }
}

/// Mutation policy is kept headless so direction, collapse, selection and ratio retention are testable.
struct PaneLayout {
    /// A live tab always has a root; closing its last pane is delegated to the tab-close path.
    private(set) var root: PaneNode
    /// Selection remains stable through resize and reparenting, and is independent of the tab ID.
    private(set) var activeID: UInt64

    /// Start with one terminal surface filling the whole content area.
    init(initialPaneID: UInt64) {
        precondition(initialPaneID != 0, "Zero is reserved for failed pane commands")
        root = .leaf(initialPaneID)
        activeID = initialPaneID
    }

    /// Reject stale IDs; clicking or explicitly selecting a live pane makes it the split target.
    @discardableResult
    mutating func select(_ id: UInt64) -> Bool {
        guard root.paneIDs.contains(id) else { return false }
        activeID = id
        return true
    }

    /// Allocate IDs outside the model, validating uniqueness before changing any topology.
    @discardableResult
    mutating func split(direction: PaneDirection, paneID: UInt64, branchID: UInt64) -> Bool {
        guard paneID != 0, branchID != 0, paneID != branchID,
              !root.allIDs.contains(paneID), !root.allIDs.contains(branchID) else { return false }
        // Replacing one leaf leaves every existing branch and neighboring session intact.
        root = root.inserting(paneID, beside: activeID, direction: direction, branchID: branchID)
        activeID = paneID // The new session immediately receives keyboard input.
        return true
    }

    /// Close a non-final pane; choose a surviving neighbor only when the focused pane was removed.
    @discardableResult
    mutating func close(_ id: UInt64) -> Bool {
        let before = root.paneIDs
        guard before.count > 1, let index = before.firstIndex(of: id), let remaining = root.removing(id) else { return false }
        root = remaining
        if activeID == id { activeID = root.paneIDs[min(index, root.paneIDs.count - 1)] } // Prefer the next visual neighbor, otherwise the preceding one.
        return true
    }

    /// A proportion is strictly inside the divider's travel range; invalid Rust/UI data changes nothing.
    @discardableResult
    mutating func resize(branchID: UInt64, fraction: Double) -> Bool {
        guard fraction.isFinite, fraction > 0, fraction < 1, root.branchIDs.contains(branchID) else { return false }
        root = root.settingFraction(fraction, branchID: branchID)
        return true
    }

    /// Wrap traversal in both directions using positive modulo arithmetic.
    mutating func cycle(by delta: Int) {
        let ids = root.paneIDs
        guard let index = ids.firstIndex(of: activeID) else { return }
        // Reduce delta first to avoid overflow; the second modulo turns negative remainders positive.
        activeID = ids[((index + delta % ids.count) % ids.count + ids.count) % ids.count]
    }
}

extension PaneNode {
    /// Include branch identities in validation so a divider can never reuse a live surface ID.
    var allIDs: [UInt64] { paneIDs + branchIDs }
    /// Rust may address each divider by this stable ID when setting its proportion.
    var branchIDs: [UInt64] {
        switch self {
        case .leaf: return []
        case .split(let id, _, _, let first, let second): return [id] + first.branchIDs + second.branchIDs
        }
    }
}
