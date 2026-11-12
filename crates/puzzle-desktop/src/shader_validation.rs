//! Native WGSL conformance complements the browser's Tint validation.
//!
//! This test-only platform module accesses wgpu's existing Naga re-export;
//! neutral crates and the WebGPU-only wasm build gain no dependency.
/// Full validation of every WGSL source, including the composed oracle fragment.
mod tests;
