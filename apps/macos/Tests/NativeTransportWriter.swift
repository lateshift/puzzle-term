// Test writer for the current C host-transport contract: user input and receipt-bearing replies share one FIFO.
import Foundation
#if PUZZLE_ENABLED

/// Fixtures consume complete immutable payloads in memory, then return reply credit to the same Session.
@MainActor
enum NativeTransportWriter {
    /// Ordinary user input needs no receipt; program replies carry a little-endian u64 prefix.
    /// The supplied sink must synchronously consume the whole value; callback delivery alone is never acknowledged.
    static func consume(_ runtime: PuzzleRuntime, paneID: UInt64, name: String, payload: Data, write: (Data) -> Void) {
        if name == "transport/input" { write(payload); return }
        guard name == "transport/reply" else { return }
        precondition(payload.count >= 8, "C reply includes its receipt prefix")
        let receipt = payload.prefix(8).enumerated().reduce(UInt64(0)) { $0 | UInt64($1.element) << ($1.offset * 8) }
        precondition(receipt != 0)
        write(Data(payload.dropFirst(8))) // This is actual consumption by the fixture's writer, preserving callback FIFO.
        guard let surface = runtime.surfaces[paneID] else { preconditionFailure("Reply writer lost its live surface") }
        precondition(puzzle_surface_reply_complete(surface, receipt) == PUZZLE_OK)
    }
}
#endif
