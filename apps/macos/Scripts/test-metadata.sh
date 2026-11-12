#!/bin/bash
# Headless verification: never initializes NSApplication, creates windows, or drives the computer.
set -euo pipefail
# Find the repository from the script location, independent of the caller's current directory.
project_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_root"
# Extra protocol fixtures are a complete optional test component.
source "$project_root/Scripts/extra-tests.sh"
# Compiler caches and binaries live in a disposable temporary directory outside source and Rust.
test_dir="$(mktemp -d "${TMPDIR:-/tmp}/PuzzleShell-metadata.XXXXXX")"
# Remove only artifacts created by this script when compilation or testing finishes.
trap 'rm -rf "$test_dir"' EXIT
# Compile a separate C caller to validate ABI layout and real exported function linkage.
xcrun clang -std=c11 -Wall -Wextra -Werror -c Tests/BridgeProbe.c -o "$test_dir/BridgeProbe.o"
xcrun clang -std=c11 -Wall -Wextra -Werror -c PuzzleShell/Integration/PuzzleGlue/PuzzleTickGate.c -o "$test_dir/PuzzleTickGate.o"
# Link production code, replacing only the application entry point with the headless test main.
sources=()
# Recursively discover the organized source tree; NUL delimiters preserve paths with spaces.
while IFS= read -r -d '' source; do
    sources+=("$source")
done < <(find PuzzleShell -name '*.swift' ! -name PuzzleShellMain.swift -print0)
# Match the project's Swift 6 actor rules; no AppKit event loop is run by MetadataTests.
xcrun swiftc -swift-version 6 -default-isolation MainActor -enable-upcoming-feature InferIsolatedConformances \
    -module-cache-path "$test_dir/ModuleCache" -import-objc-header Tests/TestBridge.h \
    "${sources[@]}" Tests/MetadataTests.swift Tests/PaneTests.swift Tests/TabDragPolicyTests.swift "${metadata_extra_sources[@]}" "$test_dir/BridgeProbe.o" "$test_dir/PuzzleTickGate.o" -o "$test_dir/MetadataTests"
"$test_dir/MetadataTests"
