#!/bin/bash
# Compile the same AppKit implementation with a test entry point and an independent C caller.
# This needs a logged-in macOS GUI session because it creates actual native NSWindows.
set -euo pipefail
# Resolve paths relative to this script so it also works when invoked outside the repository.
project_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_root"
# Extra protocol fixtures are a complete optional test component.
source "$project_root/Scripts/extra-tests.sh"
# Keep compiler artifacts out of both the source tree and the Rust build directories.
test_dir="$(mktemp -d "${TMPDIR:-/tmp}/PuzzleShell-tests.XXXXXX")"
# These are temporary test outputs only; remove them when the harness exits.
trap 'rm -rf "$test_dir"' EXIT
xcrun clang -std=c11 -Wall -Wextra -Werror -c Tests/BridgeProbe.c -o "$test_dir/BridgeProbe.o"
xcrun clang -std=c11 -Wall -Wextra -Werror -c PuzzleShell/Integration/PuzzleGlue/PuzzleTickGate.c -o "$test_dir/PuzzleTickGate.o"
# Explicitly exclude PuzzleShellMain.swift: the harness supplies its own @main entry point.
sources=()
# Recursively discover the organized source tree; NUL delimiters preserve paths with spaces.
while IFS= read -r -d '' source; do
    sources+=("$source")
done < <(find PuzzleShell -name '*.swift' ! -name PuzzleShellMain.swift -print0)
# Normal native verification includes the real C library; explicit NO keeps the standalone host mode.
# A nonempty vector also supports macOS Bash 3.2 with nounset in placeholder mode.
compile_flags=(-swift-version 6 -default-isolation MainActor -enable-upcoming-feature InferIsolatedConformances)
if [[ "${PUZZLE_ENABLED:-YES}" == "YES" ]]; then
    compile_flags+=(-D PUZZLE_ENABLED -D PUZZLE_GLUE_TESTS -Xcc -DPUZZLE_ENABLED=1
        -I ../../crates/puzzle-ffi/include ../../target/apps-macos/release/libpuzzle_ffi.a
        -lc++ -liconv -framework Metal -framework QuartzCore -framework CoreVideo
        -framework CoreText -framework CoreGraphics -framework AppKit -framework Foundation -framework UserNotifications
        -framework IOKit -framework IOSurface -framework Security -framework SystemConfiguration)
fi
# Mirror the Xcode project's Swift 6 actor isolation and inferred AppKit conformances.
xcrun swiftc "${compile_flags[@]}" -module-cache-path "$test_dir/ModuleCache" \
    -import-objc-header Tests/TestBridge.h "${sources[@]}" Tests/ShellTests.swift \
    Tests/PuzzleRuntimeTests.swift Tests/NativePresentationTests.swift Tests/NativeSelectionTests.swift Tests/NativeClipboardTests.swift Tests/NativeNotificationTests.swift Tests/NativeDropFixture.swift Tests/NativeDropTests.swift Tests/NativeTabShortcutTests.swift Tests/NativeEditingTests.swift Tests/NativeTransportWriter.swift \
    "${native_extra_sources[@]}" "$test_dir/BridgeProbe.o" "$test_dir/PuzzleTickGate.o" -o "$test_dir/ShellTests"
# --compile-only verifies harness compatibility without executing any code that creates windows.
if [[ "${1:-}" == "--compile-only" ]]; then
    echo "PASS: native integration harness compiled; no GUI tests executed."
else
    "$test_dir/ShellTests"
fi
