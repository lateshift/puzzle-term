//! Desktop adapts neutral Slug mask requests to the existing GPU device (D-47).
//!
//! The text crate owns batching and crop geometry; generated masks stay GPU-only.
//! This owner translates the tested GPU ABI and retains one lazily compiled
//! producer. It introduces neither a glyph worker nor a second device/queue.
//! There is no native font rasterizer here. The selected hinted or unhinted
//! outline has already been reduced and banded by the platform-neutral text path.
//! Only white coverage is baked; the final raster shader supplies terminal tint.
//! A transformed outline is rejected by text policy before this adapter is called.
//! The instance vector retains its high-water capacity across bounded cold batches.
//! Warm frames do not call bake; cold frames submit GPU coverage without readback.
//! A GPU reset replaces this adapter and re-creates grid masks from font sources.
//! Shader compilation is lazy so a Slug-only setting has no extra startup pipeline.
//! The host's validated font.renderer setting controls future grid preparations.
use puzzle_render::{MaskBaker, Renderer};
use puzzle_term::frame::slug::SlugInstance;
use puzzle_term::{
    frame_builder::context::TextContext,
    text::raster::{GridRasterizer, MaskDraw},
};

/// Renderer handles are safe shared wgpu ownership, touched only on the render thread.
struct Gpu {
    /// Same device/queue as this surface's frame executor.
    renderer: Renderer,
    /// Compiles the Slug coverage pipeline only after a glyph actually chooses it.
    baker: Option<MaskBaker>,
    /// ABI conversion scratch is cleared and reused for subsequent cold batches.
    instances: Vec<SlugInstance>,
}

/// Pure translation keeps glyph policy in puzzle-text and GPU execution in puzzle-render.
impl GridRasterizer for Gpu {
    /// Returns production-shader coverage for a complete bounded batch.
    fn bake(
        &mut self,
        atlas: &mut puzzle_term::slug::SlugAtlas,
        draws: &[MaskDraw],
        size: [u32; 2],
    ) -> Result<(u64, std::sync::Arc<()>), String> {
        self.instances.clear();
        for draw in draws {
            // Opaque white over black makes red equal coverage; tint is applied at final draw.
            self.instances.push(SlugInstance {
                m: draw.matrix,
                t: draw.translation,
                bbox: draw.shape.bbox,
                glyph_loc: puzzle_term::slug::encoding::encode_glyph_loc(draw.shape.glyph_loc),
                band_info: draw.shape.band_info,
                color: 0xffffffff,
                paint: 0,
            });
        }
        self.baker
            .get_or_insert_with(|| MaskBaker::new(&self.renderer))
            .bake(atlas, &self.instances, size)
            .map_err(|e| e.to_string())
    }
}

/// Enables desktop byte selection; schema-validated slug overrides remain explicit.
pub fn install(context: &mut TextContext, renderer: &Renderer, choice: &str) -> Result<(), String> {
    use puzzle_term::text::glyph::RendererChoice;
    let choice = match choice {
        "slug" => RendererChoice::Slug,
        "atlas" => RendererChoice::Atlas,
        _ => RendererChoice::Auto,
    };
    // Shared core/web contexts are never implicitly switched by this native adapter.
    context.set_grid_renderer(choice);
    context.set_grid_rasterizer(Box::new(producer(renderer)))
}

/// Creates the same lazy coverage producer for a context or an isolated raster-cost probe.
pub fn producer(renderer: &Renderer) -> impl GridRasterizer + use<> {
    // GPU handles share the caller's device/queue; no additional adapter is requested.
    Gpu {
        renderer: renderer.clone(),
        baker: None,
        instances: Vec::new(),
    }
}

#[cfg(test)]
/// Independent GPU comparisons cover cache selection and retained restoration ink.
mod tests;
