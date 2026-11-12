// Standard pointer delivery requires no retained native drag-source event.
import AppKit
extension TerminalSurfaceView {
    func preserveReplyDrag(_ code: UInt32, event: NSEvent, point: NSPoint) {}
}
