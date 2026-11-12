// M18.5: AppKit acceptance cache for one registration; OSC state and replies belong exclusively to Puzzle.
import AppKit
#if PUZZLE_ENABLED

/// Optional native destination state exists only while a terminal program has registered.
@MainActor
final class PuzzleDropTarget {
    /// Exact decimal epoch from puzzle.h, independent of the protocol client's multiplexer ID.
    let generation: UInt64
    /// Requested native representations; empty means the host's curated register-all subset.
    let names: [String]
    /// A native drag sequence prevents an old accepted hover from approving a different gesture.
    private(set) var sequence: Int?
    /// Board generation guards a changed native offer between acceptance and actual capture.
    private(set) var revision = 0
    /// Original metadata-only offer, in the same order sent to Puzzle.
    private(set) var offered: [String] = []
    /// The last forwarded view-local point suppresses redundant stationary periodic samples.
    private var point: NSPoint?
    /// Source-authorized operations use the C copy/move bits, not AppKit's different move bit.
    private(set) var operations: UInt32 = 0
    /// Initially rejected; only a live drop/acceptance callback can change the native affordance.
    private(set) var operation: NSDragOperation = []
    /// Program preference order is retained for capture and the actual Drop MIME list.
    private var preferred: [String] = []

    /// Creation always starts with no active drag and no accepted operation.
    init(generation: UInt64, names: [String]) { self.generation = generation; self.names = names }

    /// Forward changes only; new native sequences or changed offers must negotiate acceptance again.
    func update(sequence: Int, revision: Int, offered: [String], point: NSPoint, operations: UInt32) -> Bool {
        let changedOffer = self.sequence != sequence || self.revision != revision || self.offered != offered || self.operations != operations
        let changed = changedOffer || self.point != point
        if changedOffer { operation = []; preferred = [] }
        self.sequence = sequence
        self.revision = revision
        self.offered = offered
        self.point = point
        self.operations = operations
        return changed
    }

    /// AppKit periodically asks draggingUpdated even while stationary, so asynchronous feedback is visible.
    func accept(operation: UInt32, preferred: [String]) {
        guard sequence != nil else { return } // A late callback after leave cannot grant a new gesture.
        self.preferred = preferred.reduce(into: [String]()) { if !$0.contains($1) { $0.append($1) } }
        let allowed = operation & operations
        self.operation = allowed == 1 ? .copy : allowed == 2 ? .move : []
        if captureNames.isEmpty { self.operation = [] }
    }

    /// A blank preference accepts the entire offer; otherwise keep only the requested offered alternatives.
    var captureNames: [String] {
        preferred.isEmpty ? offered : preferred.filter { offered.contains($0) }
    }

    /// Release native gesture state on leave, actual drop, protocol conclusion, replacement or close.
    /// Puzzle owns captured bytes after the copying C call, so AppKit retains no completed drag data.
    func clear() {
        sequence = nil
        revision = 0
        offered = []
        point = nil
        operations = 0
        operation = []
        preferred = []
    }
}
#endif
