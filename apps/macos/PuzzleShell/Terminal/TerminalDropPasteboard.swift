// M18.5: native drag representations; names on hover, immutable bytes only at an actual user drop.
import AppKit
import UniformTypeIdentifiers

/// One bounded native alternative; neither paths nor arbitrary bytes are opened as files.
struct TerminalDropItem {
    /// MIME spelling used by Puzzle, distinct from AppKit's type identifier.
    let mime: String
    /// Owned bytes copied during the user's drop gesture.
    let data: Data
}

/// Reads the drag pasteboard, never the general clipboard or program-supplied filesystem paths.
@MainActor
struct TerminalDropPasteboard {
    /// AppKit retains this board for the native drag; no borrowed pointers cross into Puzzle.
    let board: NSPasteboard
    /// Curated register-all subset; arbitrary explicitly registered MIME types use their OS UTI.
    static let defaults = ["text/uri-list", "text/plain", "text/html", "image/png"]
    /// The C contract bounds aggregate copies, including all alternatives together.
    static let maximumBytes = 64 * 1024 * 1024

    /// Map common names exactly; OS-known MIME types and literal MIME publishers also work.
    static func nativeType(_ mime: String) -> NSPasteboard.PasteboardType {
        switch mime {
        case "text/uri-list": return .fileURL
        case "text/plain": return .string
        case "text/html": return .html
        case "image/png": return .png
        default: return NSPasteboard.PasteboardType(UTType(mimeType: mime)?.identifier ?? mime)
        }
    }

    /// Install native interests plus file-path fallback; never register the app's private tab type.
    static func types(for names: [String]) -> [NSPasteboard.PasteboardType] {
        let candidates = names.isEmpty ? defaults : names
        var types: [NSPasteboard.PasteboardType] = [.fileURL, .string]
        for name in candidates {
            for type in [nativeType(name), NSPasteboard.PasteboardType(name)] where !types.contains(type) {
                types.append(type)
            }
        }
        return types
    }

    /// Type inspection alone does not trigger a promised-data provider or disclose a path on hover.
    func offer(_ requested: [String]) -> [String] {
        let available = Set(board.types ?? [])
        return Array((requested.isEmpty ? Self.defaults : requested).filter { mime in
            available.contains(Self.nativeType(mime)) || available.contains(NSPasteboard.PasteboardType(mime)) ||
                mime == "text/plain" && available.contains(.fileURL)
        }.prefix(16))
    }

    /// Decode only native file URLs after a user drop; never resolve/open their contents or promises.
    func fileURLs() -> [URL]? {
        guard let values = board.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL],
              !values.isEmpty, values.allSatisfy(\.isFileURL) else { return nil }
        return values
    }

    /// Capture the accepted alternatives atomically, preserving the program's preference order.
    /// A failed read, changed board or excessive aggregate rejects the whole drop, not a partial copy.
    func capture(_ names: [String], revision: Int) -> [TerminalDropItem]? {
        guard names.count <= 16, board.changeCount == revision else { return nil }
        var items: [TerminalDropItem] = []
        var total = 0
        var urls: [URL]?
        let hasFiles = board.types?.contains(.fileURL) == true
        for name in names {
            let bytes: Data?
            if hasFiles && (name == "text/uri-list" || name == "text/plain") {
                if urls == nil { urls = fileURLs() }
                guard let urls else { return nil }
                // URI lists use escaped absolute URLs; plain text carries paths without shell syntax.
                bytes = Data((name == "text/uri-list" ? urls.map(\.absoluteString).joined(separator: "\r\n") + "\r\n" :
                    urls.map(\.path).joined(separator: "\n")).utf8)
            } else if name == "text/plain" {
                bytes = (board.string(forType: .string) ?? board.string(forType: NSPasteboard.PasteboardType(name))).map { Data($0.utf8) }
            } else {
                let type = Self.nativeType(name)
                bytes = board.data(forType: type) ?? board.data(forType: NSPasteboard.PasteboardType(name))
            }
            guard let bytes, bytes.count <= Self.maximumBytes - total else { return nil }
            total += bytes.count
            items.append(TerminalDropItem(mime: name, data: bytes))
        }
        guard board.changeCount == revision else { return nil }
        return items
    }

    /// Ordinary unregistered file drops paste shell-quoted paths through Puzzle's normal paste policy.
    func quotedPaths() -> String? {
        let revision = board.changeCount
        guard let urls = fileURLs() else { return nil }
        let text = urls.map { "'" + $0.path.replacingOccurrences(of: "'", with: "'\\''") + "'" }.joined(separator: " ")
        guard text.utf8.count <= Self.maximumBytes, board.changeCount == revision else { return nil }
        return text
    }
}
