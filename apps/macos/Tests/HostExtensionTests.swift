// Only ordinary host contracts belong to the standard example application.
import AppKit
@MainActor
enum HostExtensionTests {
    static func metadata() -> Int { 0 }
    #if PUZZLE_ENABLED
    static func drag(_ runtime: PuzzleRuntime) {}
    static func status(_ runtime: PuzzleRuntime, owner: TerminalWindowController) {}
    static func closed() {}
    static func editing(owner: TerminalWindowController, defaults: UserDefaults) {}
    #endif
}
