//! Resolved wasm dependency guard for WebGPU-only shader compilation.
//!
//! Wgpu 30 gates the safe `ShaderSource::Wgsl` constructor behind its
//! `wgsl` feature. That feature uses weak `wgpu-core?/wgsl`, so WebGPU-only
//! wasm should expose source creation without compiling native wgpu-core or
//! Naga. Cargo's resolved target tree is the evidence, since manifest text
//! alone cannot reveal feature unification from another workspace crate.
//!
//! This runs on every `cargo xtask lint`, so later dependency additions
//! cannot silently restore a translation backend to the browser package.
use std::process::Command;

/// Rejects native shader/backends or WebGL in the resolved browser tree.
pub(super) fn check() -> Result<(), String> {
    // The package-target pair matches the release Trunk entry point rather
    // than all workspace packages, which include native Metal tooling.
    let output = Command::new("cargo")
        .args([
            "tree",
            "-p",
            "puzzle-web",
            "--target",
            "wasm32-unknown-unknown",
            "-e",
            "features",
            "--prefix",
            "none",
        ])
        .output()
        .map_err(|error| format!("cargo tree for wasm GPU invariant: {error}"))?;
    // A failed resolution cannot establish the invariant; preserve Cargo's
    // own explanation instead of treating absent packages as success.
    if !output.status.success() {
        return Err(format!(
            "wasm GPU dependency tree failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let tree = String::from_utf8(output.stdout)
        .map_err(|error| format!("wasm GPU dependency tree is not UTF-8: {error}"))?;
    // A missing required feature could give a small tree for the wrong
    // reason: the browser would no longer create or compile this shader.
    for required in ["wgpu feature \"webgpu\"", "wgpu feature \"wgsl\""] {
        if !tree.lines().any(|line| line == required) {
            return Err(format!("wasm GPU dependency tree lacks {required}"));
        }
    }
    // Package names include ` v` before their version, distinguishing
    // `naga` from the permitted lightweight `naga-types` package.
    let forbidden = [
        "wgpu-core v",
        "naga v",
        "png v",
        "fontique v",
        "wgpu feature \"gles\"",
        "wgpu feature \"metal\"",
        "wgpu feature \"vulkan\"",
        "wgpu feature \"dx12\"",
    ];
    for line in tree.lines() {
        // WebGL can arrive through another dependency's feature name too,
        // so scan all resolved lines rather than only wgpu's own features.
        if forbidden.iter().any(|name| line.contains(name))
            || line.to_ascii_lowercase().contains("webgl")
        {
            return Err(format!("forbidden wasm GPU dependency: {line}"));
        }
    }
    Ok(())
}
