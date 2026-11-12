//! Bounded idle FIFO filling for refresh phase learning (§10.4).
//!
//! Metal reserves latency + 1 drawables. Those may all be acquired quickly;
//! the extra acquire can encounter backpressure. Stop as soon
//! as a qualifying wait occurs, including when startup already filled slots.
//!
//! Calibration submits the last image rather than taking a new snapshot, so
//! it cannot flash a partially parsed application update during a quiet turn.
//! A free acquire reports no sample; only measured FIFO backpressure learns
//! phase. The pure policy bounds attempts for either approved latency value.
#[cfg(not(target_os = "macos"))]
use super::{LiveWindow, PresentTiming};
#[cfg(all(feature = "lock-metrics", not(target_os = "macos")))]
use std::time::Instant;
/// Development timestamps are formatted after the attempt, avoiding paced logging.
#[cfg(all(feature = "lock-metrics", not(target_os = "macos")))]
mod trace;
#[cfg(all(feature = "lock-metrics", not(target_os = "macos")))]
pub use trace::Trace;

/// Acquisition-count policy is independent of GPU rendering and logging.
mod policy;
#[cfg(not(target_os = "macos"))]
use policy::Calibration;

#[cfg(test)]
mod tests;

#[cfg(not(target_os = "macos"))]
impl LiveWindow {
    /// Records visibility around one attempt without changing calibration eligibility.
    pub fn calibrate_phase(&mut self) -> Option<PresentTiming> {
        // Winit observations are recorded even when the attempt yields no sample.
        // Focus is evidence only: visible unfocused windows use this same path.
        self.log_visibility("calibration");
        #[cfg(all(feature = "lock-metrics", not(target_os = "macos")))]
        self.visibility_evidence("calibration_start");
        // Frontmost-process queries stay outside the uninterrupted acquire loop.
        // A non-blocking attempt remains no sample: the pacer arranges the next
        // idle retry, without treating focus or a free drawable as a refresh.
        let sample = self.calibration_present();
        #[cfg(all(feature = "lock-metrics", not(target_os = "macos")))]
        self.visibility_evidence("calibration_end");
        sample
    }

    /// Renders each acquired drawable in one uninterrupted bounded loop.
    /// Every sample belongs to real work submitted for that specific drawable.
    fn calibration_present(&mut self) -> Option<PresentTiming> {
        // No retained image exists before startup or for a zero-width surface.
        if self.next_frame == 0 || self.window.inner_size().width == 0 {
            return None;
        }
        // Metal has latency + 1 drawables. One further acquisition can wait after
        // quick presents fill them; an already occupied queue may block sooner.
        let mut calibration = Calibration::new(self.renderer.maximum_frame_latency());
        #[cfg(all(feature = "lock-metrics", not(target_os = "macos")))]
        let mut trace = Trace::new();
        while calibration.can_present() {
            let (timing, draws) = self.calibration_frame()?;
            calibration.acquired(timing.blocked);
            #[cfg(all(feature = "lock-metrics", not(target_os = "macos")))]
            // Copy scalar witnesses; formatting waits until the whole attempt ends.
            trace.push(
                timing.acquired,
                timing.blocked,
                timing.submitted,
                timing.presented,
                Some(draws),
                self.frame.uploads.len(),
            );
            #[cfg(not(feature = "lock-metrics"))]
            let _ = draws;
            if calibration.qualified() {
                // A qualifying first acquire ends the attempt immediately. No fast
                // subsequent acquire can overwrite the refresh sample we learned.
                tracing::debug!(
                    event = "desktop.calibration_attempt",
                    presents = calibration.presents(),
                    qualified = true
                );
                return Some(timing);
            }
        }
        tracing::debug!(
            event = "desktop.calibration_attempt",
            presents = calibration.presents(),
            qualified = false
        );
        // Exhausting the bound is explicitly no sample, never a synthetic phase.
        // Pacer retries that result at the next fresh idle opportunity.
        None
    }

    /// Acquires and presents real work without returning to the event loop.
    fn calibration_frame(&mut self) -> Option<(PresentTiming, u32)> {
        // A separate retained draw never advances Session damage, the public
        // frame ordinal, or the input recording's parsed-byte witness.
        let (output, acquired, blocked) = self.acquire_surface()?;
        // A view belongs to this acquired drawable; rendering an older view
        // would leave this slot empty and fail to reproduce the FIFO burst.
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        // The retained frame is rendered unchanged: no terminal snapshot,
        // row shaping or glyph preparation is introduced during idle time.
        let rendered = self
            .frame_renderer
            .render_view(&self.renderer, &mut self.frame, &view);
        // A failed render cannot establish a phase sample even if acquire
        // waited: it is reported and a later idle opportunity may retry.
        if let Err(error) = rendered {
            tracing::warn!(event = "desktop.calibration_failed", %error);
            return None;
        }
        #[cfg(all(feature = "lock-metrics", not(target_os = "macos")))]
        let submitted = Instant::now();
        // Present relinquishes this drawable before the next iteration.
        // No timer, readback or event-loop return separates these presents.
        self.renderer.present_surface_texture(output);
        #[cfg(all(feature = "lock-metrics", not(target_os = "macos")))]
        let presented = Instant::now();
        Some((
            PresentTiming {
                acquired,
                blocked,
                #[cfg(all(feature = "lock-metrics", not(target_os = "macos")))]
                work: Default::default(),
                #[cfg(all(feature = "lock-metrics", not(target_os = "macos")))]
                submitted,
                #[cfg(all(feature = "lock-metrics", not(target_os = "macos")))]
                presented,
            },
            rendered.ok()?,
        ))
    }
}
