// Explicit app-only measurement fixture. Normal launches install no timer, command or file writer.
import AppKit
#if PUZZLE_ENABLED
import QuartzCore
#endif

/// Opt-in counters measure the actual login-shell, native layer and AppKit queue path.
@MainActor
enum PuzzleAppProbe {
    /// No dictionary traffic or clock reads occur unless a manifest was explicitly supplied.
    private static var enabled = false
    /// Main-queue blocks that actually executed, separate from worker enqueue decisions.
    private static var blocks = 0
    /// Calls into puzzle_app_tick during the current benchmark interval.
    private static var ticks = 0
    /// Callback names reveal facts the host decodes despite not displaying them.
    private static var events: [String: Int] = [:]
    /// Deferred host effects are separate main-queue blocks in the baseline adapter.
    private static var hostBlocks = 0
    /// Tick callback time includes decoding and enqueueing, measured without per-event logging.
    private static var tickSeconds = 0.0
    /// Native effects run after tick; measuring only tick would miss IME invalidation and title work.
    private static var hostSeconds: [String: Double] = [:]
    /// Retain the longest native-effect block rather than hiding stalls in an average.
    private static var maxHostSeconds = 0.0
    /// A probe-only polling timer watches the benchmark's completion receipt.
    private static var timer: Timer?
    /// One-second snapshots isolate flood traffic from the untimed corpus-loader interval.
    private static var samples: [[String: Any]] = []
    /// Main-actor sampling state avoids mutable captures in Timer's sendable callback.
    private static var sampledSecond = -1

    /// Called at main-queue block entry, after any worker-to-main scheduling delay.
    static func blockRan() { if enabled { blocks += 1 } }
    /// Called immediately before the real C tick operation.
    static func tickRan() { if enabled { ticks += 1 } }
    /// Count copied callback names before the normal host decoder runs.
    static func event(_ name: String) { if enabled { events[name, default: 0] += 1 } }
    /// Count additional native-effect queue submissions, not just wakeup blocks.
    static func hostBlock() { if enabled { hostBlocks += 1 } }
    /// Optional timing starts only for explicit measurements.
    static func tickStart() -> Double? { enabled ? Date.timeIntervalSinceReferenceDate : nil }
    /// Accumulate C tick plus its synchronous Swift callbacks without writing in the hot path.
    static func tickEnd(_ start: Double?) {
        if let start { tickSeconds += Date.timeIntervalSinceReferenceDate - start }
    }

    /// The same opt-in clock starts at native-effect block entry, excluding queue wait.
    static func hostEnd(_ start: Double?, name: String) {
        guard let start else { return }
        let duration = Date.timeIntervalSinceReferenceDate - start
        hostSeconds[name, default: 0] += duration
        maxHostSeconds = max(maxHostSeconds, duration)
    }

    /// A JSON manifest supplies native frame, shell command and output directory; it adds no app defaults.
    static func start(owner: TerminalWindowController) {
        #if PUZZLE_ENABLED
        let arguments = ProcessInfo.processInfo.arguments
        guard let index = arguments.firstIndex(of: "--puzzle-probe"), index + 1 < arguments.count,
              let data = try? Data(contentsOf: URL(fileURLWithPath: arguments[index + 1])),
              let manifest = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let directory = manifest["directory"] as? String,
              let command = manifest["command"] as? String,
              let window = owner.window,
              let runtime = PuzzleGlue.shared.runtime as? PuzzleRuntime else { return }
        if let pane = owner.selectedTab?.surface, let handle = runtime.surfaces[pane.paneID] {
            // Both sides of the requested comparison must use 15 logical pixels regardless of defaults.
            runtime.report(PuzzleRuntime.withBytes("{\"font\":{\"size\":15}}") {
                puzzle_surface_set_config(handle, $0)
            }, operation: "Set measurement font size")
        }
        if let frame = manifest["frame"] as? [Double], frame.count == 4 {
            window.setFrame(NSRect(x: frame[0], y: frame[1], width: frame[2], height: frame[3]), display: true)
        }
        window.makeKeyAndOrderFront(nil)
        // Give the real login shell time to finish its user's ordinary startup files.
        DispatchQueue.main.asyncAfter(deadline: .now() + 2) {
            begin(owner: owner, runtime: runtime, directory: directory, command: command)
        }
        #endif
    }

    #if PUZZLE_ENABLED
    /// Start one measured workload after layout and normal shell startup have completed.
    private static func begin(owner: TerminalWindowController, runtime: PuzzleRuntime, directory: String, command: String) {
        guard let pane = owner.selectedTab?.surface, let handle = runtime.surfaces[pane.paneID] else { return }
        enabled = true
        puzzle_tick_measure(1)
        let start = Date()
        let wakeStart = puzzle_tick_wakeups()
        let queuedStart = puzzle_tick_blocks()
        let observation = pane.surfaceEvent(PUZZLE_SURFACE_CHANGED)
        let geometry = (observation.width_px, observation.height_px, observation.scale)
        let initialFrame = owner.window.map { [Double($0.frame.minX), Double($0.frame.minY), Double($0.frame.width), Double($0.frame.height)] } ?? []
        let grid = runtime.cells[pane.paneID].map { [$0.2, $0.3, $0.0, $0.1] } ?? []
        // The manifest is local developer input. This uses the same Puzzle text API as Cocoa,
        // never a second PTY, parser, GPU loop, or global keyboard injection.
        let result = PuzzleRuntime.withBytes(command + "\r") { puzzle_surface_text(handle, $0) }
        guard result == PUZZLE_OK else { return }
        timer = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { _ in
            MainActor.assumeIsolated {
                let done = FileManager.default.fileExists(atPath: directory + "/done")
                let second = Int(Date().timeIntervalSince(start))
                if second != sampledSecond {
                    sampledSecond = second
                    samples.append(["second": second, "elapsed": Date().timeIntervalSince(start),
                                    "wakeups": puzzle_tick_wakeups() - wakeStart,
                                    "blocks": puzzle_tick_blocks() - queuedStart, "ticks": ticks,
                                    "hostBlocks": hostBlocks, "hostSeconds": hostSeconds, "events": events,
                                    "corpusLoaded": FileManager.default.fileExists(atPath: directory + "/loaded")])
                }
                guard done || Date().timeIntervalSince(start) > 90 else { return }
                finish(owner: owner, directory: directory, start: start, wakeStart: wakeStart,
                       queuedStart: queuedStart, geometry: geometry, initialFrame: initialFrame,
                       initialGrid: grid, completed: done)
            }
        }
    }

    /// Publish one result after the flood; normal library callbacks never log per event (R10).
    private static func finish(owner: TerminalWindowController, directory: String, start: Date,
                               wakeStart: UInt64, queuedStart: UInt64, geometry: (UInt32, UInt32, Double),
                               initialFrame: [Double], initialGrid: [UInt32], completed: Bool) {
        timer?.invalidate()
        timer = nil
        puzzle_tick_measure(0)
        enabled = false
        let elapsed = Date().timeIntervalSince(start)
        let pane = owner.selectedTab?.surface
        let runtime = PuzzleGlue.shared.runtime as? PuzzleRuntime
        let finalGrid = pane.flatMap { runtime?.cells[$0.paneID] }.map { [$0.2, $0.3, $0.0, $0.1] } ?? []
        let finalFrame = owner.window.map { [Double($0.frame.minX), Double($0.frame.minY), Double($0.frame.width), Double($0.frame.height)] } ?? []
        let finalGeometry = pane?.surfaceEvent(PUZZLE_SURFACE_CHANGED)
        let stable = finalFrame == initialFrame && finalGrid == initialGrid
            && finalGeometry?.width_px == geometry.0 && finalGeometry?.height_px == geometry.1
            && finalGeometry?.scale == geometry.2
        let report: [String: Any] = [
            "elapsed": elapsed, "completed": completed,
            "wakeups": puzzle_tick_wakeups() - wakeStart, "enqueuedTickBlocks": puzzle_tick_blocks() - queuedStart,
            "executedTickBlocks": blocks, "ticks": ticks, "hostBlocks": hostBlocks,
            "tickSeconds": tickSeconds, "events": events,
            "hostSecondsByKind": hostSeconds, "maxHostBlockSeconds": maxHostSeconds,
            "coalescedTicks": coalesced,
            "width": geometry.0, "height": geometry.1, "scale": geometry.2,
            "initialFrame": initialFrame, "grid": Array(initialGrid.prefix(2)),
            "cellPixels": Array(initialGrid.suffix(2)), "finalGridAndCellPixels": finalGrid,
            "stableGeometry": stable, "fontSize": 15, "seconds": samples,
            "display": (owner.window?.screen?.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?.uint64Value ?? 0,
            "frame": finalFrame,
            // Winit uses a desktop top-left origin; AppKit uses the primary
            // screen's bottom-left. Preserve the conversion witness, not a guess
            // that (208, 0) means the same coordinate in both toolkit APIs.
            "primaryScreenFrame": NSScreen.screens.first.map { [$0.frame.minX, $0.frame.minY, $0.frame.width, $0.frame.height] } ?? [],
            "screenFrame": owner.window?.screen.map { [$0.frame.minX, $0.frame.minY, $0.frame.width, $0.frame.height] } ?? [],
            "environment": ProcessInfo.processInfo.environment.filter { $0.key.hasPrefix("MTL_") || $0.key == "DYLD_INSERT_LIBRARIES" }
        ]
        if let data = try? JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys]) {
            try? data.write(to: URL(fileURLWithPath: directory + "/app.json"), options: .atomic)
        }
        NSApp.terminate(nil) // Only this explicit probe process exits; other app instances remain alone.
    }

    /// The opt-in baseline build keeps the original scheduler for controlled before/after evidence.
    private static var coalesced: Bool {
        #if PUZZLE_UNCOALESCED_TICKS
        false
        #else
        true
        #endif
    }
    #endif
}
