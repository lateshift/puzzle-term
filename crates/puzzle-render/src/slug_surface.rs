//! Presentable Slug draw path for the M3 browser comparison (§10.1).
//!
//! The browser shell creates and configures its canvas surface, then hands
//! that surface and the same scene data used by the native oracle suite to
//! this renderer. Band and curve uploads happen before the draw submission.
//! A white shape over black matches the offscreen scene's color contract,
//! so browser reviewers inspect exactly the same outline and transform.
use crate::{RenderError, Renderer, SlugPipeline, SlugTextures};
use puzzle_frame::slug::SlugInstance;
use puzzle_slug::SlugAtlas;

/// Slug drawing methods that do not own a platform surface.
impl Renderer {
    /// Uploads one scene, draws it into the configured surface and presents.
    pub fn render_slug_surface(
        &self,
        surface: &wgpu::Surface<'_>,
        atlas: &mut SlugAtlas,
        instances: &[SlugInstance],
        width: u32,
        height: u32,
    ) -> Result<(), RenderError> {
        // The atlas is authoritative; its queued dirty spans must reach the
        // GPU before the instance's glyph location is sampled by WGSL.
        // A fresh texture set makes this standalone review draw independent
        // of any prior scene's upload order or atlas generation.
        let mut textures = SlugTextures::new(&self.device);
        textures.sync_full(&self.device, &self.queue, atlas)?;
        self.draw_slug_surface(surface, &textures, instances, width, height)
    }

    /// Presents a prebuilt font pack without parsing a TTF or preparing curves.
    pub fn render_packed_slug_surface(
        &self,
        surface: &wgpu::Surface<'_>,
        curves: &[[f32; 4]],
        bands: &[[u16; 2]],
        instances: &[SlugInstance],
        width: u32,
        height: u32,
    ) -> Result<(), RenderError> {
        // The pack decoder checks its addresses and dimensions before the
        // platform passes these ready-to-upload planes to the GPU.
        // Uploading the whole planes is intentional here: their glyph
        // handles refer to physical texture rows, including row padding.
        let mut textures = SlugTextures::new(&self.device);
        textures.sync_pack(&self.device, &self.queue, curves, bands)?;
        self.draw_slug_surface(surface, &textures, instances, width, height)
    }

    /// Shares the exact browser pass between runtime outlines and font packs.
    fn draw_slug_surface(
        &self,
        surface: &wgpu::Surface<'_>,
        textures: &SlugTextures,
        instances: &[SlugInstance],
        width: u32,
        height: u32,
    ) -> Result<(), RenderError> {
        // Both entry points converge after upload, so WebGPU sees the
        // same bind layout, shader and blend state for packed and live ink.
        let (curves, bands) = textures.views();
        let output = acquire(surface)?;
        // The surface's format may differ between Metal and browser WebGPU;
        // pipeline creation uses the actual acquired view format.
        let mut pipeline = SlugPipeline::new(&self.device, output.texture.format(), curves, bands);
        // The projection maps the scene's device-pixel coordinates to clip
        // space and supplies the pixel dimensions used by Slug dilation.
        pipeline.set_viewport(&self.queue, width, height);
        // One instance buffer write serves the whole batch. Atlas texture
        // coordinates stay fixed while instances vary by transform/color.
        pipeline.upload(&self.device, &self.queue, instances);
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        // A render pass keeps the draw and its clear in one submission. The
        // clear makes the browser image comparable to the native black target.
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Slug browser scene"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Slug surface draw"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    // An opaque black clear exposes premultiplied white
                    // coverage just like the native offscreen oracle target.
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            // The shader evaluates analytic coverage per fragment; no MSAA
            // or filtering is needed for the packed curve data.
            pipeline.draw(&mut pass, instances.len() as u32);
        }
        // Presentation follows submission on the same queue, so the ready
        // canvas state can be set only after this method returns success.
        self.queue.submit(Some(encoder.finish()));
        self.queue.present(output);
        Ok(())
    }
}

/// Converts every surface acquisition status into a shell-visible result.
fn acquire(surface: &wgpu::Surface<'_>) -> Result<wgpu::SurfaceTexture, RenderError> {
    // A resize or occluded tab invalidates its previous texture. Reporting
    // the actual status lets the platform reconfigure instead of presenting
    // stale ink from another viewport size.
    match surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(output)
        | wgpu::CurrentSurfaceTexture::Suboptimal(output) => Ok(output),
        wgpu::CurrentSurfaceTexture::Timeout => Err(RenderError::Surface("timeout")),
        wgpu::CurrentSurfaceTexture::Occluded => Err(RenderError::Surface("occluded")),
        wgpu::CurrentSurfaceTexture::Outdated => Err(RenderError::Surface("outdated")),
        wgpu::CurrentSurfaceTexture::Lost => Err(RenderError::Surface("lost")),
        wgpu::CurrentSurfaceTexture::Validation => Err(RenderError::Surface("validation")),
    }
}
