//! Trunk ships hashed JS names; their declarations come from the same Rust exports.
//!
//! The CLI reads the unoptimized Cargo artifact for type metadata only.
//! Its duplicate JS/wasm output stays in target/: copying that wasm would
//! replace Trunk's measured wasm-opt result with a different engine artifact.
//! Only the declaration is attached to the final hashed browser module.
//!
//! Rust attributes supply method names, promise results and checked-input
//! interfaces. Running wasm-bindgen prevents a handwritten class declaration
//! from drifting when the implementation changes. The custom event overload
//! remains merged with that generated class, so the six M7 names stay typed.
//! M10's future API surface is not added by this build-time copy.
use std::{fs, path::Path};

/// Generates declarations without modifying the optimized distribution bytes.
pub(crate) fn emit(wasm: &Path) -> Result<(), String> {
    // Trunk and this CLI use the pinned wasm-bindgen version from the workspace.
    crate::commands::quiet(
        "wasm-bindgen",
        &[
            "target/wasm32-unknown-unknown/release/puzzle_web.wasm",
            "--target",
            "web",
            "--out-dir",
            "target/web-types",
            "--out-name",
            "puzzle-terminal",
        ],
    )?;
    let source =
        fs::read_to_string("target/web-types/puzzle-terminal.d.ts").map_err(|e| e.to_string())?;
    // Missing exports must fail the build rather than leave a stale hand-maintained class.
    for name in [
        "create(",
        "write(",
        "writeln(",
        "sendKey(",
        "onData(",
        "attachTransport(",
        "connect(",
        "exited(",
        "TerminalEvents",
    ] {
        if !source.contains(name) {
            return Err(format!("browser declaration missing {name}"));
        }
    }
    // wasm-bindgen's background suffix is not part of the public JS module name.
    let stem = wasm
        .file_stem()
        .and_then(|n| n.to_str())
        .ok_or("invalid wasm filename")?;
    let stem = stem
        .strip_suffix("_bg")
        .ok_or("unexpected Trunk wasm filename")?;
    // Keep the declaration adjacent to the matching JS so module resolution finds it.
    fs::write(wasm.with_file_name(format!("{stem}.d.ts")), source).map_err(|e| e.to_string())
}
