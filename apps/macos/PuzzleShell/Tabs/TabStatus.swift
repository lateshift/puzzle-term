// Standard tab chrome has no additional record table.
import Foundation
struct TabStatusRecord { let label: String }
struct TabStatus {
    var summary: TabStatusRecord? { nil }
    var tooltip: String { "" }
    mutating func removePane(_ paneID: UInt64) {}
}
