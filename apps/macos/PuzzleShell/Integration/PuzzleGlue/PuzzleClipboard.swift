// M18.3: native MIME operations and changeCount checks stay outside Puzzle's Session owner.
import AppKit
#if PUZZLE_ENABLED

/// Small OS seam lets tests force ownership changes and failures without using the user's clipboard.
@MainActor
protocol PuzzlePasteboard {
    /// Native generation changes even when earlier bytes are restored; nil cannot authorize a grant.
    var revision: UInt64? { get }
    /// Only MIME names this backend can actually read; inspecting names does not read image bytes.
    var names: [String] { get }
    /// Read a supported representation; nil indicates that native access did not produce data.
    func read(_ mime: String) -> Data?
    /// Replace all alternatives together, or clear for an empty list; return actual OS success.
    func replace(_ items: [PuzzleClipboardItem]) -> Bool
}

/// One NSPasteboardItem preserves text, HTML and PNG alternatives in a single native publication.
final class PuzzleNativePasteboard: PuzzlePasteboard {
    /// General in production; a named private pasteboard in native integration tests.
    private let pasteboard: NSPasteboard
    /// Explicit supported mapping: do not advertise TIFF as PNG without a bounded conversion path.
    private static let types: [(String, NSPasteboard.PasteboardType)] = [
        ("text/plain", .string), ("text/html", .html), ("image/png", .png)
    ]

    /// Allow private pasteboards while keeping primary and standard mapped to general in production.
    init(_ pasteboard: NSPasteboard = .general) { self.pasteboard = pasteboard }

    /// A negative OS counter is untrustworthy; it never becomes a large unsigned grant generation.
    var revision: UInt64? { UInt64(exactly: pasteboard.changeCount) }

    /// Inspect native metadata only, retaining all supported alternatives rather than choosing one.
    var names: [String] {
        let available = pasteboard.types ?? []
        return Self.types.filter { available.contains($0.1) }.map(\.0)
    }

    /// Plain text uses Cocoa's string decoding; HTML and PNG remain byte-exact representations.
    func read(_ mime: String) -> Data? {
        guard let type = Self.types.first(where: { $0.0 == mime })?.1 else { return nil }
        if mime == "text/plain" { return pasteboard.string(forType: type).map { Data($0.utf8) } }
        return pasteboard.data(forType: type)
    }

    /// Prepare every representation before clearing: unsupported or invalid input leaves contents intact.
    func replace(_ items: [PuzzleClipboardItem]) -> Bool {
        let item = NSPasteboardItem()
        for alternative in items {
            guard let type = Self.types.first(where: { $0.0 == alternative.mime })?.1,
                  item.setData(alternative.data, forType: type) else { return false }
        }
        // clearContents and writeObjects return after their pasteboard-server operations complete.
        // Publish one item with every alternative, never successive clipboard replacements.
        pasteboard.clearContents()
        return items.isEmpty || pasteboard.writeObjects([item])
    }
}

/// Owns asynchronous native work; Session still owns policy, mode precedence and secure tokens.
final class PuzzleClipboard {
    /// Shared header caps also bound untrusted native clipboard data and multi-format totals.
    static let maxBytes = 64 * 1024 * 1024
    /// This host can preserve all combinations of these native representations.
    private static let supported = ["text/plain", "text/html", "image/png"]
    /// Native or controlled test backend; never stores a surface pointer or credential.
    private let pasteboard: any PuzzlePasteboard

    /// Production uses the standard pasteboard; tests inject a private one or controlled failures.
    init(_ pasteboard: any PuzzlePasteboard = PuzzleNativePasteboard()) { self.pasteboard = pasteboard }

    /// Defer OS access beyond tick; revalidate the surface before I/O and before returning completion.
    func complete(_ request: PuzzleClipboardRequest, isLive: @escaping () -> Bool,
                  completion: @escaping (PuzzleClipboardAnswer) -> Void) {
        DispatchQueue.main.async {
            guard isLive() else { return }
            let answer = self.perform(request)
            guard isLive() else { return }
            completion(answer) // OS success/failure is known now; no earlier DONE is possible.
        }
    }

    /// Capture a user gesture's metadata and optional text from exactly one unchanged native generation.
    func pasteSnapshot() -> (revision: UInt64, names: [String], text: String?)? {
        guard let before = pasteboard.revision, before != .max else { return nil }
        let names = pasteboard.names
        let data = names.contains("text/plain") ? pasteboard.read("text/plain") : nil
        guard before == pasteboard.revision, (data?.count ?? 0) <= Self.maxBytes else { return nil }
        return (before, names, data.flatMap { String(data: $0, encoding: .utf8) })
    }

    /// Execute only policy-approved work; a grant is checked immediately around all native reads.
    private func perform(_ request: PuzzleClipboardRequest) -> PuzzleClipboardAnswer {
        switch request.operation {
        case "read": return read(request)
        case "write": return write(request.items)
        default: return PuzzleClipboardAnswer(status: .unavailable)
        }
    }

    /// Any unsupported write alternative rejects the entire set before modifying the OS clipboard.
    private func write(_ items: [PuzzleClipboardItem]) -> PuzzleClipboardAnswer {
        guard items.count <= 64, items.reduce(0, { $0 + $1.data.count }) <= Self.maxBytes else {
            return PuzzleClipboardAnswer(status: .tooLarge)
        }
        guard items.allSatisfy({ Self.supported.contains($0.mime) }), Set(items.map(\.mime)).count == items.count else {
            return PuzzleClipboardAnswer(status: .unavailable)
        }
        guard items.allSatisfy({ !$0.mime.hasPrefix("text/") || String(data: $0.data, encoding: .utf8) != nil }) else {
            return PuzzleClipboardAnswer(status: .invalid)
        }
        return PuzzleClipboardAnswer(status: pasteboard.replace(items) ? .done : .io)
    }

    /// Read only requested names, retaining request order and denying every byte on a revision race.
    private func read(_ request: PuzzleClipboardRequest) -> PuzzleClipboardAnswer {
        let before = pasteboard.revision
        if let expected = request.revision, before != expected { return PuzzleClipboardAnswer(status: .denied) }
        var answer = PuzzleClipboardAnswer(status: .done, revision: before ?? .max)
        let available = pasteboard.names
        var total = 0
        for name in request.names {
            let data: Data?
            if name == "." { data = Data(available.joined(separator: " ").utf8) }
            else if available.contains(name) { data = pasteboard.read(name) }
            else { continue } // Unavailable alternatives are omitted, never replaced with unrelated types.
            guard let data else { answer.status = .io; break }
            total += data.count
            guard total <= Self.maxBytes, answer.items.count < 64 else { answer.status = .tooLarge; break }
            answer.items.append(PuzzleClipboardItem(mime: name, data: data))
        }
        let after = pasteboard.revision
        if request.revision != nil && after != before { answer.status = .denied }
        answer.revision = after == before ? (after ?? .max) : .max
        if answer.items.isEmpty && answer.status == .done { answer.status = .unavailable }
        if answer.status != .done { answer.items.removeAll() } // Never return partially authorized data.
        return answer
    }
}
#endif
