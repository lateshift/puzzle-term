//! Main-thread IOSurface integration shared by winit and C native-layer hosts.
//!
//! Only CALayer.display may wait for changed geometry. GPU completion posts a
//! wake; App::tick commits ordinary frames and emits firstFrame/cursorRect only
//! after an actual contents assignment. Closed identities discard late receipts.
//!
//! Render ordinals are unrelated to the source display-link counters. Moving
//! between displays or taking over host pacing cannot turn a new frame into a
//! duplicate contents notification. CA reports the exact ordinal it assigned.
//! Metadata is sent before the completion fence is committed, so an eager GPU
//! can never present an image before its cursor and hit regions are queued.
//!
//! The main-thread submission ledger retains only outstanding image metadata.
//! When CA coalesces completed frames, earlier region allocations return to the
//! renderer without ever becoming the input model's presented region table.
//! Statistics and hit regions for the selected image enter the Session control
//! channel in order, while firstFrame/cursorRect go to the host's tick sink.
//!
//! Geometry controls carry numbers and an atomic mailbox handle only. Surface
//! clones remain Send and reject wrong-thread API calls as before; they never
//! contain an Objective-C layer. The native Host remains in App's non-Send entry.
//! It is invalidated before worker cancellation, preventing a layer retained by
//! an enclosing CA transaction from asking a closed rendering actor for work.
use super::*;
/// Cold theme resolution uses the existing registry; live colours come from frames.
mod background;
/// Private delivery counters distinguish missing images from undrained host facts.
mod caret;
use crate::render_thread::surface::{Extent, SurfaceInfo, SurfaceUpdate};
/// Cold attachment must paint its backing before any image can be presented.
pub(super) use background::initial as initial_background;
use puzzle_sys::presentation::{Control, Geometry, Host, Receipt};
use puzzle_term::{FrameResult, SessionMessage, overlay::regions::PresentedRegions};
use std::{
    collections::VecDeque,
    sync::{Mutex, OnceLock},
    time::Instant,
};

/// Numeric control contains no native layer reference and is safe in SurfaceHandle.
pub(crate) struct GeometryControl {
    /// Source identity/rate must survive a CA callback preceding the next event-loop tick.
    info: Arc<Mutex<SurfaceInfo>>,
    /// Atomic scale/rate hints; setting them performs no Objective-C calls.
    control: Control,
    /// Geometry invalidates only old pending frame metadata, never glyph caches.
    submissions: std::sync::Weak<Submissions>,
}
impl GeometryControl {
    /// Ordinary host geometry forwarding occurs before the next CA transaction.
    pub(crate) fn update(&self, info: SurfaceInfo) {
        *self.info.lock().unwrap_or_else(|e| e.into_inner()) = info;
        // Publish desired pixels before recycling their metadata: GPU completion
        // can race this call while CA still has the preceding host's bounds.
        self.control.resize(
            Geometry {
                size: [info.extent.width, info.extent.height],
                scale: info.scale,
            },
            info.millihertz,
        );
        if let Some(submissions) = self.submissions.upgrade() {
            submissions.geometry(Geometry {
                size: [info.extent.width, info.extent.height],
                scale: info.scale,
            });
        }
    }
}
/// Frame-local hit regions and caret refer to the image, never later parser state.
pub(crate) struct Submission {
    /// Render ordinal, distinct from any display-link namespace.
    pub serial: u64,
    /// Sealed source offset travels with the pixels, never the live parser.
    pub parsed: u64,
    /// Opt-in recording is emitted only for an actual contents assignment.
    pub recorder: Option<crate::recording::Recorder>,
    /// Wrong-size images cannot satisfy a later bounds transaction.
    pub geometry: Geometry,
    /// Top-left logical points already include padding and viewport position.
    pub cursor: Option<[f32; 4]>,
    /// Region table is installed on Session only when this image is committed.
    pub regions: PresentedRegions,
    /// HUD facts describe only successful presented frames.
    pub statistics: Option<puzzle_term::messages::FrameStats>,
}
/// Native objects stay in App's main-thread entry, never Surface capabilities.
pub(super) struct Presentation {
    /// Owned subclass invalidates callbacks before worker shutdown.
    host: Host,
    /// The same bounded ledger observes emission and tick delivery without Session access.
    submissions: Arc<Submissions>,
}
/// Presented metadata can be consumed directly by CA even inside event tracking.
struct Delivery {
    /// Storage is bounded by the native image pool; obsolete geometry is recycled.
    submissions: VecDeque<Submission>,
    /// Installed after Session registration, before any native callback can run.
    endpoint: Arc<OnceLock<SurfaceHandle>>,
    /// Session messages are nonblocking; no callback enters Session state.
    session: Option<crate::session_thread::Handle>,
    /// Host presentation events still enter the App tick queue.
    posts: mpsc::Sender<Posted>,
    /// A commit during tick must schedule another drain for its newly queued effects.
    wake: Arc<dyn Fn() + Send + Sync>,
    /// Stable App identity rejects effects after close.
    id: SurfaceId,
    /// One first-frame notification per surface lifetime.
    first: bool,
    /// Geometry changes refresh even an equal logical caret rectangle.
    cursor: Option<(Geometry, Option<[f32; 4]>)>,
    /// Change-only notification totals never participate in redraw scheduling.
    carets: caret::Facts,
    /// Native display callbacks may run outside winit's own event dispatch.
    perf: Option<crate::window::perf::Counters>,
}
/// Short guards move owned region tables; they never cover native or GPU work.
pub(crate) struct Submissions(Mutex<Delivery>);
impl Submissions {
    /// GPU coalescing releases metadata even when native tracking prevents tick.
    pub(crate) fn retire(&self, serial: u64) {
        let mut delivery = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(index) = delivery
            .submissions
            .iter()
            .position(|item| item.serial == serial)
            && let Some(old) = delivery.submissions.remove(index)
        {
            // This ordinal was superseded before main acquired it. Never retire
            // an independently acquired image which CA is about to commit.
            delivery.recycle(old.regions);
        }
    }
    /// Metadata is queued before the completion that makes its pixels eligible.
    pub(crate) fn submit(&self, frame: Option<Submission>) {
        // The native image pool bounds outstanding submissions. Moving the region
        // table into preallocated ledger storage does not copy its primitives.
        let mut delivery = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(frame) = frame {
            delivery.submissions.push_back(frame);
        } else {
            // A device loss or failed completion fence cannot later produce a
            // legitimate presentation. Return its region allocations for reuse.
            delivery.prune(None);
        }
    }
    /// A new bounds transaction can never commit an old-geometry image.
    fn geometry(&self, geometry: Geometry) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .prune(Some(geometry));
    }
    /// Called only after actual CA assignment, including inside a native modal loop.
    fn presented(&self, receipt: Receipt) {
        if let Some(elapsed) = receipt.main_busy {
            // An activity sample has no pixels or hit regions to acknowledge.
            // CA can enter independently of winit while AppKit tracks a drag.
            // Keep only its residence time, never the gap before this callback.
            // Nested winit/CA samples use maxima, so their durations are not summed.
            if let Some(perf) = &self.0.lock().unwrap_or_else(|e| e.into_inner()).perf {
                perf.busy(elapsed);
            }
            return;
        }
        tracing::trace!(target:"puzzle_desktop::resize",event="embed.contents_commit",
            serial=?receipt.serial,requested=?receipt.requested,committed=?receipt.committed,
            wait_us=receipt.waited.as_secs_f64()*1e6,timed_out=receipt.timed_out);
        // A timeout reports retained pixels; it is not a presentation and must
        // not invent a caret event or install an unpresented region table.
        if let Some(serial) = receipt.serial {
            let wake = {
                let mut delivery = self.0.lock().unwrap_or_else(|e| e.into_inner());
                delivery
                    .presented(serial, receipt.requested)
                    .then(|| Arc::clone(&delivery.wake))
            };
            // A host wake is foreign code. Never call it with the ledger guard
            // held, including when commit happened inside an existing App tick.
            if let Some(wake) = wake {
                wake();
            }
        }
    }
}
/// Cold attachment is completed before any native callback can enter the event loop.
pub(super) struct Attachment {
    /// Main-only ownership remains in App after registration.
    presentation: Presentation,
    /// Request callback obtains the send-only handle once registration succeeds.
    endpoint: Arc<OnceLock<SurfaceHandle>>,
    /// Latest host display observations are shared as plain numeric data only.
    info: Arc<Mutex<SurfaceInfo>>,
    /// Shared metadata carries no native object and never retains the App.
    submissions: Arc<Submissions>,
}
impl Attachment {
    /// The renderer shares only metadata, never this main-thread native host.
    pub(super) fn submissions(&self) -> Arc<Submissions> {
        Arc::clone(&self.submissions)
    }
    /// Installs a physically sized child without changing host layer identity (kind 5).
    pub(super) fn new(
        app: &App,
        id: SurfaceId,
        surface: &wgpu::Surface<'_>,
        info: SurfaceInfo,
        latency: u32,
        background: [f32; 4],
    ) -> Result<(Self, crate::window::live::presentation::contents::Native), ApiError> {
        let endpoint = Arc::new(OnceLock::<SurfaceHandle>::new());
        let facts = Arc::new(Mutex::new(info));
        let submissions = Arc::new(Submissions(Mutex::new(Delivery {
            submissions: VecDeque::with_capacity(latency as usize + 1),
            endpoint: Arc::clone(&endpoint),
            session: None,
            posts: app.posts.clone(),
            wake: Arc::clone(&app.config.wakeup),
            id,
            first: true,
            cursor: None,
            carets: caret::Facts::default(),
            perf: None,
        })));
        let request = request(
            Arc::clone(&endpoint),
            Arc::clone(&facts),
            Arc::clone(&submissions),
        );
        // The CA callback consumes metadata directly during native event tracking.
        // Requiring a future App::tick here would defeat synchronous resizing.
        let observed = Arc::clone(&submissions);
        let observe = Arc::new(move |receipt| observed.presented(receipt));
        // Existing native latency = 2; the pool derives its front/in-flight capacity.
        let (host, target) = Host::attach(
            surface,
            info.scale,
            info.millihertz,
            latency,
            request,
            Arc::clone(&app.config.wakeup),
            observe,
        )?;
        // Creation geometry is authoritative too, before any later resize call.
        // A zero extent stays inert until the host supplies its first real size.
        // Its wake schedules a later tick: install completes the callback endpoint
        // before this main-thread creation call returns to the host event loop.
        host.control().resize(
            Geometry {
                size: [info.extent.width, info.extent.height],
                scale: info.scale,
            },
            info.millihertz,
        );
        host.background(background);
        Ok((
            Self {
                presentation: Presentation {
                    host,
                    submissions: Arc::clone(&submissions),
                },
                endpoint,
                info: facts,
                submissions,
            },
            crate::window::live::presentation::contents::Native::new(target),
        ))
    }
    /// Fill the nonblocking request endpoint before returning the surface to its host.
    /// The actor can already queue metadata while registration returns. Main cannot
    /// enter a CA display callback until this creation call returns to AppKit, so
    /// both the Session capability and geometry endpoint are installed first.
    /// No synchronous handshake with the actor is needed for this final wiring.
    /// Closed capabilities retain only numeric controls; the weak ledger edge
    /// prevents the callback endpoint from extending its own native lifetime.
    pub(super) fn install(self, entry: &mut Entry, perf: crate::window::perf::Counters) {
        // The returned Surface and App entry share this once-installed capability.
        // It contains no CALayer and is published only by actual main-thread commits.
        let _ = entry
            .surface
            .snapshot
            .set(self.presentation.host.snapshot());
        // The endpoint must not own its ledger strongly: the ledger also keeps
        // this endpoint for recycling. A weak edge breaks that ownership cycle.
        entry.surface.render.presentation(GeometryControl {
            info: self.info,
            control: self.presentation.host.control(),
            submissions: Arc::downgrade(&self.submissions),
        });
        // Registration is finished before the host can commit this layer. The
        // Session capability only sends values; it cannot synchronously query VT.
        self.submissions
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .session = Some(entry.surface.session.clone());
        self.submissions
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .perf = Some(perf);
        let _ = self.endpoint.set(entry.surface.render.clone());
        entry.presentation = Some(self.presentation);
    }
}
/// CA forwards the authoritative host pixel key; it never queries Session.
fn request(
    endpoint: Arc<OnceLock<SurfaceHandle>>,
    info: Arc<Mutex<SurfaceInfo>>,
    submissions: Arc<Submissions>,
) -> puzzle_sys::presentation::Request {
    Arc::new(move |geometry: Geometry, deadline: Instant| {
        submissions.geometry(geometry);
        if let Some(render) = endpoint.get() {
            let mut facts = *info.lock().unwrap_or_else(|e| e.into_inner());
            facts.extent = Extent {
                width: geometry.size[0],
                height: geometry.size[1],
            };
            facts.scale = geometry.scale;
            render.update(SurfaceUpdate::GeometryFrame(facts, deadline));
        }
    })
}
impl Presentation {
    /// Record only facts actually handed to the host sink, never merely queued.
    pub(super) fn caret_delivered(&self, rect: Option<[f32; 4]>) {
        let mut delivery = self.submissions.0.lock().unwrap_or_else(|e| e.into_inner());
        let id = delivery.id;
        delivery.carets.delivered(id, rect);
    }
    /// GPU wake or ordinary App tick may adopt ready content, always without a wait.
    pub(super) fn flush(&self) {
        self.host.flush();
    }
    /// Cancel a potential CA wait before joining a render or Session worker.
    pub(super) fn close(&self) {
        self.host.close();
    }
}
impl Delivery {
    /// Recycle superseded native geometry without ever exposing its hit regions.
    fn prune(&mut self, geometry: Option<Geometry>) {
        // A geometry change can occur repeatedly without an event-loop tick.
        // Retire each obsolete table now rather than retaining one per drag step.
        // This never invalidates text contexts, atlases or negative glyph caches.
        let mut index = 0;
        while index < self.submissions.len() {
            if geometry.is_some_and(|g| self.submissions[index].geometry == g) {
                index += 1;
            } else if let Some(old) = self.submissions.remove(index) {
                self.recycle(old.regions);
            }
        }
    }
    /// Returning storage is a mailbox update, never a call into the frame builder.
    fn recycle(&self, regions: PresentedRegions) {
        if let Some(render) = self.endpoint.get() {
            render.update(SurfaceUpdate::Regions(regions));
        }
    }
    /// Report only the image that became contents; coalesced images stay inert.
    fn presented(&mut self, serial: u64, geometry: Geometry) -> bool {
        let mut posted = false;
        while self
            .submissions
            .front()
            .is_some_and(|item| item.serial <= serial)
        {
            let Some(frame) = self.submissions.pop_front() else {
                break;
            };
            // Core Animation may coalesce ready images. Only the selected
            // ordinal may become the Session's presented hit-test model.
            if frame.serial != serial {
                self.recycle(frame.regions);
                continue;
            }
            // Sealed source offsets, caret and regions stay paired even if the
            // parser advanced many slices while this GPU frame was in flight.
            record(&frame);
            if let Some(session) = &self.session {
                if let Some(stats) = frame.statistics {
                    let _ =
                        session.send(SessionMessage::FrameResult(FrameResult::Statistics(stats)));
                }
                let _ = session.send(SessionMessage::FrameResult(FrameResult::Presented(
                    frame.regions,
                )));
            }
            // These are queue posts, not host callbacks. Even a resize callback
            // executing inside AppKit tracking preserves tick-only delivery.
            if self.first {
                self.first = false;
                posted |= self.post(HostAction::FirstFrame);
            }
            // A scale/display geometry change refreshes native IME placement
            // even if the logical caret rectangle happened to remain equal.
            let cursor = (geometry, frame.cursor);
            if self.cursor != Some(cursor) {
                self.cursor = Some(cursor);
                // Count queue admission, not merely the change-detection decision.
                let queued = self.post(HostAction::CursorRect(frame.cursor));
                self.carets.emitted(self.id, serial, frame.cursor, queued);
                posted |= queued;
            }
        }
        posted
    }
    /// Delivery still happens exclusively when the native host drains App::tick.
    fn post(&self, action: HostAction) -> bool {
        self.posts
            .send(Posted::Event(self.id, AppEvent::Render(action)))
            .is_ok()
    }
}

/// Coalesced native contents never acknowledge unpresented UI state.
#[cfg(test)]
mod tests;

/// A recording's frame boundary describes only the image actually made visible.
fn record(frame: &Submission) {
    // This private diagnostic target is normally disabled. Capture the real CA
    // assignment clock, not GPU submission, without recording all PTY bytes.
    if tracing::enabled!(target: "puzzle_desktop::frames", tracing::Level::DEBUG) {
        let unix_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |time| time.as_nanos());
        tracing::debug!(target:"puzzle_desktop::frames",event="Presented",
            parsed_bytes=frame.parsed,serial=frame.serial,unix_ns,
            size=?frame.geometry.size,scale=frame.geometry.scale);
    }
    if let Some(recorder) = &frame.recorder
        && let Err(error) = recorder.frame(frame.serial, frame.parsed)
    {
        tracing::warn!(event="desktop.record_failed",%error);
    }
}
