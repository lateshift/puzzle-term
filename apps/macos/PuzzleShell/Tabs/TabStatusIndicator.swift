// Standard tabs reserve no status-adornment space.
import AppKit
@MainActor
final class TabStatusIndicator: NSView {
    init() { super.init(frame: .zero); isHidden = true }
    required init?(coder: NSCoder) { fatalError("Use init()") }
    var preferredWidth: CGFloat { 0 }
    func configure(_ status: TabStatus) { isHidden = true }
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
}
