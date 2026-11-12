//! Every shipped WGSL file must pass Naga's full native validation.
//!
//! wgpu re-exports the existing native Naga dependency, so this adds no crate
//! and cannot put a shader compiler in the WebGPU-only wasm tree (R9).
//! Naga accepts the original branched raster textureSample with every flag
//! enabled. This is a parsing/general-validation guard; only the browser
//! witness guards that derivative-uniformity defect in flat fragment inputs.
#![allow(
    clippy::unwrap_used,
    reason = "shader fixtures and directory discovery must fail the test on invalid input"
)]
use wgpu::naga;

/// Discover shaders rather than maintaining a list that could omit a new file.
#[test]
fn every_shader_validates_with_uniformity() {
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../puzzle-render/src/shaders");
    let mut count = 0;
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|extension| extension != "wgsl") {
            continue;
        }
        let mut source = std::fs::read_to_string(&path).unwrap();
        // The diagnostic fragment is an extension of Slug's vertex types,
        // concatenated with that module by the native oracle pipeline.
        let name = path.file_name().unwrap().to_str().unwrap();
        if matches!(name, "slug.wgsl" | "slug_debug.wgsl" | "paint.wgsl") {
            let base = format!(
                "{}\n{}",
                include_str!("../../../puzzle-render/src/shaders/slug.wgsl"),
                include_str!("../../../puzzle-render/src/shaders/paint.wgsl")
            );
            source = if name == "slug_debug.wgsl" {
                format!("{base}\n{source}")
            } else {
                base
            };
        }
        let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|error| {
            panic!("{}: {}", path.display(), error.emit_to_string(&source))
        });
        // Explicitly include uniformity, not just expression/layout validation.
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        count += 1;
    }
    assert!(
        count >= 3,
        "the rect, Slug and raster shaders must be checked"
    );
}
