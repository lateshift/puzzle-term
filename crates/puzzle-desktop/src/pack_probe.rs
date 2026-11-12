//! Native M4 proof of prebuilt-pack upload without a source font file.
//!
//! This command opens only `assets/fontpacks/jetbrains-regular.pzfp`.
//! Its decoded cmap, metrics and ready-to-upload texels produce a short
//! GPU screenshot through the same Slug pipeline as runtime outlines.
//! A nonzero RGB readback is a mechanical first-frame ink assertion.
//!
//! The test is intentionally stricter than merely parsing a pack: it
//! uploads its curve and band bytes to Metal and presents recognizable
//! ASCII through the production shader. No `assets/fonts/*.ttf` path is
//! opened in this module, which proves that a font file is unnecessary
//! for the first browser frame's equivalent upload path.
use crate::{block_on::block_on, png};
use puzzle_render::Renderer;
use puzzle_term::{pack_scene::PackScene, text_scene::FontPack};
use std::fs;

/// Reads one pack, draws its ASCII glyphs and writes reviewable evidence.
pub(crate) fn run() -> Result<(), String> {
    // A failed file read names the pack, rather than falling back to a
    // full font and accidentally passing the no-font acceptance check.
    let bytes = fs::read("assets/fontpacks/jetbrains-regular.pzfp")
        .map_err(|error| format!("font pack: {error}"))?;
    let pack = FontPack::decode(&bytes)?;
    let scene = PackScene::new(pack, "PACK B", 14.0, 1.0)?;
    // The native shell creates the GPU instance; puzzle-render receives
    // only validated texels and ordinary packed draw records.
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let renderer = block_on(Renderer::new(&instance, None)).map_err(|error| error.to_string())?;
    let image = renderer
        .render_packed_slug_to_image(
            &scene.pack.curves,
            &scene.pack.bands,
            &scene.instances,
            scene.width,
            scene.height,
        )
        .map_err(|error| error.to_string())?;
    // Black is the clear color, so any nonzero RGB sample came from a
    // real shader draw after the pack texture upload completed.
    let ink = image
        .pixels
        .chunks_exact(4)
        .filter(|pixel| pixel[..3].iter().any(|v| *v != 0))
        .count();
    if ink < 20 {
        return Err(format!("font pack produced only {ink} ink pixels"));
    }
    fs::create_dir_all("target/fontpacks").map_err(|error| error.to_string())?;
    png::write(
        std::path::Path::new("target/fontpacks/pack-probe.png"),
        &image,
    )?;
    println!("font pack GPU probe: {ink} ink pixels");
    Ok(())
}
