# macOS example host

This AppKit application demonstrates embedding Puzzle through its C ABI. The
host owns windows, tabs, split panes, keyboard/IME input and native services.
Puzzle owns each terminal session, PTY, shared rendering resources and terminal
content. No protocol parsing belongs in the Swift host.

From the repository root, with Xcode and the pinned Rust toolchain installed:

```sh
cargo xtask app macos
```

The command builds `puzzle-ffi` as a release static library under
`target/apps-macos/release`, builds `PuzzleShell.xcodeproj` with Puzzle enabled,
and runs the headless and native host test scripts. Native tests create windows;
run them in an active desktop session. Code signing is disabled for this local
build. You can also open the Xcode project; its build script locates the workspace
relative to the project instead of using a developer-specific path.

`Config/Puzzle.xcconfig` defines the header location and library linkage.
`PuzzleShell/Integration/PuzzleGlue` adapts native events and services to
`puzzle.h`. Worker wakeups schedule `puzzle_app_tick` on the main thread, where
copied callback values are delivered to their original surface.

On macOS, Puzzle adds an IOSurface-backed child layer to the host container.
The host forwards exact physical pixel extents and scale. Puzzle owns the child,
its top-left contents and background. A surface must be freed synchronously
before its native container is destroyed. See the shared
[native presentation contract](../../docs/native-presentation.md) and
[embedding contracts](../../docs/embedding/search.md).

The library-free placeholder mode (`PUZZLE_ENABLED=NO`) is useful for testing
native window/pane models. It does not substitute for a working terminal build.
