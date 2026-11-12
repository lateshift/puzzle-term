//! Render-thread surface, snapshot and text resources (§4.4, D-41, D-43).
//!
//! The PTY drain and parser are separate blocking workers. The process render
//! thread owns snapshot timing and GPU submission while the parser owns VT byte
//! slices by exclusive Session ownership (D-50). The web shell keeps its single
//! thread and still uses the same neutral Session and frame builder.
//!
//! A PTY master exposes separate read and write handles. The reader thread
//! owns its clone and transfers fixed-pool chunks through MPSC channels.
//! Session owns the writer behind Transport; a keyboard command is encoded
//! and written synchronously by `pump`, independently of render scheduling.
//! The child lifetime and keyboard adapter stay with the native input owner.
//! This render owner cannot encode a key or invoke toolkit operations.
//!
//! Physical window pixels and terminal grid dimensions have different
//! lifetimes during dragging. The GPU surface follows the OS immediately;
//! the grid uses the latest size before each frame. The frame builder
//! knows the current backing extent so clip coordinates remain physical.
//! DPI changes trigger glyph metric recomputation before the next grid
//! calculation, preserving one cell origin between VT and Slug text.
//! The fixed terminal command set in M6.4 is deliberately smaller than
//! the configurable shortcuts of M7. This layer only maps OS identities;
//! Session owns protocol encoding and reads active VT modes immediately
//! before writing to the PTY. Focus changes alter cursor geometry without
//! editing a row. OSC title bytes have already been validated and bounded
//! by puzzle-vt before a notification posts them to the native title setter.
//!
//! Optional recording distinguishes the three moments that matter for a
//! reported stale frame: a PTY read, a parsed VT update and the snapshot
//! frozen for GPU work. The parser can continue while Metal waits for a
//! Fifo presentation slot. Therefore the `frame` entry stores the parsed
//! count from the snapshot, not the current atomic count at presentation.
//! This preserves the old frame's identity even when a newer erase sequence
//! has already reached the parser before the swapchain call returns.
//!
//! The M6.4i platform defaults are resolved against physical key identity
//! before terminal encoding. A paste gesture becomes a typed Paste command
//! containing clipboard text, so the Session applies its bracketed-paste
//! and control-character policy. The source RawInput and resulting Command
//! remain distinct in private recordings. A copy binding is inactive until
//! M8 supplies selection, allowing Ctrl+C to keep its interrupt meaning.
/// Output/reply progress outlives a single consumed pacer reservation.
mod demand;
/// Actor-owned contexts move into and out of a surface for each turn.
pub(crate) mod loan;
use crate::render_thread::{host::HostView, surface::Extent as PhysicalSize};
use crate::{
    recording::Recorder,
    session_thread::{Frames, Handle},
    settings::DesktopSettings,
};
use puzzle_render::{FrameRenderer, Renderer};
use puzzle_term::frame_builder::context::TextContext;
use puzzle_term::{FontMetrics, SessionMessage, frame_builder::FrameBuilder};
use puzzle_term::{frame::RenderFrame, frame_snapshot::FrameSnapshot};
use puzzle_term::{platform::PtySize, text::metrics::CellMetrics};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
/// Bounded idle FIFO calibration stops at its first real refresh sample.
#[cfg(any(not(target_os = "macos"), test))]
mod calibration;
/// Raw scheduling, snapshot and surface gates explain missing native presentations.
#[cfg(feature = "lock-metrics")]
mod frame_metrics;
/// Per-appearance ink latency is recorded only in explicitly instrumented runs.
#[cfg(feature = "lock-metrics")]
mod ink_latency;
/// Session snapshot, GPU submission and display timing for one frame.
pub(crate) mod presentation;
/// Bounded PTY drain and child exit happen independently of surface ownership.
mod reader;
/// PTY startup uses this drain implementation without owning the rendering driver.
pub use reader::{Drain, start_reader};
/// Lost devices recreate GPU objects from retained CPU atlas mirrors.
pub mod recovery;
/// Applies changed settings after focus or the reload shortcut.
mod settings;
/// Debug-only cold GPU comparison of the exact captured VT state.
mod verify;
/// OS visibility witnesses and eligibility for idle phase learning.
mod visibility;
use verify::{FrameVerifier, VerifyRequest, VerifySchedule};

/// Paired GPU objects created from the same adapter and surface format.
pub struct LiveGpu {
    /// Window creation precedes adapter/font work; startup traces need that origin.
    pub window_created: Instant,
    /// Successful initial surface configuration, before the first OS callbacks.
    pub surface_configured: Instant,
    /// Device and queue used by the live window.
    pub renderer: Renderer,
    /// Error recovery is installed before startup GPU resource creation.
    pub gpu_errors: puzzle_render::ErrorMonitor,
    /// Retained pipelines and atlas textures for that device.
    pub frame_renderer: FrameRenderer,
}

/// Resources selected from validated settings before shell startup.
pub struct LiveResources {
    /// One background cmap index survives all monitor/size reconfigurations.
    pub font_service: Option<crate::system_fonts::Service>,
    /// Reinstalled sources retain their event-driven completion notification.
    pub font_wake: Arc<dyn Fn() + Send + Sync>,
    /// Shared fonts and glyph atlases for this window.
    pub text_context: TextContext,
    /// File-backed settings and the active CLI/profile layers.
    pub settings: DesktopSettings,
}

/// Window, GPU and process handles that live for one OS window lifetime.
pub struct LiveWindow {
    /// Host-reserved left UI columns; ordinary shell surfaces reserve none.
    pub(crate) gutter_columns: u8,
    /// Opt-in evidence includes failed redraws rather than only successful frames.
    #[cfg(feature = "lock-metrics")]
    pub frame_metrics: frame_metrics::Metrics,
    /// The window owns native font discovery independently of its text context.
    pub font_service: Option<crate::system_fonts::Service>,
    /// Optional retained source counts describe the image built before actual submission.
    #[cfg(feature = "lock-metrics")]
    pub glyph_status: Option<[usize; 2]>,
    /// A diagnostic records successful presentations without preparing reference glyphs.
    #[cfg(feature = "lock-metrics")]
    pub ink_observer: ink_latency::Observer,
    /// Font worker completion is an event, independent of parser output damage.
    font_wake: Arc<dyn Fn() + Send + Sync>,
    /// Actual OS window creation origin, independent of later Session startup.
    window_created: Instant,
    /// Last successful surface configure, including early resize/DPR callbacks.
    surface_configured: Instant,
    /// Copied host observations and nonblocking native-thread notifications.
    pub window: HostView,
    /// Presentable Metal/Vulkan/DX12 target owned at the platform seam.
    surface: wgpu::Surface<'static>,
    /// IOSurface pool is local to this surface; text atlases remain shared.
    #[cfg(target_os = "macos")]
    pub(crate) presentation: Option<presentation::contents::Native>,
    /// Shared GPU device and queue used by the retained frame executor.
    pub renderer: Renderer,
    /// Device callbacks set a flag and wake the actor without touching its resources.
    device_lost: Arc<std::sync::atomic::AtomicBool>,
    /// Uncaptured GPU errors wake recovery instead of aborting the application.
    gpu_errors: puzzle_render::ErrorMonitor,
    /// Three persistent pipelines and growable instance buffers.
    pub frame_renderer: FrameRenderer,
    /// Fonts, shaping plans and CPU atlas shared across sessions.
    pub text_context: TextContext,
    /// User config and session-layer overrides for focus-triggered reload.
    pub settings: DesktopSettings,
    /// Last installed family order lets reload invalidate only changed sources.
    fallback_families: Vec<String>,
    /// Sending capability has no reference to the Session owned by its worker.
    pub session: Handle,
    /// Three allocations circulate by move; receiving is bounded by the frame deadline.
    frames: Frames,
    /// Rendering caches never leave this worker or wait for a parser loan.
    builder: FrameBuilder,
    /// No placeholder is returned before the first real circulating slot arrives.
    has_snapshot: bool,
    /// A late reply or unpresented output keeps the physical refresh source active.
    frame_demand: demand::FrameDemand,
    /// Immutable presented regions return from Session for allocation reuse.
    pub regions: puzzle_term::overlay::regions::PresentedRegions,
    /// Session observations replace synchronous animation queries.
    pub animating: bool,
    /// Publication completion separates owner wait from subsequent render work.
    pub snapshot_done: Instant,
    /// Current receive wait and the adopted answer's actual request latency.
    pub snapshot_wait_and_latency: [Duration; 2],
    /// Every answer, including unchanged and superseded ones, remains in diagnostics.
    #[cfg(feature = "lock-metrics")]
    pub publications: Vec<crate::session_thread::PublicationTiming>,
    /// The retained sealed slot is also the cold verifier’s exact source state.
    pub snapshot: FrameSnapshot,
    /// Retained neutral instances are built and rendered on the process worker.
    pub frame: RenderFrame,
    /// Last applied OSC title avoids allocating or calling AppKit per frame.
    pub title: String,
    /// Optional ordered diagnostic record shared with PTY worker threads.
    recorder: Option<Recorder>,
    /// Presented-frame ordinal, independent of winit redraw callbacks.
    next_frame: u64,
    /// The host focus state controls hollow cursor geometry in both draws.
    pub focused: bool,
    /// Last winit occlusion event; unknown stays distinct from visible.
    pub occluded: Option<bool>,
    /// Optional full-state comparison; absent from the normal frame path.
    pub verifier: Option<FrameVerifier>,
    /// A cold comparison waits until presentation has been idle for 250 ms.
    verification: VerifySchedule,
    /// Blink and frame identity must match the snapshot that was presented.
    verify_info: Option<VerifyRequest>,
}

/// Acquisition time lets the desktop infer a Fifo display phase.
pub struct PresentTiming {
    /// Instant just after a presentable texture became available.
    pub acquired: Instant,
    /// Time spent waiting for the texture rather than building VT geometry.
    pub blocked: Duration,
    /// Completion of the real render submission, before queue presentation.
    #[cfg(feature = "lock-metrics")]
    pub submitted: Instant,
    /// Return from presenting that drawable, excluding later diagnostic work.
    #[cfg(feature = "lock-metrics")]
    pub presented: Instant,
    /// Optional phase timings are absent from ordinary native builds.
    #[cfg(feature = "lock-metrics")]
    pub work: super::frame_work::FrameWork,
}

impl LiveWindow {
    /// Joins the platform-owned resources after the PTY child starts.
    pub fn new(
        window: HostView,
        surface: wgpu::Surface<'static>,
        gpu: LiveGpu,
        resources: LiveResources,
        session: Handle,
        frames: Frames,
        recorder: Option<Recorder>,
    ) -> Self {
        Self::new_with_loss(
            window, surface, gpu, resources, session, frames, recorder, None,
        )
    }

    /// Pooled startup retains the single process loss callback and prepared producer.
    #[allow(
        clippy::too_many_arguments,
        reason = "cold transfer of independently owned native resources"
    )]
    pub(crate) fn new_with_loss(
        window: HostView,
        surface: wgpu::Surface<'static>,
        mut gpu: LiveGpu,
        mut resources: LiveResources,
        session: Handle,
        frames: Frames,
        recorder: Option<Recorder>,
        shared_loss: Option<Arc<std::sync::atomic::AtomicBool>>,
    ) -> Self {
        resources.prepare(&mut gpu, shared_loss.is_none());
        let verifier = FrameVerifier::enabled(&gpu.renderer);
        let device_lost =
            shared_loss.unwrap_or_else(|| recovery::watch(&gpu.renderer, window.clone()));
        Self {
            #[cfg(feature = "lock-metrics")]
            frame_metrics: frame_metrics::Metrics::default(),
            font_service: resources.font_service,
            #[cfg(feature = "lock-metrics")]
            glyph_status: None,
            #[cfg(feature = "lock-metrics")]
            ink_observer: ink_latency::Observer::default(),
            font_wake: resources.font_wake,
            window_created: gpu.window_created,
            surface_configured: gpu.surface_configured,
            window,
            surface,
            renderer: gpu.renderer,
            device_lost,
            gpu_errors: gpu.gpu_errors,
            frame_renderer: gpu.frame_renderer,
            builder: FrameBuilder::new(&resources.text_context),
            text_context: resources.text_context,
            fallback_families: resources.settings.store.effective().font_fallback.to_vec(),
            settings: resources.settings,
            gutter_columns: 0,
            session,
            frames,
            has_snapshot: false,
            frame_demand: demand::FrameDemand::default(),
            regions: Default::default(),
            animating: false,
            snapshot_done: Instant::now(),
            snapshot_wait_and_latency: [Duration::ZERO; 2],
            #[cfg(feature = "lock-metrics")]
            publications: Vec::new(),
            snapshot: FrameSnapshot::default(),
            frame: RenderFrame::default(),
            title: String::new(),
            recorder,
            #[cfg(target_os = "macos")]
            presentation: None,
            next_frame: 0,
            focused: true,
            occluded: None,
            verifier,
            verification: VerifySchedule::default(),
            verify_info: None,
        }
    }

    /// Makes the GPU attachment match winit's physical backing pixels now.
    pub fn resize_surface(&mut self, size: PhysicalSize) {
        #[cfg(target_os = "macos")]
        if self.presentation.is_some() {
            // IOSurface storage follows requested bounds. Reconfiguring an unused
            // CAMetalLayer would mutate native geometry from the render worker.
            self.surface_configured = Instant::now();
            self.send(SessionMessage::SurfaceSize([size.width, size.height]));
            return;
        }
        let _stage = crate::resize_timing::Stage::new("surface_reconfigure");
        // Device-pixel extents come directly from winit. Passing logical
        // dimensions here would stretch text on a Retina monitor.
        if let Err(error) =
            crate::surface_config::configure(&self.renderer, &self.surface, size.width, size.height)
        {
            tracing::warn!(event="desktop.surface_config_failed", %error);
            return;
        }
        // Configuration can replace the drawable queue before phase learning.
        self.surface_configured = Instant::now();
        self.send(SessionMessage::SurfaceSize([size.width, size.height]));
    }

    /// Recomputes hinted/unhinted glyph metrics when monitor DPR changes.
    pub fn change_dpr(&mut self, scale: f64) {
        // Equal-key monitor notifications must preserve all D-30 resource owners.
        if self.text_context.dpr() == scale as f32 {
            return;
        }
        // Session retains the logical 14 px size; only the physical scale
        // and D-27 hinting choice change when the window crosses monitors.
        if let Err(error) = self
            .text_context
            .reconfigure(self.settings.run_font_size(), scale as f32)
        {
            tracing::warn!(event="desktop.dpr_failed", %error);
            return;
        }
        // Mapped source bytes were released; recreate metric-bound sources lazily.
        let font_wake = self.font_wake();
        if let Err(error) = crate::system_fonts::install_with_wake(
            &mut self.text_context,
            &self.fallback_families,
            font_wake,
            &mut self.font_service,
        ) {
            tracing::warn!(event="desktop.fallback_failed",%error);
        }
        if let Err(error) = crate::grid_rasterizer::install(
            &mut self.text_context,
            &self.renderer,
            &self.settings.store.effective().font_renderer,
        ) {
            tracing::warn!(event = "desktop.grid_rasterizer_failed", %error);
        }
        self.sync_font_metrics();
    }

    /// Reflows the latest grid once before this frame is assembled (§6.6).
    pub fn apply_grid_resize(&mut self) {
        let size = self.window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        let metrics = self.text_context.metrics();
        // Reserve the same inset as the frame builder before division;
        // otherwise the shell could write into an invisible last column.
        // The configured logical inset is rounded once at the actual device scale.
        // Integer division preserves the same device cells in VT and GPU.
        let padding = padding_for(&self.settings, self.text_context.dpr());
        let grid = grid_with_gutter(size, metrics, padding, self.gutter_columns);
        let columns = usize::from(grid.columns);
        let rows = usize::from(grid.rows);
        tracing::debug!(?size, columns, rows, "PTY and grid geometry updated");
        self.send(SessionMessage::GridResize {
            columns,
            rows,
            surface: [size.width, size.height],
        });
    }

    /// Sends the latest grid to the PTY when the first-event throttle fires.
    pub fn flush_pty_resize(&mut self) {
        let size = self.window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        tracing::trace!(target:"puzzle_desktop::resize",event="embed.pty_resize_sent",?size);
        self.send(SessionMessage::PtyResize([size.width, size.height]));
    }
}

/// Derives both VT and PTY dimensions from one physical backing extent.
pub fn grid_for_surface(
    size: impl Into<PhysicalSize>,
    metrics: CellMetrics,
    padding: u32,
) -> PtySize {
    grid_with_gutter(size, metrics, padding, 0)
}

/// Subtract reserved whole cells before dividing; height and surface extent are unchanged.
pub(crate) fn grid_with_gutter(
    size: impl Into<PhysicalSize>,
    metrics: CellMetrics,
    padding: u32,
    gutter: u8,
) -> PtySize {
    // FrameBuilder uses this same device inset on each edge. Integer division
    // keeps drawing, terminal reports and child geometry in agreement.
    let size = size.into();
    let columns = (size
        .width
        .saturating_sub(padding.saturating_mul(2))
        .saturating_sub(u32::from(gutter).saturating_mul(metrics.width_px))
        / metrics.width_px.max(1))
    .max(2);
    let rows =
        (size.height.saturating_sub(padding.saturating_mul(2)) / metrics.height_px.max(1)).max(1);
    PtySize {
        columns: columns.min(u32::from(u16::MAX)) as u16,
        rows: rows.min(u32::from(u16::MAX)) as u16,
        pixel_width: size.width.min(u32::from(u16::MAX)) as u16,
        pixel_height: size.height.min(u32::from(u16::MAX)) as u16,
    }
}

#[cfg(test)]
/// Physical-pixel grid calculations before shell spawn and after resize.
mod tests;

impl LiveWindow {
    /// Reconfigured text sources keep the live event-driven swap-in notification.
    pub fn font_wake(&self) -> std::sync::Arc<dyn Fn() + Send + Sync> {
        Arc::clone(&self.font_wake)
    }
}

impl LiveWindow {
    /// Diagnostic inputs are posted to the native adapter, never encoded on this worker.
    #[cfg(feature = "lock-metrics")]
    pub fn input(&self, input: crate::keyboard::raw::RawInput) {
        self.window.input(input);
    }
}

/// Render-owned resources communicate only copied geometry with the Session owner.
impl LiveWindow {
    /// A channel send cannot execute parser policy or wait for a Session turn.
    pub fn send(&self, message: SessionMessage) {
        if let Err(error) = self.session.send(message) {
            tracing::warn!(event = "desktop.session_message_failed", %error);
        }
    }
    /// Rebinds local caches and publishes only metrics, never font resources.
    pub fn sync_font_metrics(&mut self) {
        self.builder.set_context(&self.text_context);
        self.send(SessionMessage::ImageTextureLimit(
            self.renderer.device().limits().max_texture_dimension_2d,
        ));
        self.send(SessionMessage::FontMetrics(FontMetrics {
            cell: self.text_context.metrics(),
            font_size: self.text_context.font_size(),
            dpr: self.text_context.dpr(),
            atlas_generation: self.text_context.atlas_generation(),
        }));
        self.send(SessionMessage::Padding(padding_for(
            &self.settings,
            self.text_context.dpr(),
        )));
    }
}

/// Converts settings to the same bounded device inset accepted by frame policy.
pub(crate) fn padding_for(settings: &DesktopSettings, dpr: f32) -> u32 {
    (settings.store.effective().render_padding * dpr)
        .round()
        .clamp(0.0, 4096.0) as u32
}

/// Opt-in actual-drawable colour evidence, independent of golden rendering.
mod surface_colour;
