import Foundation

/// Session-owned presentation data, independent of AppKit, a renderer, or the containing window.
/// Keeping the raw runtime title separate from a custom name prevents OSC title updates from
/// overwriting the user's label. Display-only duplicate suffixes never enter this stored state.
struct TabMetadata: Equatable, Sendable {
    /// Assigned once from an app-wide counter; closing, moving, and collecting never renumber it.
    let fallbackName: String
    /// Raw, valid single-line title supplied by Rust; an empty/blank value uses the fallback.
    var automaticTitle = ""
    /// An optional user/Rust override; clearing it reveals the latest runtime title immediately.
    var customName: String?
    /// Optional packed sRGB 0xRRGGBBAA; nil means native neutral styling, including alpha=0 as valid.
    var colorRGBA: UInt32?

    /// Resolve blank runtime titles consistently in the tab, tooltip, and rename-sheet placeholder.
    var automaticName: String {
        automaticTitle.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? fallbackName : automaticTitle
    }

    /// Resolve a session's name without assigning any window-specific disambiguation suffix.
    var effectiveTitle: String { customName ?? automaticName }

    /// Reject controls/newlines and excessive UTF-8 lengths before updating titles or Cocoa labels.
    static func validTitle(_ title: String) -> Bool {
        // C0/C1 and Unicode line separators cannot belong to a single-line tab label.
        // Keep format characters such as ZWJ, which are needed by normal compound emoji.
        title.utf8.count <= 4096 && !title.unicodeScalars.contains {
            $0.value < 0x20 || (0x7F...0x9F).contains($0.value) || CharacterSet.newlines.contains($0)
        }
    }

    /// Custom names trim surrounding whitespace; clearing the field resets automatic naming.
    mutating func setCustomName(_ name: String) {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        customName = trimmed.isEmpty ? nil : trimmed
    }

    /// Select an explicit naming layer for the C getter; unknown kinds fail instead of guessing.
    /// The containing window supplies the display label, keeping window-dependent data out of sessions.
    func title(kind: UInt32, displayTitle: String?) -> String? {
        switch kind {
        case PUZZLE_TITLE_AUTOMATIC.rawValue: return automaticTitle
        case PUZZLE_TITLE_CUSTOM.rawValue: return customName ?? ""
        case PUZZLE_TITLE_EFFECTIVE.rawValue: return effectiveTitle
        case PUZZLE_TITLE_DISPLAY.rawValue: return displayTitle
        case PUZZLE_TITLE_FALLBACK.rawValue: return fallbackName
        default: return nil
        }
    }

    /// Derive unique labels within one window, stable across reordering because IDs break ties.
    /// Reserve every real name first so a generated suffix cannot impersonate another custom name.
    static func displayTitles(_ titles: [(id: UInt64, title: String)]) -> [UInt64: String] {
        let counts = Dictionary(grouping: titles, by: { $0.title }).mapValues(\.count) // Count exact effective-name duplicates.
        var reserved = Set(titles.map(\.title)) // Actual names always take precedence over generated labels.
        var result: [UInt64: String] = [:]
        var nextSuffix: [String: Int] = [:]
        for entry in titles.sorted(by: { $0.id < $1.id }) { // Creation identity, not strip order, determines suffix assignment.
            if counts[entry.title] == 1 { result[entry.id] = entry.title; continue }
            var suffix = nextSuffix[entry.title] ?? 1
            var label = "\(entry.title) · \(suffix)"
            while reserved.contains(label) { // Skip both real names and labels already assigned in this pass.
                suffix += 1
                label = "\(entry.title) · \(suffix)"
            }
            reserved.insert(label)
            nextSuffix[entry.title] = suffix + 1
            result[entry.id] = label
        }
        return result
    }

    /// Snapshot source windows in creation order, retaining each source's current tab order.
    /// The destination's own tabs stay at the front and must never appear in the move plan.
    static func collectionPlan(destination: UInt64, windows: [(id: UInt64, tabs: [UInt64])]) -> [UInt64] {
        windows.filter { $0.id != destination }.sorted { $0.id < $1.id }.flatMap(\.tabs)
    }
}
