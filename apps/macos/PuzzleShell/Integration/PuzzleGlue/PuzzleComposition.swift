// NSTextInputClient uses UTF-16; puzzle.h's provisional caret uses a UTF-8 byte boundary.
import Foundation

/// Conversion shared with headless tests, independent of a live Puzzle surface or AppKit event loop.
enum PuzzleComposition {
    /// Invalid, absent and surrogate-interior offsets become SIZE_MAX instead of splitting Unicode.
    static func byteOffset(_ text: String, utf16: UInt32) -> Int {
        guard utf16 != UInt32.max, Int(utf16) <= text.utf16.count else { return Int(bitPattern: UInt.max) }
        let index = text.utf16.index(text.utf16.startIndex, offsetBy: Int(utf16))
        guard let utf8Index = index.samePosition(in: text.utf8) else { return Int(bitPattern: UInt.max) }
        return text.utf8.distance(from: text.utf8.startIndex, to: utf8Index)
    }
}
