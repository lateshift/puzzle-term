//! Device, surface, and clear pass for the first milestone (design §10.1).
//!
//! The platform creates the instance and surface; the renderer requests a device
//! from that instance's adapter and uses one background color on both targets.
//!
//! M0 has no terminal cells, pipelines, or shaders. A render pass with a clear
//! load operation gives the two shells a real GPU path without anticipating
//! the later frame format. The native harness uses the same clear operation on
//! an offscreen texture, then checks its pixels before writing a screenshot.
//!
//! Surface acquisition is separate from device creation: a surface can become
//! outdated after resize while the device remains valid. The platform shell
//! decides when to reconfigure because it owns the window or canvas size.
//! Keeping the adapter here lets configuration query the format compatible
//! with that exact device instead of assuming a format across browsers.
use thiserror::Error;
/// The M0 dark-theme background, expressed as encoded sRGB render target components.
///
/// These values produce the `#0e121a` smoke pixel in an RGBA8 unorm texture.
/// The full theme resolver replaces this constant in M6 (design §14.6).
pub const CLEAR: wgpu::Color = wgpu::Color {
    r: 0.055,
    g: 0.071,
    b: 0.102,
    a: 1.0,
};
/// Failures that prevent a GPU frame from being produced.
///
/// Callers decide whether a transient acquisition failure needs a resize
/// or whether to show a persistent initialization error to the user.
#[derive(Debug, Error)]
pub enum RenderError {
    /// The platform's surface has no usable adapter.
    #[error("no WebGPU adapter is available")]
    NoAdapter,
    /// Requesting the GPU device failed.
    #[error("requesting a WebGPU device failed: {0}")]
    Device(#[from] wgpu::RequestDeviceError),
    /// A presentable frame could not be acquired.
    #[error("acquiring the surface failed: {0}")]
    Surface(&'static str),
    /// The surface cannot be configured for this adapter.
    #[error("the surface has no default configuration")]
    NoSurfaceConfiguration,
    /// Reading an offscreen image failed.
    #[error("GPU readback failed: {0}")]
    Readback(String),
    /// A CPU atlas span could not be transferred into its GPU plane.
    #[error("Slug texture upload failed: {0}")]
    Upload(String),
}
/// The GPU device and submission queue shared by the window and offscreen paths.
///
/// A device records work; its queue submits and presents that work. They stay
/// together so a texture from one device is never submitted to another queue.
#[derive(Clone)]
pub struct Renderer {
    /// Records rendering commands for the selected adapter.
    pub(crate) device: wgpu::Device,
    /// Submits commands to the selected adapter.
    pub(crate) queue: wgpu::Queue,
    /// Keeps the adapter for surface capability queries.
    pub(crate) adapter: wgpu::Adapter,
    /// Opt-in real-drawable evidence; ordinary surfaces remain framebuffer-only.
    pub(crate) surface_readback: bool,
    /// Cold GPU coverage tickets bridge the producer and executor on the same device.
    pub(crate) masks: std::sync::Arc<std::sync::Mutex<crate::mask_baker::Tickets>>,
}
/// Manages device setup and the two M0 clear targets.
impl Renderer {
    /// Borrows the shared device for platform lifecycle notifications.
    /// The platform may observe device loss without recreating another queue.
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// Borrows the submission queue for platform-native completion notifications.
    /// The native wrapper must preserve queue order and never submit another device's resources.
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Presents an acquired surface image after the frame pass was submitted.
    ///
    /// The desktop host times acquisition itself for D-33, while this owner
    /// keeps the device queue private and tied to the acquired texture.
    pub fn present_surface_texture(&self, output: wgpu::SurfaceTexture) {
        self.queue.present(output);
    }
    /// Requests the low-power adapter and default-feature device for a platform surface.
    ///
    /// The optional surface is present for a window or canvas and absent for
    /// the headless harness. Adapter compatibility is checked during selection
    /// because some native machines have multiple GPUs with different outputs.
    pub async fn new(
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
    ) -> Result<Self, RenderError> {
        // The surface preference avoids selecting a GPU that cannot present to it.
        // A terminal should not wake a discrete GPU merely to clear a canvas.
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: surface,
                force_fallback_adapter: false,
                // Bucketed limits can obscure the adapter's actual capabilities.
                apply_limit_buckets: false,
            })
            .await
            .map_err(|_| RenderError::NoAdapter)?;
        // M0 needs no optional GPU features; default limits also work in browsers.
        // Future pipelines must explicitly request any feature they require.
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await?;
        tracing::debug!("GPU device ready for clear passes");
        Ok(Self {
            device,
            queue,
            adapter,
            surface_readback: false,
            masks: Default::default(),
        })
    }
    /// Recreates a lost device on this surface-compatible adapter.
    /// CPU atlas mirrors remain owned by the host and are re-uploaded afterward.
    pub async fn recreate(&self) -> Result<Self, RenderError> {
        let (device, queue) = self
            .adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await?;
        Ok(Self {
            device,
            queue,
            adapter: self.adapter.clone(),
            surface_readback: self.surface_readback,
            masks: Default::default(),
        })
    }

    /// Returns the configured queue latency for surface setup and host calibration.
    ///
    /// Metal reserves one more drawable than this value. The host must use the
    /// same value when filling the idle FIFO to learn a refresh phase (§10.4).
    pub fn maximum_frame_latency(&self) -> u32 {
        // D-35: one frame of FIFO slack removed all missed refreshes in the
        // five-run 60/120-Hz comparison, for +1–1.5 ms median input latency.
        // Calibration uses this same depth; WebGPU's browser owns canvas pacing.
        2
    }

    /// Configures a presentable surface after creation or a nonzero resize.
    ///
    /// Device pixels are supplied by the platform; CSS pixels are not enough
    /// when browser zoom or a Retina scale factor changes the backing store.
    pub fn configure(
        &self,
        surface: &wgpu::Surface<'_>,
        width: u32,
        height: u32,
    ) -> Result<(), RenderError> {
        // Configuration and pipeline selection share one encoded-colour decision.
        let config = self.surface_configuration(surface, width, height)?;
        surface.configure(&self.device, &config);
        Ok(())
    }

    /// Returns the same attachment format used by configure, never wgpu's sRGB default.
    pub fn surface_format(
        &self,
        surface: &wgpu::Surface<'_>,
        width: u32,
        height: u32,
    ) -> Result<wgpu::TextureFormat, RenderError> {
        let config = self.surface_configuration(surface, width, height)?;
        // Called at creation/recovery, not on each resize or presented frame (R10).
        tracing::info!(event="render.surface_colour", format=?config.format,
            color_space=?config.color_space, "D-27 encoded sRGB attachment and compositor");
        Ok(config.format)
    }
    /// Clears one surface frame to the M0 theme background and presents it.
    ///
    /// Occlusion means no image can be shown, so the caller can wait for the
    /// next redraw. Lost and outdated surfaces require platform reconfiguration.
    pub fn clear_surface(&self, surface: &wgpu::Surface<'_>) -> Result<(), RenderError> {
        let output = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(output)
            | wgpu::CurrentSurfaceTexture::Suboptimal(output) => output,
            wgpu::CurrentSurfaceTexture::Timeout => return Err(RenderError::Surface("timeout")),
            // A hidden window has nothing to paint until the OS exposes it again.
            wgpu::CurrentSurfaceTexture::Occluded => return Ok(()),
            wgpu::CurrentSurfaceTexture::Outdated => return Err(RenderError::Surface("outdated")),
            wgpu::CurrentSurfaceTexture::Lost => return Err(RenderError::Surface("lost")),
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err(RenderError::Surface("validation"));
            }
        };
        self.clear_view(
            &output
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default()),
        );
        self.queue.present(output);
        // Present consumes the acquired texture, preventing stale reuse.
        Ok(())
    }
    /// Records a single render pass whose load operation fills every pixel.
    ///
    /// The pass intentionally contains no draw calls: the attachment load
    /// operation itself replaces the prior contents with the background color.
    pub(crate) fn clear_view(&self, view: &wgpu::TextureView) {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("clear encoder"),
            });
        {
            // A clear load operation requires no shader or draw call in M0.
            // Dropping the pass closes its command scope before encoder.finish().
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("background clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(CLEAR),
                        // Store preserves the cleared pixels for presentation or readback.
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        self.queue.submit(Some(encoder.finish()));
    }
}
