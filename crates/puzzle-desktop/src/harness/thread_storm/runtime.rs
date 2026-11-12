//! Finite resize/flood harness using the same single-owner channels as desktop (D-50).
//!
//! The simulated display owns its builder, text context and GPU executor. VT
//! belongs exclusively to the worker. Controls have a separate inbox from the
//! four-MiB drain pool, so even saturated output cannot queue ahead of a key.
//! Three slots circulate by move; a refresh waits only within its own period.
//! Skipped publications still transfer overlay views before their slots return.
//! The fake transport timestamps ETX where bytes actually arrive, rather than
//! using the later polling turn as a latency sample. Its observation mutex
//! protects only that diagnostic timestamp, never terminal or frame state.
//! Grid changes coalesce at display opportunities. OS size changes retain the
//! first-event 25-ms throttle; an ordered final publication seals the last grid.
//! The manual controller and fixed 500-us poll remain the original scenario's
//! workload, not new production pacing policy or a throughput-tuned slice.
use super::*;
use puzzle_term::{FrameResult, HostEvent, SessionMessage, frame_builder::FrameBuilder};
/// The pending notification has one owner across timer expiry and final flushing.
mod quiet_resize;
use quiet_resize::QuietResize;

/// Drawing resources have one owner while the worker advances independent VT state.
pub(super) struct Runtime<'a> {
    /// Sending authority grants no reference or synchronous callback to Session.
    pub(super) owner: session_thread::Handle,
    /// This thread exclusively receives publications and returns the three slots.
    pub(super) frames: session_thread::Frames,
    /// Host results are owned values, drained independently of frame drawing.
    pub(super) events: session_thread::Events,
    /// Rebuildable row caches never move back to the parser.
    pub(super) builder: FrameBuilder,
    /// The returned previous hit-table allocation is reused after submission.
    pub(super) regions: puzzle_term::overlay::regions::PresentedRegions,
    /// Latest sealed grid dimensions need no query of the live owner.
    pub(super) grid: (usize, usize),
    /// Independent controller timestamp for the one injected ETX.
    pub(super) key_sent: Option<Instant>,
    /// Retained GPU pipelines match the live window's executor.
    pub(super) executor: &'a mut FrameRenderer,
    /// One device submits every measured frame.
    pub(super) renderer: &'a Renderer,
    /// Attachment survives resizes without texture allocation noise.
    pub(super) view: &'a wgpu::TextureView,
    /// Uploads remain here until the executor actually writes them.
    pub(super) frame: RenderFrame,
    /// Fonts and atlases stay on the frame-building thread (D-30).
    pub(super) context: puzzle_term::frame_builder::context::TextContext,
    /// Fake OS resize calls are observed separately from grid changes.
    pub(super) sizes: Arc<Mutex<Vec<PtySize>>>,
    /// Every finite publication keeps its timing, including obsolete and NothingNew answers.
    pub(super) publications: Vec<session_thread::PublicationTiming>,
    /// Exact transport timestamp, excluding the harness poll after the write.
    pub(super) key: Arc<Mutex<Option<Instant>>>,
}

/// Controls and publications are consumed without acquiring any terminal guard.
impl Runtime<'_> {
    /// Drives one finite phase; final geometry is sealed by an ordered frame reply.
    pub(super) fn drive(&mut self, events: mpsc::Receiver<Control>) -> Result<Metrics, String> {
        // Start with the same immediate first opportunity as the old harness.
        // Later opportunities are measured from actual completion, not inferred FPS.
        let mut next_frame = Instant::now();
        let mut last_frame = None;
        // A resize is host geometry. Its last value wins even while a heavy
        // Unicode or color frame prevents this simulated UI from taking a turn.
        let mut latest = None;
        let mut pty_due = QuietResize::default();
        let mut close = false;
        let mut metrics = Metrics::default();
        // The independent controller owns the three-second workload clock.
        // This loop cannot reduce event counts by slowing its own generator.
        while !close {
            while let Ok(event) = events.try_recv() {
                // Observe all arrivals before scheduling another display opportunity.
                // Close is accumulated so an earlier resize remains visible in totals.
                close |= self.control_event(event, &mut latest, &mut pty_due, &mut metrics)?;
            }
            // Return observational allocations before requesting another snapshot;
            // no effect handler can reach back into the Session from this thread.
            self.host_results();
            if Instant::now() >= next_frame {
                if let Some(size) = latest.take() {
                    // Send the grid change before the frame request on one FIFO sender.
                    // The owner therefore publishes the geometry associated with the drag.
                    self.resize_grid(size)?;
                    metrics.applied += 1;
                }
                if self.present()? {
                    let now = Instant::now();
                    // Only completed GPU submissions enter cadence. Counting requests
                    // would conceal a skipped deadline as a successfully painted frame.
                    if let Some(prior) = last_frame {
                        metrics
                            .frame_intervals
                            .push(now.duration_since(prior).as_secs_f64() * 1000.0);
                    }
                    last_frame = Some(now);
                    metrics.frames += 1;
                }
                next_frame = Instant::now() + REFRESH;
            }
            // Retain the fixture poll: production rendering uses the actor deadline.
            // The ETX sample itself is timestamped on its writer, excluding this sleep.
            thread::sleep(Duration::from_micros(500));
        }
        // The controller can close between two frame ticks. Seal that last resize
        // before comparing VT dimensions against the fake OS transport dimensions.
        self.finish(latest, &mut pty_due, &mut metrics)?;
        Ok(metrics)
    }

    /// FIFO publication is a final geometry barrier without a live terminal query.
    fn finish(
        &mut self,
        latest: Option<[u32; 2]>,
        pty_due: &mut QuietResize,
        metrics: &mut Metrics,
    ) -> Result<(), String> {
        if let Some(size) = latest {
            self.resize_grid(size)?;
            metrics.applied += 1;
        }
        // GridResize must precede PtyResize: the owner derives PTY columns/rows
        // from its grid. Closing between refreshes must not notify the old grid.
        if let Some(size) = pty_due.take(Instant::now(), true) {
            self.owner.send(SessionMessage::PtyResize(size))?;
        }
        // Discard older replies first; the final request follows the final resize on one sender.
        while let Some(mut reply) = self.frames.try_receive() {
            self.publications.push(reply.timing);
            self.builder.accept_snapshot_views(&mut reply.slot);
            self.frames.recycle(reply.slot);
        }
        if let Some(mut reply) = self.request(true)? {
            self.builder.accept_snapshot_views(&mut reply.slot);
            // NothingNew still reports geometry: its free slot may never have
            // carried pixels, so reading dimensions from that allocation is invalid.
            // Source dimensions and its recording byte offset were sealed together
            // by the owner, before any later bytes or pointer commands were applied.
            self.grid = reply.grid;
            tracing::trace!(snapshot_parsed = reply.parsed, "final geometry sealed");
            metrics.bytes = self.owner.parsed_total();
            self.frames.recycle(reply.slot);
        } else {
            return Err("thread-storm final geometry publication missed its deadline".into());
        }
        // These dimensions belong to the ordered final reply, not a synchronous
        // query that could race a later parser update or await an owner lock.
        metrics.grid = self.grid;
        // Preserve unsuccessful and superseded answers in the same denominator.
        // Their missed deadlines must remain visible beside successful presentations.
        metrics.publications = std::mem::take(&mut self.publications);
        // Record the actual transport write, never the later host observation time.
        if let (Some(sent), Some(queued)) =
            (self.key_sent, *self.key.lock().map_err(|e| e.to_string())?)
        {
            metrics.key_ms = queued.saturating_duration_since(sent).as_secs_f64() * 1000.0;
        }
        // This mutex protects only the diagnostic resize journal. No Session
        // or rendering resource is reachable through the transport observation.
        let sizes = self.sizes.lock().map_err(|error| error.to_string())?;
        metrics.pty_calls = sizes.len();
        if let Some(last) = sizes.last() {
            metrics.pty = (usize::from(last.columns), usize::from(last.rows));
        }
        Ok(())
    }

    /// Input sends are nonblocking and retain the independent controller's timestamp.
    fn control_event(
        &mut self,
        event: Control,
        latest: &mut Option<[u32; 2]>,
        pty_due: &mut QuietResize,
        metrics: &mut Metrics,
    ) -> Result<bool, String> {
        match event {
            Control::Resize { size, at } => {
                // This fixture's controller supplies its clock. A slow GPU cannot
                // shift the first-event deadline while sizes accumulate in the inbox.
                if let Some(previous) = pty_due.take(at, false) {
                    // An expired window must seal its grid before notifying the child,
                    // even if no display opportunity consumed that host size yet.
                    self.resize_grid(previous)?;
                    metrics.applied += 1;
                    self.owner.send(SessionMessage::PtyResize(previous))?;
                }
                *latest = Some(size);
                metrics.received += 1;
                pty_due.update(size, at);
            }
            Control::Key(sent) => {
                self.key_sent = Some(sent);
                self.owner.send(SessionMessage::Command(
                    Command::Text("\u{3}".into()),
                    Origin::User,
                ))?;
            }
            Control::Close => return Ok(true),
        }
        Ok(false)
    }

    /// Cell geometry belongs to the host's font resources, not live VT state.
    fn resize_grid(&mut self, size: [u32; 2]) -> Result<(), String> {
        // Metrics are render-side font facts. A grid message copies integers
        // into the owner; it cannot lend a font or shape cache to the parser.
        let grid = grid_for_surface(
            crate::render_thread::surface::Extent {
                width: size[0],
                height: size[1],
            },
            self.context.metrics(),
            16,
        );
        self.owner.send(SessionMessage::GridResize {
            columns: usize::from(grid.columns),
            rows: usize::from(grid.rows),
            surface: size,
        })
    }

    /// Uses the existing harness refresh period as its bounded build opportunity.
    fn request(
        &mut self,
        final_barrier: bool,
    ) -> Result<Option<session_thread::FrameReply>, String> {
        let now = Instant::now();
        // Publication owns no independent timeout: the display supplied this period.
        // The owner records this deadline without delaying a completed late answer.
        let deadline = now + REFRESH;
        let id = self.owner.request_frame(
            now,
            self.context.atlas_generation(),
            self.builder.style_epoch(),
            deadline,
        )?;
        // Receive time is bounded by this already scheduled opportunity.
        // A late session answer cannot extend the display turn indefinitely.
        // Only one candidate is retained; replaced answers immediately return their slot.
        let mut late: Option<session_thread::FrameReply> = None;
        while let Some(mut reply) = self.frames.receive_until(deadline) {
            self.publications.push(reply.timing);
            // Adopt one-time registrations even when the content answer is unchanged.
            self.builder.accept_snapshot_views(&mut reply.slot);
            if reply.id >= id {
                if final_barrier || reply.published {
                    if let Some(older) = late.take() {
                        self.frames.recycle(older.slot);
                    }
                    return Ok(Some(reply));
                }
                self.frames.recycle(reply.slot);
                // An unchanged acknowledgment cannot erase a dirty late publication.
                // Slot flags describe owner damage, not whether this renderer has
                // already submitted the preceding publication's sealed content.
                return Ok(late);
            }
            if final_barrier {
                // Final geometry must follow the final resize rather than an older request.
                self.builder.accept_snapshot_views(&mut reply.slot);
                self.frames.recycle(reply.slot);
            } else {
                // A missed opportunity retains its answer for the following refresh (D-50).
                // Prefer fresh state within the deadline, otherwise draw this completed one.
                if reply.published {
                    if let Some(older) = late.replace(reply) {
                        self.frames.recycle(older.slot);
                    }
                } else {
                    self.frames.recycle(reply.slot);
                }
            }
        }
        Ok(late)
    }

    /// Newest source replaces obsolete slots, but all view registrations are adopted.
    fn present(&mut self) -> Result<bool, String> {
        let Some(mut reply) = self.request(false)? else {
            return Ok(false);
        };
        while let Some(mut newer) = self.frames.try_receive() {
            self.publications.push(newer.timing);
            self.builder.accept_snapshot_views(&mut newer.slot);
            if newer.published {
                // Older registrations were already adopted in arrival order, before newer ones.
                self.frames.recycle(reply.slot);
                reply = newer;
            } else {
                self.frames.recycle(newer.slot);
            }
        }
        // Source dimensions and its recording byte offset were sealed together
        // by the owner, before any later bytes or pointer commands were applied.
        self.grid = reply.grid;
        if !reply.published {
            self.builder.accept_snapshot_views(&mut reply.slot);
            self.frames.recycle(reply.slot);
            return Ok(false);
        }
        let built =
            self.builder
                .build_published(&mut self.context, &mut reply.slot, &mut self.frame);
        // Instances are now owned by RenderFrame; cells can circulate back even
        // while the GPU submission for their completed build has yet to occur.
        self.frames.recycle(reply.slot);
        if let Err(error) = built {
            self.owner
                .send(SessionMessage::FrameResult(FrameResult::BuildFailed))?;
            return Err(error);
        }
        self.executor
            .render_view(self.renderer, &mut self.frame, self.view)
            .map_err(|e| e.to_string())?;
        // Hit geometry becomes authoritative only after successful submission.
        // A failed acquisition would retain the previously presented table.
        self.builder.capture_presented_regions(&mut self.regions);
        self.owner
            .send(SessionMessage::FrameResult(FrameResult::Presented(
                std::mem::take(&mut self.regions),
            )))?;
        Ok(true)
    }

    /// Returned region storage is reused; the finite mock has no foreign UI callbacks.
    fn host_results(&mut self) {
        self.events.begin_drain();
        // Returning the previous hit-table allocation avoids repeated growth;
        // no pointer handling or animation scheduling belongs to the drawing view.
        while let Ok(event) = self.events.try_recv() {
            if let HostEvent::RegionsReturned(regions) = event {
                self.regions = regions;
            }
        }
    }
}
