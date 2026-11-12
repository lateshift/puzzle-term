//! Moving resource loans on the sole render actor (D-30).
//!
//! A surface retains only its sealed snapshot, row caches and presentation state.
//! The actor lends one owned font/atlas bundle for a turn, then takes it back.
//! Joining and splitting move fields; neither operation allocates or uses a lock.
use super::*;

/// Fonts and GPU atlas executor lent to one surface at a time.
pub(crate) struct Shared {
    /// The shared context retains discovery endpoints for its native font policy.
    pub(crate) font_service: Option<crate::system_fonts::Service>,
    /// Font worker completion is an event, independent of parser output damage.
    pub(crate) font_wake: Arc<dyn Fn() + Send + Sync>,
    /// Shared GPU device and queue used by the retained frame executor.
    pub(crate) renderer: Renderer,
    /// Device callbacks set a flag and wake the actor without touching its resources.
    pub(crate) device_lost: Arc<std::sync::atomic::AtomicBool>,
    /// Uncaptured GPU errors wake recovery instead of aborting the application.
    pub(crate) gpu_errors: puzzle_render::ErrorMonitor,
    /// Three persistent pipelines and growable instance buffers.
    pub(crate) frame_renderer: FrameRenderer,
    /// Fonts, shaping plans and CPU atlas shared across sessions.
    pub(crate) text_context: TextContext,
    /// Last installed family order lets reload invalidate only changed sources.
    pub(crate) fallback_families: Vec<String>,
}

/// Per-surface frame data, independent of the selected text-context key.
pub(crate) struct SurfaceState {
    /// Imported image ownership survives each exclusive text-context loan.
    #[cfg(target_os = "macos")]
    presentation: Option<presentation::contents::Native>,
    /// GPU images remain surface-local across shared font/DPR context changes.
    image_cache: Option<Box<puzzle_render::ImageCache>>,
    /// Surface-local UI cells survive resource loans and display changes.
    gutter_columns: u8,
    /// Opt-in evidence includes failed redraws rather than only successful frames.
    #[cfg(feature = "lock-metrics")]
    pub(crate) frame_metrics: frame_metrics::Metrics,
    /// Optional retained source counts describe the image built before actual submission.
    #[cfg(feature = "lock-metrics")]
    pub(crate) glyph_status: Option<[usize; 2]>,
    /// A diagnostic records successful presentations without preparing reference glyphs.
    #[cfg(feature = "lock-metrics")]
    pub(crate) ink_observer: ink_latency::Observer,
    /// Actual OS window creation origin, independent of later Session startup.
    pub(crate) window_created: Instant,
    /// Last successful surface configure, including early resize/DPR callbacks.
    pub(crate) surface_configured: Instant,
    /// Copied host observations and nonblocking native-thread notifications.
    pub(crate) window: HostView,
    /// Presentable Metal/Vulkan/DX12 target owned at the platform seam.
    pub(crate) surface: wgpu::Surface<'static>,
    /// User config and session-layer overrides for focus-triggered reload.
    pub(crate) settings: DesktopSettings,
    /// Sending capability has no reference to the Session owned by its worker.
    pub(crate) session: Handle,
    /// Three allocations circulate by move; receiving is bounded by the frame deadline.
    pub(crate) frames: Frames,
    /// Rendering caches never leave this worker or wait for a parser loan.
    pub(crate) builder: FrameBuilder,
    /// No placeholder is returned before the first real circulating slot arrives.
    pub(crate) has_snapshot: bool,
    /// A late reply or unpresented output keeps the physical refresh source active.
    frame_demand: demand::FrameDemand,
    /// Immutable presented regions return from Session for allocation reuse.
    pub(crate) regions: puzzle_term::overlay::regions::PresentedRegions,
    /// Session observations replace synchronous animation queries.
    pub(crate) animating: bool,
    /// Publication completion separates owner wait from subsequent render work.
    pub(crate) snapshot_done: Instant,
    /// Current receive wait and the adopted answer's actual request latency.
    pub(crate) snapshot_wait_and_latency: [Duration; 2],
    /// Every answer, including unchanged and superseded ones, remains in diagnostics.
    #[cfg(feature = "lock-metrics")]
    pub(crate) publications: Vec<crate::session_thread::PublicationTiming>,
    /// The retained sealed slot is also the cold verifier’s exact source state.
    pub(crate) snapshot: FrameSnapshot,
    /// Retained neutral instances are built and rendered on the process worker.
    pub(crate) frame: RenderFrame,
    /// Last applied OSC title avoids allocating or calling AppKit per frame.
    pub(crate) title: String,
    /// Optional ordered diagnostic record shared with PTY worker threads.
    pub(crate) recorder: Option<Recorder>,
    /// Presented-frame ordinal, independent of winit redraw callbacks.
    pub(crate) next_frame: u64,
    /// The host focus state controls hollow cursor geometry in both draws.
    pub(crate) focused: bool,
    /// Last winit occlusion event; unknown stays distinct from visible.
    pub(crate) occluded: Option<bool>,
    /// Optional full-state comparison; absent from the normal frame path.
    pub(crate) verifier: Option<FrameVerifier>,
    /// A cold comparison waits until presentation has been idle for 250 ms.
    verification: VerifySchedule,
    /// Blink and frame identity must match the snapshot that was presented.
    pub(crate) verify_info: Option<VerifyRequest>,
}

impl LiveWindow {
    /// Returns the text loan without discarding any per-surface cached frame.
    pub(crate) fn split(mut self) -> (SurfaceState, Shared) {
        let image_cache = self.frame_renderer.take_image_cache();
        (
            SurfaceState {
                #[cfg(target_os = "macos")]
                presentation: self.presentation,
                image_cache,
                gutter_columns: self.gutter_columns,
                #[cfg(feature = "lock-metrics")]
                frame_metrics: self.frame_metrics,
                #[cfg(feature = "lock-metrics")]
                glyph_status: self.glyph_status,
                #[cfg(feature = "lock-metrics")]
                ink_observer: self.ink_observer,
                window_created: self.window_created, // Startup timestamps stay with this target, never with reused fonts.
                surface_configured: self.surface_configured,
                window: self.window, // Host actions and native backing ownership follow one surface lifetime.
                surface: self.surface,
                settings: self.settings, // Per-surface preferences choose a context but are not mutable shared policy.
                session: self.session, // IO capabilities and publication slots cannot cross into another terminal.
                frames: self.frames,
                builder: self.builder, // Cached rows name this Session’s LineIds, so sharing them would corrupt ink.
                has_snapshot: self.has_snapshot, // Publication identity survives lending fonts to the next sibling.
                frame_demand: self.frame_demand, // Pending output must keep the physical refresh subscription alive.
                regions: self.regions, // The user hit-tests this surface’s last presented regions only.
                animating: self.animating,
                snapshot_done: self.snapshot_done, // Paired timing belongs to this request, independently of atlas reuse.
                snapshot_wait_and_latency: self.snapshot_wait_and_latency,
                #[cfg(feature = "lock-metrics")]
                publications: self.publications,
                snapshot: self.snapshot, // The retained sealed slot remains the verifier’s exact input.
                frame: self.frame, // Reuse instance-vector capacities instead of copying a finished frame.
                title: self.title, // Title changes are delivered per surface and not per font key.
                recorder: self.recorder,
                next_frame: self.next_frame, // Recording ordinals count presentations of this target only.
                focused: self.focused, // Cursor/occlusion observations never follow the shared atlas.
                occluded: self.occluded,
                verifier: self.verifier, // Optional verification keeps its own last presentation and idle schedule.
                verification: self.verification,
                verify_info: self.verify_info,
            },
            Shared {
                font_service: self.font_service, // Discovery and text caches leave together so source results stay compatible.
                font_wake: self.font_wake,
                renderer: self.renderer, // Device-bound executor, textures and CPU cache form one moving bundle.
                device_lost: self.device_lost,
                gpu_errors: self.gpu_errors,
                frame_renderer: self.frame_renderer,
                text_context: self.text_context, // Returning the CPU atlas preserves addresses used by sibling row handles.
                fallback_families: self.fallback_families,
            },
        )
    }
}

impl SurfaceState {
    /// Reassembles a borrow-free turn using the actor’s exclusively owned context.
    pub(crate) fn join(self, mut shared: Shared) -> LiveWindow {
        shared.frame_renderer.set_image_cache(self.image_cache);
        LiveWindow {
            #[cfg(target_os = "macos")]
            presentation: self.presentation,
            #[cfg(feature = "lock-metrics")]
            frame_metrics: self.frame_metrics,
            font_service: shared.font_service, // Native source completion resumes against the same shared policy.
            #[cfg(feature = "lock-metrics")]
            glyph_status: self.glyph_status,
            #[cfg(feature = "lock-metrics")]
            ink_observer: self.ink_observer,
            font_wake: shared.font_wake, // The process notifier wakes all users of the context, not just this window.
            window_created: self.window_created, // Joining must not restart startup or configure-age measurements.
            surface_configured: self.surface_configured,
            window: self.window, // Borrow-free assembly retains the safe native owner for this turn.
            surface: self.surface,
            renderer: shared.renderer, // All executor handles came from the one process device incarnation.
            device_lost: shared.device_lost,
            gpu_errors: shared.gpu_errors,
            frame_renderer: shared.frame_renderer, // Already-uploaded glyph ink remains sampled after a different window draws.
            text_context: shared.text_context,
            settings: self.settings, // Local settings remain independent even when two keys currently match.
            gutter_columns: self.gutter_columns, // Reserved UI space stays with the surface through font loans.
            fallback_families: shared.fallback_families,
            session: self.session, // The scheduler receives only its own Session’s publications.
            frames: self.frames,
            builder: self.builder, // Row-cache generation checks reject entries invalidated by another surface.
            has_snapshot: self.has_snapshot,
            frame_demand: self.frame_demand, // Preserve outstanding replies across the resource loan boundary.
            regions: self.regions, // Presented-region ownership stays aligned with this surface’s pointer stream.
            animating: self.animating,
            snapshot_done: self.snapshot_done, // Snapshot latency is distinct from the time spent in shared glyph work.
            snapshot_wait_and_latency: self.snapshot_wait_and_latency,
            #[cfg(feature = "lock-metrics")]
            publications: self.publications,
            snapshot: self.snapshot, // No cell copy occurs here; the circulating snapshot keeps its allocation.
            frame: self.frame,       // Frame buffers remain reusable per-surface assembly scratch.
            title: self.title,
            recorder: self.recorder, // Optional transcripts continue in the original worker-produced order.
            next_frame: self.next_frame,
            focused: self.focused, // These observations determine presentation eligibility and cursor appearance.
            occluded: self.occluded,
            verifier: self.verifier, // Diagnostic comparison remains associated with the frame it observed.
            verification: self.verification,
            verify_info: self.verify_info,
        }
    }
}

impl LiveWindow {
    /// A new key/device cannot reuse cached instances naming an older atlas.
    pub(crate) fn adopt_context(&mut self, restored: bool) {
        // An old slot may otherwise claim that rows naming the prior atlas are unchanged.
        self.snapshot.invalidate();
        // Pending spans name the retired context; the new context supplies its own dirty data.
        self.frame.uploads.clear();
        // set_context also advances the builder style epoch even if generation values coincide.
        self.sync_font_metrics();
        if restored {
            #[cfg(target_os = "macos")]
            self.window.discard_submissions();
            #[cfg(target_os = "macos")]
            if let Some(native) = &mut self.presentation {
                native.reset();
            }
            self.verifier = FrameVerifier::enabled(&self.renderer);
            let size = self.window.inner_size();
            if size.width > 0 && size.height > 0 {
                self.resize_surface(size);
            }
        }
    }
}
impl SurfaceState {
    /// Subscription reads output counters and sealed demand, never the Session.
    #[cfg(target_os = "macos")]
    pub(crate) fn refresh_display(
        &self,
        pending: &crate::render_thread::surface::Pending,
        pacer: Option<&crate::window::pacing::Pacer>,
        timer: Option<Instant>,
    ) -> Option<u32> {
        let visible = &self.window.visibility;
        let extent = self.window.info.extent;
        if visible.visible == Some(false)
            || visible.occluded == Some(true)
            || extent.width == 0
            || extent.height == 0
        {
            tracing::trace!(target: "puzzle_desktop::scheduling", event="desktop.unsubscribe_reason",
                reason="hidden_occluded_or_zero_extent", visible=?visible.visible, occluded=?visible.occluded, ?extent);
            return None;
        }
        let needs = pending.frame
            || pending.output.is_some()
            || pending.fallback
            || self.animating
            || self
                .frame_demand
                .pending(self.session.parsed_total(), self.session.pending_bytes())
            || pacer.is_some_and(crate::window::pacing::Pacer::wants_refresh);
        if !needs {
            tracing::trace!(target: "puzzle_desktop::scheduling", event="desktop.unsubscribe_reason",
                reason="no_render_demand", pending_bytes=self.session.pending_bytes(), timer_pending=timer.is_some());
        }
        needs
            .then(|| u32::try_from(self.window.info.display).ok())
            .flatten()
    }
}

impl LiveResources {
    /// Legacy probes install their producer; actor-pooled contexts arrive already prepared.
    pub(super) fn prepare(&mut self, gpu: &mut LiveGpu, install: bool) {
        // Initial CPU and sampled-GPU capacities use the same validated allowance.
        gpu.frame_renderer
            .set_glyph_budget(&gpu.renderer, self.text_context.glyph_budget_bytes());
        // Replacing a prepared producer during every surface startup would discard its GPU state.
        // The actor registry installs it once per key; independent diagnostic owners still need it.
        if install
            && let Err(error) = crate::grid_rasterizer::install(
                &mut self.text_context,
                &gpu.renderer,
                &self.settings.store.effective().font_renderer,
            )
        {
            // A configured unsupported producer is recoverable input, not a host-boundary panic.
            tracing::warn!(event = "desktop.grid_rasterizer_failed", %error);
        }
    }
}
