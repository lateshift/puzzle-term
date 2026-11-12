//! Snapshot-late live frame assembly and Fifo surface presentation (D-33).
//!
//! The Session owner publishes a sealed slot, then shaping and GPU work run
//! on this thread. The parsed-byte count remains paired with the snapshot for
//! private recordings even when the parser advances during surface wait.
//!
//! Instance geometry describes only the latest snapshot. Atlas bytes have a
//! different lifetime: cached handles keep using them after that image is gone.
//! Queue their writes before acquiring a drawable, so Occluded, Timeout and
//! Lost/Outdated reconfiguration cannot strand a prepared glyph in the CPU cache.
//! RenderFrame retains any batch whose writer failed, even through later builds.
//!
//! The optional cold verifier owns a second GPU atlas. It observes the pending
//! batch before the primary executor acknowledges it; otherwise the diagnostic
//! could itself lose glyphs or incorrectly repair the image being investigated.
//! Timing distinguishes build and uploads from FIFO acquisition. Submission
//! and present return are CPU witnesses, not physical display scanout times.
//!
//! Boundary publication happens before a row snapshot obtains shape addresses.
//! Page eviction changes generations before the permanent builder checks row caches.
//! The build cannot recycle a page whose addresses it is already collecting.
//! Worker completions carry owned geometry; snapshot capture never maps a font.
//! Frame observation distinguishes boundary work from snapshot and upload cost.
//! A failed acquisition remains in the attempt denominator even without a present.
//! Covered-glyph latency advances only after a real successful submission.
//! The observer reads final handles rather than treating nonblank tofu as ink.
//! New uploads stay pending when a surface failure leaves them unacknowledged.
//! Atlas pressure can select an oracle bitmap without changing the cell placement.
//! Vector page generations and raster generations invalidate the same row owners.
//! No survivor data is repacked or uploaded when another page is cleared.
//! Glyph counters sample keys before and after a build, not parser scalar count.
//! Upload counts describe queued regions rather than the number of curves.
//! Title effects return to the main thread because AppKit can block.
//! The parsed counter and damaged rows are sealed in one Session-owner turn.
//! A no-damage attempt cannot teach acquisition phase or count as presentation.
//!
//! Three slot allocations circulate with the Session owner. This renderer retains
//! the selected slot through submission and verification, then returns it when
//! replacing that image. Its initial empty placeholder is never added to the pool.
//! A published answer can arrive after its original refresh deadline and still
//! contain the only new ink. A newer NothingNew answer must not discard that ink.
//! Draining therefore chooses the newest dirty publication and recycles unchanged
//! answers separately, adopting every one-time view registration before recycling.
//! Complete reply timings remain observable even for content this surface supersedes.
//! Only the wait for the first answer can block, and its end is this surface's
//! measured build deadline. Remaining already-queued answers drain without waiting.
//! A timed-out opportunity requests the next refresh; it cannot count as a present
//! or force an unbounded owner wait that stalls the process's other surfaces.
//! The cold verifier reads the same retained source, so later parser updates never
//! become a fictitious pixel mismatch with the image that was actually submitted.

use super::verify::{VerifyRequest, VerifyResult};
use super::{LiveWindow, PresentTiming};
use puzzle_term::{FrameResult, SessionMessage};
use std::time::Instant;

/// Row assembly owns its opt-in metrics separately from surface submission.
mod build;
/// IOSurface submission and actual layer-commit acknowledgements (macOS only).
#[cfg(target_os = "macos")]
pub(crate) mod contents;
/// Persistent uploads, drawable acquisition and GPU submission have no Session guard.
mod submit;

impl LiveWindow {
    /// Submits a real retained terminal frame, with surface loss recovery.
    pub fn redraw(&mut self, deadline: Instant) -> Option<PresentTiming> {
        #[cfg(feature = "lock-metrics")]
        let started = Instant::now();
        #[cfg(feature = "lock-metrics")]
        self.frame_metrics.begin();
        let timing = self.redraw_inner(deadline);
        #[cfg(feature = "lock-metrics")]
        self.frame_metrics.finish(
            started,
            timing.as_ref(),
            self.text_context.vector_upload_spans(),
            self.text_context.grid_cache_metrics(),
            [
                self.text_context.atlas_generation(),
                self.text_context.prepared_glyphs() as u64,
                self.text_context.vector_pressure().0,
            ],
            // Native contents may coalesce GPU results. Preserve the ordinal
            // so hardware evidence can assess only images actually assigned.
            timing.as_ref().map(|_| self.next_frame.saturating_sub(1)),
        );
        timing
    }

    /// Executes the ordinary path while its wrapper records unsuccessful attempts too.
    fn redraw_inner(&mut self, deadline: Instant) -> Option<PresentTiming> {
        // Device callbacks can arrive after present, when Session otherwise has no damage.
        // Consume them before requesting a slot so recovery forces a full recapture.
        self.recover_gpu_error();
        // Motion is coalesced at the same display turn as a VT snapshot.
        self.window.flush_pointer();
        if self.verification.due().is_some() {
            // A second frame arrived before an idle check could start.
            // The prior image is no longer the retained warm frame.
            self.verification.frame_started();
            self.verify_info = None;
        }
        tracing::trace!(target: "puzzle_desktop::perf", pending_bytes = self.session.pending_bytes(), parsed_bytes = self.session.parsed_total());
        // The slot is sealed on the owner; no VT reference enters this thread.
        let parsed_at_snapshot = self.begin_snapshot(deadline)?;
        #[cfg(feature = "lock-metrics")]
        self.frame_metrics.build_started();
        let work = self.build_rows()?;
        let timing = self.present_frame(parsed_at_snapshot);
        if timing.is_some() {
            self.frame_demand.presented(parsed_at_snapshot);
            // IOSurface submission is not a contents commit. Its region table
            // reaches Session only after the main-thread layer assigns that image.
            #[cfg(target_os = "macos")]
            if self.presentation.is_some() {
                self.finish_contents(parsed_at_snapshot);
            } else {
                self.finish_regions();
            }
            #[cfg(not(target_os = "macos"))]
            self.finish_regions();
        } else {
            self.send(SessionMessage::FrameResult(FrameResult::Skipped));
        }
        #[cfg(feature = "lock-metrics")]
        if timing.is_some() {
            // Only actual queue submission completes a covered appearance's time-to-ink.
            self.ink_observer.presented(Instant::now());
        }
        #[cfg(feature = "lock-metrics")]
        let timing = timing.map(|mut timing| {
            timing.work.build = work.build_cpu;
            timing.work.build_phases = work.build_phases;
            timing.work.rebuilt_rows = work.rebuilt_rows;
            timing.work.row_phases = work.row_phases;
            timing.work.cache_metrics = self.text_context.cache_metrics();
            timing.work.glyphs = self
                .text_context
                .prepared_glyphs()
                .saturating_sub(work.glyphs_before);
            timing.work.uploads = work.upload_count;
            timing
        });
        // Debug comparison is intentionally after presentation: it cannot
        // delay the frame being diagnosed or alter its recorded byte count.
        if timing.is_some() {
            self.queue_verification(work.blink);
        }
        timing
    }

    /// Drawable hosts acknowledge immediately; IOSurface hosts do so from CA.
    fn finish_regions(&mut self) {
        // A failed acquisition emits no completed-frame event and cannot feed the HUD.
        // Send bounded counters before the presented regions in the same owner order.
        // The Session observer updates data without creating an idle redraw loop.
        if let Some(stats) = self.builder.take_frame_statistics() {
            self.send(SessionMessage::FrameResult(FrameResult::Statistics(stats)));
        }
        self.builder.capture_presented_regions(&mut self.regions);
        let regions = std::mem::take(&mut self.regions);
        self.send(SessionMessage::FrameResult(FrameResult::Presented(regions)));
    }

    /// Defers expensive readback until the successfully presented image is idle.
    fn queue_verification(&mut self, blink: bool) {
        if self.verifier.is_none() {
            return;
        }
        // Readback can exceed a refresh; the live frame never waits for its check.
        self.verification.presented(Instant::now());
        self.verify_info = Some(VerifyRequest {
            blink,
            focused: self.focused,
            dpr: self.window.scale_factor() as f32,
            font_size: self.text_context.font_size(),
            number: self.next_frame.saturating_sub(1),
        });
    }

    /// Mirrors pending bytes before the real executor acknowledges GPU ownership.
    fn observe_glyph_uploads(&mut self) {
        // Trace ownership transfer before a surface failure could abandon these spans.
        tracing::trace!(target: "puzzle_desktop::glyphs", frame = self.next_frame,
            glyphs = self.text_context.prepared_glyphs(), uploads = ?self.frame.uploads.iter().map(|u| match u {
                puzzle_term::frame::Upload::Curves { origin, texels } => ("curves", *origin, texels.len()),
                puzzle_term::frame::Upload::Bands { origin, texels } => ("bands", *origin, texels.len()),
                _ => ("other", [0,0], 0),
            }).collect::<Vec<_>>(), "built glyph upload spans");
        // The optional mirror follows writes, including images with no drawable.
        // Observe before the real writer acknowledges and clears this batch.
        if let Some(verifier) = self.verifier.as_mut()
            && let Err(error) = verifier.observe_uploads(&self.renderer, &self.frame)
        {
            tracing::error!(event = "desktop.frame_verify_failed", %error);
        }
    }

    /// Runs a retained-frame comparison only after output has settled.
    pub fn verify_idle(&mut self, now: Instant) {
        if !self.verification.ready(now, None) {
            return;
        }
        let skipped = self.verification.finish();
        let Some(request) = self.verify_info.take() else {
            return;
        };
        if let Some(verifier) = self.verifier.as_mut() {
            match verifier.check(
                &self.renderer,
                &mut self.frame,
                &self.snapshot,
                request,
                &self.fallback_families,
            ) {
                Ok(VerifyResult::Match) => {}
                Ok(VerifyResult::Mismatch(error)) => tracing::error!(
                    event = "desktop.frame_mismatch", %error, "warm and cold frames differ"
                ),
                Err(error) => tracing::error!(
                    event = "desktop.frame_verify_failed", %error,
                    "frame verification could not be completed"
                ),
            }
        }
        tracing::info!(target: "puzzle_desktop::frames", frame = request.number,
            skipped, "idle frame verification completed");
    }

    /// Gives the host the next optional idle-check deadline.
    pub fn verify_deadline(&self) -> Option<Instant> {
        self.verification.due()
    }

    /// Requests a sealed slot and drains answers without losing an earlier dirty one.
    /// Current request clocks stay distinct from those of the reply actually drawn.
    fn begin_snapshot(&mut self, deadline: Instant) -> Option<u64> {
        #[cfg(feature = "lock-metrics")]
        let boundary_started = Instant::now();
        let generation = self.prepare_text_frame()?;
        #[cfg(feature = "lock-metrics")]
        self.frame_metrics.boundary(boundary_started.elapsed());
        let started = Instant::now();
        let id = match self.session.request_frame(
            started,
            generation,
            self.builder.style_epoch(),
            deadline,
        ) {
            Ok(id) => id,
            Err(error) => {
                tracing::warn!(event = "desktop.snapshot_failed", %error);
                return None;
            }
        };
        self.frame_demand.requested(id);
        tracing::trace!(target: "puzzle_desktop::scheduling", event="desktop.snapshot_request",
            id, wait_us=deadline.saturating_duration_since(started).as_secs_f64()*1e6,
            pending_bytes=self.session.pending_bytes());
        #[cfg(feature = "lock-metrics")]
        self.frame_metrics.requested(started, deadline, id);
        // An unchanged answer retires nonvisual source bytes but cannot retire
        // a dirty publication still waiting for submission. Older replies likewise
        // cannot acknowledge the newer request whose wait this turn is bounding.
        let (newest, answered) = self.frames.newest_until(id, deadline, |answer| {
            self.frame_demand
                .answered(answer.id, answer.published, answer.parsed);
            answer.timing.trace_reply(answer.id, id, answer.published);
            #[cfg(feature = "lock-metrics")]
            self.publications.push(answer.timing);
            // Even superseded publications carry one-time view ownership. Idle
            // acknowledgements carry only recycled storage, never source removals.
            answer.accept_views(&mut self.builder);
        });
        let Some(answer) = newest else {
            tracing::trace!(target: "puzzle_desktop::scheduling", event="desktop.snapshot_empty",
                id, answered, pending_bytes=self.session.pending_bytes());
            // Owner publication rearmed its wake; a timeout still needs the next refresh.
            if !answered {
                self.window.request_redraw();
            }
            return None;
        };
        Some(self.adopt_snapshot(answer, started))
    }

    /// Replaces the retained source only after selecting a complete publication.
    /// Returning its predecessor restores pool capacity without another owner wait.
    /// Recording uses the selected answer's byte offset, not current parser progress.
    fn adopt_snapshot(
        &mut self,
        answer: crate::session_thread::FrameReply,
        started: Instant,
    ) -> u64 {
        self.snapshot_done = Instant::now();
        #[cfg(feature = "lock-metrics")]
        self.frame_metrics
            .received(self.snapshot_done, answer.timing);

        // A reply from the preceding refresh can be received immediately. Its
        // actual owner latency still belongs in D-33's complete-work prediction.
        self.snapshot_wait_and_latency = [started.elapsed(), answer.timing.request_wall()];
        let old = std::mem::replace(&mut self.snapshot, answer.slot);
        if self.has_snapshot {
            self.frames.recycle(old);
        }
        self.has_snapshot = true;
        #[cfg(feature = "lock-metrics")]
        self.frame_metrics.snapshot(
            started.elapsed(),
            [
                self.snapshot.columns(),
                self.snapshot.rows(),
                self.snapshot.copied_count(),
            ],
        );
        self.record_snapshot(answer.parsed);
        answer.parsed
    }

    /// Completes text-store eviction before the Session damage snapshot is taken.
    fn prepare_text_frame(&mut self) -> Option<u64> {
        // Report failed eviction without replacing the previous visible frame.
        if let Err(error) = self.text_context.begin_frame() {
            #[cfg(feature = "lock-metrics")]
            self.frame_metrics.gate("boundary_failed");
            tracing::warn!(event = "font.paint_eviction_failed", %error);
            return None;
        }
        Some(self.text_context.atlas_generation())
    }

    /// Records the frozen byte offset before the parser can resume.
    fn record_snapshot(&self, parsed_at_snapshot: u64) {
        // A later frame record refers to this earlier offset even if
        // another PTY read arrives during the GPU surface wait.
        if let Some(recorder) = &self.recorder
            && let Err(error) = recorder.emit(
                "snapshot",
                serde_json::json!({"number": self.next_frame, "parsed": parsed_at_snapshot}),
            )
        {
            tracing::warn!(event = "desktop.record_failed", %error);
        }
    }
}
