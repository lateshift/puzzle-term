//! Cold Slug coverage stays GPU-owned until its atlas copies are acknowledged.
//!
//! The production shader targets R8 directly, preserving its coverage math and
//! quantization without a readback or CPU mirror. A bounded scratch batch becomes
//! an opaque ticket; the neutral frame specifies each crop's final atlas slot.
//! Queue submissions order rasterization, crop copies and capacity page captures.
//! Tickets retain source textures across skipped frames and partial upload retries.
//! Their registry belongs to one device, never a global. Safe shared ownership is
//! required because the native producer and optional diagnostic executor borrow
//! the same device; its lock is crossed only for cold batch registration/transfer.
//! Warm glyphs never touch it. A successful primary upload releases tickets after
//! the optional verifier has observed them. Device loss drops this whole registry.
//! R8 staging is temporary upload ownership, not a second permanent glyph cache.
//! Neutral uploads carry a shared lifetime marker, which contains no GPU object.
//! The registry holds it weakly: dropping a failed build or obsolete text context
//! makes that batch reclaimable before the next admission. A skipped acquisition
//! retains the marker with its pending uploads and keeps the exact source alive.
//! Successful upload acknowledgement removes registry ownership immediately.
use crate::{RenderError, Renderer, SlugPipeline, SlugTextures};
use puzzle_frame::slug::SlugInstance;
use puzzle_slug::SlugAtlas;

/// One lazy device-bound producer is shared by a text context's cold glyphs.
pub struct MaskBaker {
    /// Safe wgpu handles share the render owner's original device and queue.
    renderer: Renderer,
    /// Scratch curves/bands never affect the ordinary renderer's vector addresses.
    textures: SlugTextures,
    /// Shader compilation and instance storage are retained across cold batches.
    pipeline: SlugPipeline,
}

/// Retained pipeline state amortizes cold batch work across the text context.
impl MaskBaker {
    /// Binds an isolated scratch store to the host's existing GPU device.
    pub fn new(renderer: &Renderer) -> Self {
        let textures = SlugTextures::new(&renderer.device);
        // Isolated views keep temporary grid packing out of the ordinary vector atlas.
        let (curves, bands) = textures.views();
        let pipeline = SlugPipeline::new(
            &renderer.device,
            wgpu::TextureFormat::R8Unorm, // One channel stores the unchanged coverage value.
            curves,
            bands,
        );
        Self {
            renderer: renderer.clone(),
            textures,
            pipeline,
        }
    }
    /// Produces GPU-owned coverage through unchanged Slug coverage math.
    pub fn bake(
        &mut self,
        atlas: &mut SlugAtlas,
        draws: &[SlugInstance],
        size: [u32; 2],
    ) -> Result<(u64, std::sync::Arc<()>), RenderError> {
        if size[0] == 0 || size[1] == 0 || size[0] > 2048 || size[1] > 2048 {
            // Scratch sheets cannot exceed the fixed mask page or have an empty attachment.
            return Err(RenderError::Readback("invalid grid mask sheet".into()));
        }
        // sync_full drains the scratch's dirty spans; only new packed ink is transferred.
        self.textures
            .sync_full(&self.renderer.device, &self.renderer.queue, atlas)?;
        // The scratch budget is one page pair; these views never grow or relocate.
        let (curves, bands) = self.textures.views();
        self.pipeline.rebind(&self.renderer.device, curves, bands);
        self.pipeline
            .set_viewport(&self.renderer.queue, size[0], size[1]);
        // Upload one contiguous instance batch, sharing the analytic coverage shader.
        self.pipeline
            .upload(&self.renderer.device, &self.renderer.queue, draws);
        let target = self
            .renderer
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("GPU grid coverage staging"),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1, // Staging is a 2D sheet, not sampled atlas storage.
                },
                mip_level_count: 1, // Grid sampling never needs a mip pyramid.
                sample_count: 1,    // Analytic coverage replaces multisample anti-aliasing.
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[], // Coverage remains unorm; it is never interpreted as sRGB.
            });
        // The full batch is complete before its coverage leaves GPU ownership.
        self.renderer
            .draw_slug_target(&target, &self.pipeline, draws.len() as u32);
        let mut tickets = self
            .renderer
            .masks
            .lock()
            .map_err(|_| RenderError::Upload("GPU mask ticket owner poisoned".into()))?;
        // Different shared contexts must not resolve another batch's staging image.
        tickets.next = tickets.next.wrapping_add(1);
        let ticket = tickets.next;
        // Discard abandoned batches before admitting another: a dropped frame owns no lease.
        tickets
            .images
            .retain(|_, (lease, _)| lease.strong_count() > 0);
        let lease = std::sync::Arc::new(());
        tickets
            .images
            .insert(ticket, (std::sync::Arc::downgrade(&lease), target));
        Ok((ticket, lease))
    }
}

/// Device-owned pending batch textures; none exist once uploads are acknowledged.
#[derive(Default)]
pub(crate) struct Tickets {
    /// Monotonic identity distinguishes retries and different shared text contexts.
    next: u64,
    /// Source textures are retained only while a neutral upload references them.
    images: std::collections::HashMap<u64, (std::sync::Weak<()>, wgpu::Texture)>,
}

/// Ticket lookup and acknowledgement are serialized only during cold resource transfer.
impl Renderer {
    /// Resolves a neutral ticket without exposing GPU resources to layout.
    pub(crate) fn mask_ticket(&self, ticket: u64) -> Result<wgpu::Texture, RenderError> {
        let tickets = self
            .masks
            .lock()
            .map_err(|_| RenderError::Upload("GPU mask registry poisoned".into()))?;
        // An expired marker cannot be used to resurrect a frame whose uploads were dropped.
        tickets
            .images
            .get(&ticket)
            .filter(|(lease, _)| lease.strong_count() > 0)
            .map(|(_, texture)| texture.clone())
            .ok_or_else(|| RenderError::Upload("missing GPU mask ticket".into()))
    }
    /// Only the primary successful upload writer acknowledges these temporary sources.
    /// A diagnostic executor observes the same batch first and leaves its ownership intact.
    pub(crate) fn acknowledge_masks(
        &self,
        uploads: &[puzzle_frame::Upload],
    ) -> Result<(), RenderError> {
        if !uploads
            .iter()
            .any(|u| matches!(u, puzzle_frame::Upload::GpuMask { .. }))
        {
            return Ok(());
        }
        let mut tickets = self
            .masks
            .lock()
            .map_err(|_| RenderError::Upload("GPU mask registry poisoned".into()))?;
        for upload in uploads {
            if let puzzle_frame::Upload::GpuMask { ticket, .. } = upload {
                // Submitted commands retain safe GPU handles after registry ownership ends.
                tickets.images.remove(ticket);
            }
        }
        Ok(())
    }
}
