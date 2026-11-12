//! Native IOSurface submission shares the ordinary frame builder and GPU executor.
//!
//! GPU completion makes pixels eligible; only the main-thread CALayer assignment
//! acknowledges presented hit regions/caret geometry. No drawable is acquired.
//!
//! Snapshot publication and glyph preparation retain their existing ownership.
//! This module changes the final image destination, not the text renderer or its
//! encoded colour math. Atlas uploads still precede image acquisition, including
//! attempts that cannot obtain a free slot. Prepared ink therefore survives a
//! skipped presentation just as it does on the drawable path.
//!
//! A geometry-change callback may have a newer request than the publication in
//! hand. Such an old-size image cannot fulfil the synchronous transaction: it is
//! skipped and the previous layer contents stay top-left rather than falsely
//! advertising the new extent. The owner publishes the requested grid next.
//! Presentation metadata is sealed before the asynchronous completion fence;
//! actual user-visible hit regions are acknowledged later by App's CA receipt.
use super::{LiveWindow, PresentTiming};
use puzzle_sys::presentation::{Geometry, RenderImage, Target};
use std::time::{Duration, Instant};

/// Per-surface native resources move with the existing exclusive render loan.
pub(crate) struct Native {
    /// Bounded pool excludes compositor-owned and in-flight textures.
    target: Target,
    /// Encoding precedes metadata publication and its GPU completion fence.
    pending: Option<RenderImage>,
}
impl Native {
    /// Called on main before transferring ownership to the process render worker.
    pub(crate) fn new(target: Target) -> Self {
        Self {
            target,
            pending: None,
        }
    }
    /// A replaced device cannot render into an old device's imported textures.
    pub(crate) fn reset(&mut self) {
        self.pending = None;
        self.target.reset();
    }
}
impl LiveWindow {
    /// Render the exact backing extent into a reusable IOSurface without a FIFO wait.
    pub(super) fn present_contents(&mut self, _parsed: u64) -> Option<PresentTiming> {
        let _started = Instant::now();
        // This is the same encoded clear used by the GPU, including theme and
        // OSC defaults. Publish it even when the pool/geometry skips this image.
        let geometry = self.contents_geometry()?;
        // Cached glyph handles remain valid even if this attempt finds no free
        // presentation slot. Flush their uploads before making that decision.
        let uploads = self.write_glyph_uploads()?;
        // An old publication may arrive after a resize request. Its grid is still
        // clipped safely by other hosts, but it cannot satisfy a synchronous
        // new-geometry commit: keep the previous top-left frame and retry.
        if self.frame.target_size != geometry.size {
            tracing::trace!(target:"puzzle_desktop::resize",event="embed.contents_skipped", reason="stale_publication",published=?self.frame.target_size,requested=?geometry);
            self.window.request_redraw();
            return None;
        }
        let output = match self
            .presentation
            .as_mut()?
            .target
            .acquire(self.renderer.device(), geometry)
        {
            Ok(Some(output)) => output,
            Ok(None) => {
                tracing::trace!(target:"puzzle_desktop::resize",event="embed.contents_skipped",reason="pool_owned",requested=?geometry);
                // No wait here: another surface on this actor must stay responsive.
                // Pending visual demand keeps the physical link subscribed for retry.
                self.window.request_redraw();
                return None;
            }
            Err(error) => {
                tracing::error!(event="embed.contents_acquire_failed", %error);
                self.failed_frame();
                return None;
            }
        };
        let acquired = Instant::now();
        // Validation errors identify both sealed source extent and actual target.
        // The global handler can then recover without losing the failed frame's
        // geometry when a host resized between publication and rendering.
        self.gpu_errors.context(puzzle_render::FrameContext {
            number: self.next_frame,
            published: self.frame.target_size,
            acquired: geometry.size,
        });
        // The ordinary frame executor draws into this imported allocation. There
        // is no separate screenshot render, CPU copy or presentation-specific shader.
        let view = output
            .texture()
            .create_view(&wgpu::TextureViewDescriptor::default());
        // Encoding retains upload/draw queue order, including atlas batch flushes.
        // GPU completion of that submit is observed by the later native fence.
        let _upload = self.encode_contents(&view, uploads)?;
        // This diagnostic reads the same imported texture, never an oracle substitute.
        self.observe_contents_colour(&output);
        let _presented = Instant::now();
        // Keep the image until its sealed metadata has been queued. A very fast
        // GPU must not wake main with pixels whose region table is still absent.
        self.presentation.as_mut()?.pending = Some(output);
        // This is a submission ordinal. Recording/host events wait for CA commit.
        self.next_frame += 1;
        Some(PresentTiming {
            acquired,
            blocked: Duration::ZERO,
            #[cfg(feature = "lock-metrics")]
            submitted: _presented,
            #[cfg(feature = "lock-metrics")]
            presented: _presented,
            #[cfg(feature = "lock-metrics")]
            work: crate::window::frame_work::FrameWork {
                upload_cpu: _upload,
                submit_cpu: _presented.duration_since(_started),
                ..Default::default()
            },
        })
    }
    /// Background colour travels independently of optional target acquisition.
    fn contents_geometry(&self) -> Option<Geometry> {
        self.presentation
            .as_ref()?
            .target
            .background(self.frame.clear);
        let info = self.window.info;
        Some(Geometry {
            size: [info.extent.width, info.extent.height],
            scale: info.scale,
        })
    }
    /// Failed encoding needs a fresh publication, just like a lost drawable.
    fn encode_contents(&mut self, view: &wgpu::TextureView, uploads: Duration) -> Option<Duration> {
        let encoded = self.encode_frame(view, uploads);
        if encoded.is_none() || self.gpu_errors.take_error().is_some() {
            // This native path has no acquired drawable to release before resize.
            // The caller drops its unsubmitted image; recovery invalidates the
            // sealed snapshot so consumed Session damage is captured again.
            self.failed_frame();
            return None;
        }
        encoded
    }
    /// Opt-in evidence reads the same allocation and its compositor ICC attachment.
    fn observe_contents_colour(&self, output: &RenderImage) {
        let mut colour = self.capture_surface_colour(output.texture());
        if let Some(colour) = &mut colour {
            colour["layer_srgb"] = serde_json::json!(output.is_srgb());
            colour["presentation"] = serde_json::json!("IOSurface");
        }
        self.record_surface_colour(colour);
    }
    /// Queue immutable metadata *before* the completion fence can wake the UI.
    pub(super) fn finish_contents(&mut self, parsed: u64) {
        if let Some(native) = &self.presentation {
            // A blocked main loop may let many completed images coalesce. Their
            // metadata must retire as promptly as their reusable native slots.
            while let Some(serial) = native.target.take_retired() {
                self.window.retire_submission(serial);
            }
        }
        // Ordinals count submitted images and do not share a display-link index
        // namespace. The CA callback can therefore coalesce completions safely.
        let serial = self.next_frame.saturating_sub(1);
        self.builder.capture_presented_regions(&mut self.regions);
        // These owned values refer only to the sealed snapshot. Session may
        // continue parsing while CA retains the image or a newer one supersedes it.
        self.window.submitted(crate::app::presentation::Submission {
            serial,
            parsed,
            recorder: self.recorder.clone(),
            geometry: Geometry {
                size: self.frame.target_size,
                scale: self.window.info.scale,
            },
            cursor: self.builder.cursor_extent(&self.snapshot),
            regions: std::mem::take(&mut self.regions),
            statistics: self.builder.take_frame_statistics(),
        });
        if let Some(native) = &mut self.presentation
            && let Some(output) = native.pending.take()
            && let Err(error) = native
                .target
                .submitted(output, self.renderer.queue(), serial)
        {
            // A failed fence cannot later certify any of these pending images.
            // Force owner recapture after returning all unpresented region storage.
            self.window.discard_submissions();
            tracing::error!(event="embed.contents_submit_failed", %error);
            self.failed_frame();
        }
    }
}
