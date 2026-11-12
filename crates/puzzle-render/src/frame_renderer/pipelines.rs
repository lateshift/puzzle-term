//! Immutable shaders are shared; sampled textures and instance buffers are context data.
//!
//! Explicit binding layouts describe the same ABI for every context. Changing
//! attachment format replaces shader handles only, retaining all uploaded ink.
//!
//! Glyph resources follow a shared text context; image resources instead follow
//! the individual surface. The host transfers that cache before lending context
//! state to another window, preventing sibling frames from evicting each other.
//! Device replacement is the only transfer that invalidates sampled image ink.
use super::*;
/// Three compiled pipelines for one device and attachment format (D-30).
#[derive(Clone)]
pub struct FramePipelines {
    /// Shader output format must match the acquired native or offscreen target.
    pub(super) format: wgpu::TextureFormat,
    /// Rect shader has no dependency on text atlas storage.
    pub(super) rect: wgpu::RenderPipeline,
    /// Every context binds its own curves, bands and paint table to this shader.
    pub(super) slug: wgpu::RenderPipeline,
    /// Mask/color arrays remain with the context rather than these shader handles.
    pub(super) raster: wgpu::RenderPipeline,
}
impl FramePipelines {
    /// Compiles once for the process device/format, without allocating atlas pages.
    #[must_use]
    pub fn new(renderer: &Renderer, format: wgpu::TextureFormat) -> Self {
        // Compilation uses explicit binding layouts, identical across context instances.
        let device = renderer.device();
        Self {
            format,
            rect: RectPipeline::compile_shared(device, format),
            slug: SlugPipeline::compile_shared(device, format),
            raster: RasterPipeline::compile_shared(device, format),
        }
    }
    /// Selects attachment-compatible shader state without coupling it to a text key.
    #[must_use]
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }
}
impl FrameRenderer {
    /// Changes only immutable format-bound shaders, preserving all context atlas uploads.
    pub fn set_pipelines(&mut self, pipelines: &FramePipelines) {
        // Formats affect color attachments only; sampled textures retain their identity.
        self.format = pipelines.format;
        self.rect.pipeline = pipelines.rect.clone();
        self.slug.pipeline = pipelines.slug.clone();
        self.raster.pipeline = pipelines.raster.clone();
    }
}

impl FrameRenderer {
    /// Transfers per-surface image residency across shared font-context loans.
    pub fn take_image_cache(&mut self) -> Option<Box<crate::ImageCache>> {
        // Taking ownership also prevents a sibling from sampling this surface's images.
        self.images.take()
    }
    /// Installs the surface's cache before it draws with the next text context.
    pub fn set_image_cache(&mut self, cache: Option<Box<crate::ImageCache>>) {
        // Reattachment happens before prepare, which validates the device identity.
        self.images = cache;
    }
    /// Lazy images cost no allocation on a terminal that has never shown one.
    pub(super) fn prepare_images(
        &mut self,
        renderer: &Renderer,
        frame: &RenderFrame,
        target: [u32; 2],
    ) -> Result<(), RenderError> {
        // Never-visible images leave both CPU bookkeeping and GPU bindings absent.
        if frame.images.is_none() && self.images.is_none() {
            return Ok(());
        }
        // A lost device cannot use old texture handles; source pixels remain sealed.
        if self
            .images
            .as_ref()
            .is_some_and(|cache| !cache.is_device(renderer))
        {
            self.images = None;
        }
        // Recovery recreates only this surface's bindings; immutable pixels survive.
        let images = self
            .images
            .get_or_insert_with(|| Box::new(crate::ImageCache::new(renderer)));
        images.prepare(renderer, frame, target)
    }
}
