// Standard example host: shells and host-owned byte transports only.
import AppKit
/// No extra child-launch policy is constructible in this distribution.
enum SurfaceLaunch {}
#if PUZZLE_ENABLED
/// Standard hosts do not initiate native data exports from terminal output.
@MainActor
final class SurfaceDragSource: NSObject {
    private override init() { super.init() }
    func revoke() {}
    func allowsDrop(_ sender: NSDraggingInfo, into pane: TerminalSurfaceView) -> Bool { false }
}
#endif
/// Optional host hooks are inert; terminal lifetime and geometry stay shared.
@MainActor
enum HostExtensions {
    static func installMenu(delegate: ShellAppDelegate, menu: NSMenu) {}
    static func validate(menuItem: NSMenuItem, owner: TerminalWindowController?) -> Bool? { nil }
    #if PUZZLE_ENABLED
    static func create(runtime: PuzzleRuntime, pane: TerminalSurfaceView, options: inout puzzle_surface_options_t,
                       surface: inout puzzle_surface_t) -> puzzle_result_t {
        puzzle_surface_new(runtime.app, &options, &surface)
    }
    static func created(runtime: PuzzleRuntime, surface: puzzle_surface_t) {}
    static func binaryEvent(runtime: PuzzleRuntime, paneID: UInt64, surface: UInt64, name: String, payload: Data) -> Bool { false }
    static func fact(paneID: UInt64, name: String, value: Any?) -> Bool { false }
    #endif
}
