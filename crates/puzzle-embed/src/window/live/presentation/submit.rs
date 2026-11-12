//! Native GPU submission consumes published values without accessing the Session.
//! Persistent glyph uploads precede drawable acquisition, so a hidden or lost
//! surface cannot strand prepared cache handles without their bytes. Instance
//! writes and command encoding follow acquisition and retain queue ordering.
//! The optional observer separates upload-only cost from full upload/submit CPU.
//! FIFO waiting is measured independently and subtracted from that CPU reserve;
//! the warm full-screen budget combines it with build p95 and D-33's margin.
//! A submit timestamp precedes handing the image to the compositor. Neither
//! timestamp claims physical scanout or substitutes for hardware review.
//! Failed acquisition and rendering leave a named gate in the original attempt
//! chronology. They never fabricate a successful presentation or consume ink.
//! Snapshot dimensions can lag a native size/DPR jump. Projection and clipping
//! therefore use the acquired attachment, preserving old ink at the top left.
//! Uncaptured errors retain only scalar evidence and wake this resource owner.
//! Recovery drops the acquired image before configure and asks for a full recapture.
use super::{LiveWindow, PresentTiming};
use std::time::Instant;

/// GPU ownership remains on the one process render thread.
impl LiveWindow {
    /// Presents a built frame while input and parsing remain independently owned.
    pub fn present_frame(&mut self, parsed_at_snapshot: u64) -> Option<PresentTiming> {
        #[cfg(target_os = "macos")]
        if self.presentation.is_some() {
            return self.present_contents(parsed_at_snapshot);
        }
        #[cfg(feature = "lock-metrics")]
        let present_started = Instant::now();
        // Upload and acquire are observed independently of snapshot geometry.
        let (output, acquired, blocked, _glyph_upload_cpu) = self.prepare_drawable()?;
        // This view belongs to the just-acquired surface image, never the oracle target.
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let upload = self.encode_frame(&view, _glyph_upload_cpu);
        if upload.is_none() {
            // No acquired drawable may survive a surface reconfiguration.
            // The view also owns the texture; release it before replacing the surface queue.
            drop(view);
            drop(output);
            self.failed_frame();
            return None;
        }
        let _upload_cpu = upload?;
        // Keep compositor geometry paired with the image actually submitted, rather
        // than only the earlier acquisition. Host bounds can change between them.
        // These scalar getters run only when this trace target is enabled (P9).
        #[cfg(target_os = "macos")]
        tracing::trace!(target:"puzzle_desktop::resize",event="desktop.layer_present",
            frame=self.next_frame, configured=?self.window.inner_size(),
            host_scale=self.window.scale_factor(), published=?self.frame.target_size,
            texture=?output.texture.size(),
            geometry=?puzzle_sys::layer::surface_geometry(&self.surface));
        // A synchronous uncaptured failure invalidates this drawable; do not present it.
        if self.gpu_errors.take_error().is_some() {
            // Drop the acquired image before reconfiguration, as required by wgpu.
            // The view also owns the texture; release it before replacing the surface queue.
            drop(view);
            drop(output);
            self.failed_frame();
            return None;
        }
        // Submission and presentation remain in device order. A later
        // offscreen verification cannot be the image users just saw.
        #[cfg(feature = "lock-metrics")]
        let submitted = Instant::now();
        // The diagnostic copies this very drawable after encoding, then presents it.
        // It observes only the first frame; production does no map/copy work.
        let colour = self.capture_surface_colour(&output.texture);
        // Present only the image submitted by the ordinary frame renderer.
        self.renderer.present_surface_texture(output);
        self.record_surface_colour(colour);
        #[cfg(feature = "lock-metrics")]
        let presented = Instant::now();
        // Recorded bytes describe the captured VT state even if parsing advanced.
        // Recorder formatting/file I/O follows the submission clock and is not GPU work.
        self.record_presented(parsed_at_snapshot);
        self.notify_presented_geometry();
        Some(PresentTiming {
            acquired,
            blocked,
            #[cfg(feature = "lock-metrics")]
            submitted,
            #[cfg(feature = "lock-metrics")]
            presented,
            #[cfg(feature = "lock-metrics")]
            work: crate::window::frame_work::FrameWork {
                upload_cpu: _upload_cpu,
                // CPU preparation for submission excludes the independently recorded FIFO wait.
                submit_cpu: presented
                    .duration_since(present_started)
                    .saturating_sub(blocked),
                ..Default::default()
            },
        })
    }

    /// Uses the sealed cursor and the exact geometry built for this presented image.
    /// Line identity resolves through the viewport, so scrolling past the caret
    /// naturally returns None instead of retaining a stale OS candidate position.
    /// Width belongs to the captured cell, including a wide cluster's second cell.
    /// Padding and fractional scroll offsets already belong to TextGeometry.
    /// No live Session query or device-pixel conversion can race this observation.
    fn notify_presented_geometry(&mut self) {
        let rect = self.builder.cursor_extent(&self.snapshot);
        // Geometry uses logical pixels, so AppKit consumes points without DPR division.
        self.window.presented(rect);
    }

    /// Context follows the actual attachment, not the Session's last geometry message.
    fn prepare_drawable(
        &mut self,
    ) -> Option<(
        wgpu::SurfaceTexture,
        Instant,
        std::time::Duration,
        std::time::Duration,
    )> {
        // Upload errors need context too, before any surface image has been obtained.
        // An acquired extent of zero identifies this pre-acquisition stage in the log.
        self.gpu_errors.context(puzzle_render::FrameContext {
            number: self.next_frame,
            published: self.frame.target_size,
            acquired: [0; 2],
        });
        let uploads = self.write_glyph_uploads()?;
        let (output, acquired, blocked) = self.acquire_surface()?;
        // Host resize messages can race this acquisition; the texture itself is authoritative.
        let size = output.texture.size();
        self.gpu_errors.context(puzzle_render::FrameContext {
            number: self.next_frame,
            published: self.frame.target_size,
            acquired: [size.width, size.height],
        });
        #[cfg(target_os = "macos")]
        tracing::trace!(target:"puzzle_desktop::resize",event="desktop.layer_drawable",
            frame=self.next_frame, configured=?self.window.inner_size(),
            host_scale=self.window.scale_factor(), published=?self.frame.target_size,
            acquired=?[size.width,size.height],
            geometry=?puzzle_sys::layer::surface_geometry(&self.surface));
        Some((output, acquired, blocked, uploads))
    }

    /// Failed draws force a new owner publication after releasing their texture.
    pub(super) fn failed_frame(&mut self) {
        // The slot and Session publication identity must both forget the failed frame.
        // Reusing unchanged cached rows is unsafe until a fresh owner recapture arrives.
        self.snapshot.invalidate();
        self.send(puzzle_term::SessionMessage::FrameResult(
            puzzle_term::FrameResult::BuildFailed,
        ));
        // Callers release their drawable first: configure may replace the entire queue.
        // A minimized window resumes recovery when a nonzero geometry message arrives.
        let size = self.window.inner_size();
        if size.width > 0 && size.height > 0 {
            self.resize_surface(size);
        }
    }

    /// Assigns an ordinal only after a real surface submission and present.
    pub(super) fn record_presented(&mut self, parsed_at_snapshot: u64) {
        // Record the sealed byte offset without querying the Session. It belongs
        // to the frozen snapshot, never the parser's later current total.
        tracing::debug!(target: "puzzle_desktop::frames", event = "Presented", parsed_bytes = parsed_at_snapshot);
        if let Some(recorder) = &self.recorder
            && let Err(error) = recorder.frame(self.next_frame, parsed_at_snapshot)
        {
            tracing::warn!(event = "desktop.record_failed", %error);
        }
        // The ordinal counts successful frames, not arbitrary winit
        // RedrawRequested callbacks that found no Session damage.
        self.next_frame += 1;
    }

    /// Queues cached glyph bytes even when the following acquire has no image.
    pub(super) fn write_glyph_uploads(&mut self) -> Option<std::time::Duration> {
        #[cfg(feature = "lock-metrics")]
        let glyph_upload_started = Instant::now();
        // Glyph handles are already cached and the CPU dirty spans were drained.
        // Transfer their bytes to the persistent GPU atlas before acquisition can
        // skip an occluded/lost drawable. A later frame may reuse every handle.
        if let Err(error) = self
            .frame_renderer
            .write_frame_uploads(&self.renderer, &mut self.frame)
        {
            #[cfg(feature = "lock-metrics")]
            self.frame_metrics.gate("upload_failed");
            tracing::warn!(event = "desktop.glyph_upload_failed", %error);
            return None;
        }
        // Successful queue writes own these bytes even without a drawable. Their
        // next render submission flushes them before sampling the cached glyphs.
        #[cfg(feature = "lock-metrics")]
        let elapsed = glyph_upload_started.elapsed();
        #[cfg(not(feature = "lock-metrics"))]
        let elapsed = std::time::Duration::ZERO;
        Some(elapsed)
    }

    /// Acquires a Fifo slot and returns its measured refresh-phase sample.
    pub fn acquire_surface(
        &mut self,
    ) -> Option<(wgpu::SurfaceTexture, Instant, std::time::Duration)> {
        // A host crossing can arrive after snapshot/build began on the old display.
        // Keep uploads, skip this obsolete FIFO, and let the queued geometry select
        // the destination source on the next actor turn. Size-only frames stay clipped.
        if !self.window.acquisition_current() {
            #[cfg(feature = "lock-metrics")]
            self.frame_metrics.gate("display_changed_before_acquire");
            tracing::trace!(target: "puzzle_desktop::resize",
                event="desktop.old_display_acquire_cancelled", display=self.window.info.display);
            return None;
        }
        // This opt-in paired clock attributes FIFO waits separately from CPU frame work.
        let _stage = crate::resize_timing::Stage::new("drawable_acquire");
        // Fifo can wait for vsync here. No Session is accessed, and the
        // recording retains the earlier snapshot's parsed count.
        let started = Instant::now();
        // Time the blocking acquisition itself, excluding text shaping.
        // A wait over 1 ms is evidence that Fifo reached a refresh slot.
        let output = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(output)
            | wgpu::CurrentSurfaceTexture::Suboptimal(output) => output,
            // Hidden windows do not teach a display phase or count as
            // successful visible presentations in the D-29 perf logger.
            wgpu::CurrentSurfaceTexture::Occluded => {
                #[cfg(feature = "lock-metrics")]
                self.frame_metrics.gate("surface_occluded");
                tracing::trace!(target: "puzzle_desktop::glyphs", "acquire occluded");
                return None;
            }
            // A transient timeout must request a retry. Otherwise the
            // consumed Session damage could leave an idle window stale.
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Outdated
            | wgpu::CurrentSurfaceTexture::Lost
            | wgpu::CurrentSurfaceTexture::Validation => {
                #[cfg(feature = "lock-metrics")]
                self.frame_metrics.gate("surface_retry");
                tracing::trace!(target: "puzzle_desktop::glyphs", "acquire failed; reconfiguring");
                // A stale surface after a monitor move needs reconfigure;
                // the next VT wake can then retry with the new format.
                self.failed_frame();
                return None;
            }
        };
        // This timestamp approximates the compositor phase only if the
        // acquisition blocked; Pacer checks that threshold itself.
        // Capture immediately after acquisition so upload cost cannot shift the phase sample.
        let acquired = Instant::now();
        Some((output, acquired, acquired.duration_since(started)))
    }
    /// Encode the acquired image and separately observe atlas/instance queue writes.
    /// Rendering errors preserve the prior image and reconfigure before the next turn.
    pub(super) fn encode_frame(
        &mut self,
        view: &wgpu::TextureView,
        _glyph_upload_cpu: std::time::Duration,
    ) -> Option<std::time::Duration> {
        // Encoding/submission is distinct from both glyph layout and drawable waiting.
        // The observer records thread CPU as well as wall time without a GPU readback.
        let _stage = crate::resize_timing::Stage::new("render_encode_submit");
        #[cfg(feature = "lock-metrics")]
        let upload_started = Instant::now();
        #[cfg(feature = "lock-metrics")]
        let mut upload_cpu = std::time::Duration::ZERO;
        #[cfg(feature = "lock-metrics")]
        let rendered =
            self.frame_renderer
                .render_view_observed(&self.renderer, &mut self.frame, view, || {
                    upload_cpu = _glyph_upload_cpu + upload_started.elapsed()
                });
        #[cfg(not(feature = "lock-metrics"))]
        let rendered = self
            .frame_renderer
            .render_view(&self.renderer, &mut self.frame, view);
        if let Err(error) = rendered {
            #[cfg(feature = "lock-metrics")]
            self.frame_metrics.gate("render_failed");
            // Keep the prior visible frame and reconfigure after a device
            // or surface change; no partial pass is presented.
            tracing::warn!(event="desktop.render_failed", %error, "will reconfigure the surface");
            return None;
        }
        #[cfg(feature = "lock-metrics")]
        return Some(upload_cpu);
        #[cfg(not(feature = "lock-metrics"))]
        Some(std::time::Duration::ZERO)
    }
}
