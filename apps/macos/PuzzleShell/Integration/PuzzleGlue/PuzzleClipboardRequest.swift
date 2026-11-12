// M18.3: owned clipboard requests and call-scoped C buffers; no protocol credentials live here.
import Foundation
#if PUZZLE_ENABLED

/// One native clipboard alternative, retaining binary bytes without a text conversion.
struct PuzzleClipboardItem {
    /// Protocol MIME spelling, independent of the pasteboard's native type identifier.
    let mime: String
    /// Owned representation copied from a callback or the OS.
    let data: Data
}

/// Native completion tags match puzzle.h; only Puzzle assembles OSC acknowledgements.
enum PuzzleClipboardStatus: UInt32 {
    /// Every requested native write succeeded, or requested readable alternatives were collected.
    case done = 0
    /// This host cannot provide the requested representations or operation.
    case unavailable = 1
    /// A gesture's clipboard generation no longer matches its grant.
    case denied = 2
    /// Native clipboard access failed after the operation was attempted.
    case io = 4
    /// Text bytes were not valid UTF-8 or the callback shape was invalid.
    case invalid = 5
    /// Combined native representations exceeded the header's byte bound.
    case tooLarge = 6
}

/// A completion proves OS work finished; enqueueing a request is never success.
struct PuzzleClipboardAnswer {
    /// Native result forwarded without substituting success on failure.
    var status: PuzzleClipboardStatus
    /// Unchanged generation on a successful read; the sentinel means no trusted revision.
    var revision: UInt64 = .max
    /// Requested read alternatives only; writes and failures carry no data.
    var items: [PuzzleClipboardItem] = []
}

/// Decodes the authoritative owned JSON callback after its borrowed C buffers were copied.
struct PuzzleClipboardRequest {
    /// Decimal correlation ID from embed; zero is an API write with no acknowledgement.
    let token: UInt64
    /// Native operation requested after Puzzle's policy interceptors have run.
    let operation: String
    /// A grant's expected native changeCount; nil means an explicitly allowed general read.
    let revision: UInt64?
    /// Read names in request order, including the special name-list representation ".".
    let names: [String]
    /// Complete write alternatives; never silently reduce this set to plain text.
    let items: [PuzzleClipboardItem]

    /// Preserve 64-bit decimal identities exactly, avoiding floating-point JSON conversions.
    init?(_ value: Any?) {
        guard let map = value as? [String: Any], let tokenText = map["token"] as? String,
              let token = UInt64(tokenText), let operation = map["operation"] as? String,
              map["primary"] is Bool else { return nil }
        self.token = token
        self.operation = operation
        if let text = map["revision"] as? String {
            guard let revision = UInt64(text), revision != .max else { return nil }
            self.revision = revision
        } else {
            guard map["revision"] is NSNull else { return nil }
            revision = nil
        }
        // Both primary locations use NSPasteboard.general, matching the header's OSC 52 mapping.
        if operation == "read" {
            guard let names = map["items"] as? [String] else { return nil }
            self.names = names
        } else { names = [] }
        // A malformed write must never turn into an empty-list clear operation.
        let encodedItems: [[String: Any]]
        if operation == "write" {
            guard let encoded = map["items"] as? [[String: Any]] else { return nil }
            encodedItems = encoded
        } else { encodedItems = [] }
        var alternatives: [PuzzleClipboardItem] = []
        for item in encodedItems {
            guard let mime = item["type"] as? String, let bytes = item["data"] as? [NSNumber],
                  bytes.allSatisfy({ $0.doubleValue >= 0 && $0.doubleValue <= 255 && $0.doubleValue.rounded() == $0.doubleValue }) else { return nil }
            alternatives.append(PuzzleClipboardItem(mime: mime, data: Data(bytes.map(\.uint8Value))))
        }
        items = alternatives
    }
}

/// Keeps every MIME/data allocation alive together until the synchronous copying C call returns.
enum PuzzleClipboardBuffers {
    /// Recursion nests buffer borrows (at most 64), rather than storing escaped Swift pointers.
    static func withItems<T>(_ items: [PuzzleClipboardItem], _ body: (UnsafeBufferPointer<puzzle_mime_item_t>) -> T) -> T {
        var native: [puzzle_mime_item_t] = []
        /// Each recursive level retains one MIME and data borrow for the final array call.
        func borrow(_ index: Int) -> T {
            guard index < items.count else { return native.withUnsafeBufferPointer(body) }
            return PuzzleRuntime.withBytes(items[index].mime) { mime in
                items[index].data.withUnsafeBytes { bytes in
                    native.append(puzzle_mime_item_t(mime: mime, data: puzzle_bytes_t(
                        data: bytes.bindMemory(to: UInt8.self).baseAddress, len: bytes.count)))
                    defer { native.removeLast() }
                    return borrow(index + 1)
                }
            }
        }
        return borrow(0)
    }

    /// Paste names have no data; reuse the same nested lifetime proof for their borrowed buffers.
    static func withNames<T>(_ names: [String], _ body: (UnsafeBufferPointer<puzzle_bytes_t>) -> T) -> T {
        withItems(names.map { PuzzleClipboardItem(mime: $0, data: Data()) }) { items in
            items.map(\.mime).withUnsafeBufferPointer(body)
        }
    }
}
#endif
