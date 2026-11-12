#!/bin/bash
# Build only Puzzle's native C library in a private target directory, leaving the main instance's lock alone.
set -euo pipefail
# Locate the workspace from this script so callers can run it from any working directory.
macos_root="$(cd "$(dirname "$0")/.." && pwd)"
workspace_root="$(cd "$macos_root/../.." && pwd)"
cd "$workspace_root"
# D-56/M17.ffi supplies the header; Phase A does not fabricate or copy a substitute ABI.
if [[ ! -f crates/puzzle-ffi/include/puzzle.h ]]; then
    echo "Puzzle's header is not available yet: wait for the committed M17.ffi puzzle.h." >&2
    exit 1
fi
# Override any inherited CARGO_TARGET_DIR: this script must never share the primary instance's build lock.
# macOS ships Bash 3.2: under nounset, expanding an empty array is an error.
# Keep one nonempty argument vector so the ordinary library-only command works too.
# Keep Xcode's documented feature contract and command-line builds in one place.
cargo_features="$(sed -n 's/^PUZZLE_CARGO_FEATURES = //p' "$macos_root/Config/Puzzle.xcconfig")"
if [[ -z "$cargo_features" ]]; then
    echo "Config/Puzzle.xcconfig must declare PUZZLE_CARGO_FEATURES." >&2
    exit 1
fi
build_args=(--release -p puzzle-ffi --features "$cargo_features")
if [[ "${1:-}" == "--with-harness" || "${1:-}" == "--with-harness-lifecycle" ]]; then
    # One Cargo invocation unifies core feature compilation for the matched app/harness pair.
    # This prepares a normal Release harness; it does not start any window or diagnostic workload.
    build_args+=(-p puzzle-desktop)
    if [[ "$1" == "--with-harness-lifecycle" ]]; then
        # The user requested the existing winit lifecycle script for exact geometry.
        # Enable its approved diagnostic code in both linked engines, without a
        # byte recording, frame verification, pressure driver or timed stage sink.
        # Removing the static debug ceiling retains the cold native-frame readback.
        build_args+=(--no-default-features --features puzzle-desktop/lock-metrics)
    fi
elif [[ $# -ne 0 ]]; then
    echo "Usage: $0 [--with-harness|--with-harness-lifecycle]" >&2
    exit 1
fi
CARGO_TARGET_DIR="$workspace_root/target/apps-macos" cargo build "${build_args[@]}"
