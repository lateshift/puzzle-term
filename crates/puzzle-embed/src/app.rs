//! Main-thread native embedding owner (D-43, D-50).
//!
//! One App owns the process render actor and all Session/PTY workers. Host calls
//! enqueue values; tick alone delivers host events on the creating thread. A
//! worker may invoke only the supplied wakeup function, never a host UI callback.
//! Shutdown cancels every Session and child before joining rendering, preserving
//! the native surface owners until their in-flight frames have left the actor.
use crate::{
    pipeline::Pipeline,
    render_thread::{RenderThread, SurfaceHandle, host::HostAction, surface::SurfaceId},
    session_thread::Events,
    settings::DesktopSettings,
};
use std::{
    collections::BTreeMap,
    marker::PhantomData,
    rc::Rc,
    sync::{Arc, mpsc},
    thread::{self, ThreadId},
};
/// Checked input capabilities cannot expose a live Session or rendering owner.
mod surface;
pub use surface::{SnapshotError, SnapshotSize, Surface, SurfacePointer};
/// Native cold attachment and its split UI/render ownership precede Session startup.
mod creation;
/// Main-thread delivery routes internal scheduling without a host Session query.
mod events;
/// Cold extension lifecycle is replaceable independently of native ownership.
mod extensions;
/// Shell choice and process environment are explicit, before child creation.
mod options;
/// Cold actor handshake returns copied metrics before PTY startup.
mod preparation;
/// D-60 main-thread CALayer ownership and asynchronous contents acknowledgements.
#[cfg(target_os = "macos")]
pub(crate) mod presentation;
/// Existing bounded drain and ordered writer startup precede Session ownership.
mod pty_bridge;
/// OS read boundaries and initial geometry remain ordered in optional recordings.
mod record;
/// Actor teardown acknowledges when the native target is no longer borrowed.
mod release;
// Optional diagnostic modules do not widen the native host API.
include!("app/optional_items.rs");
/// Clipboard and optional OS effects are queued as owned host transactions.
mod services;
/// Core startup policy has no native toolkit references.
mod session;
/// Native GPU, font, Session and worker assembly preserves the harness startup order.
mod startup;
pub use options::SurfaceOptions;
/// Host byte source selection and checked fixed-pool admission.
pub(crate) mod transport;
pub use transport::TransportMode;

/// Recoverable misuse and platform initialization failures (R8).
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// Every host operation must originate on the App's creating thread.
    #[error("Puzzle host call from the wrong thread")]
    WrongThread,
    /// A closed endpoint cannot address a later surface.
    #[error("Puzzle surface is closed")]
    Closed,
    /// Bad geometry is rejected before GPU or PTY creation.
    #[error("invalid surface geometry: {0}")]
    InvalidArgument(&'static str),
    /// Native failures retain their original diagnostic text.
    #[error("{0}")]
    Platform(String),
}
/// Existing native subsystems use string errors; the API preserves their messages.
impl From<String> for ApiError {
    /// Initialization remains fallible without panicking across a host boundary.
    fn from(value: String) -> Self {
        Self::Platform(value)
    }
}

/// App-wide validated settings and the only callback allowed on worker threads.
pub struct AppConfig {
    /// Settings are already resolved by the native JSON/profile adapter.
    pub settings: DesktopSettings,
    /// Must be nonblocking and thread-safe, usually posting a native event-loop token.
    pub wakeup: Arc<dyn Fn() + Send + Sync>,
}

/// Host actions are values, delivered only by App::tick.
pub enum AppEvent {
    /// Core policy-completed facts and clipboard transactions retain their typed payloads.
    Host(puzzle_term::HostEvent),
    /// Ordered keys, paste and replies for a host-owned byte source.
    TransportInput(Vec<u8>),
    /// Program bytes retain credit until the host reports actual FIFO consumption.
    TransportReply(puzzle_term::platform::reply::ReplyWrite),
    /// Coalesced grid/pixel dimensions for a host-owned source.
    TransportResize(puzzle_term::platform::PtySize),
    /// Core exit/close policy asks the host to release its connection.
    TransportClose,
    /// The render driver has released its native target; the host may destroy its view.
    SurfaceReleased,
    /// Native actions, such as pointer flushing, run on the host thread.
    Render(HostAction),
    /// A validated link requests the host's platform opener.
    OpenLink(String),
    /// Notification policy has already stripped controls and applied its rate limit.
    /// Structured notification lifecycle, delivered only during tick.
    DesktopNotification(crate::NotificationEffect),
    /// Legacy host notification without lifecycle options.
    Notification {
        /// Bounded title chosen by the application behind the PTY.
        title: String,
        /// Bounded notification body.
        body: String,
    },
}

/// Internal queue never contains a native toolkit object or a host callback.
enum Posted {
    /// Render or service effects carry the App's stable surface identity.
    Event(SurfaceId, AppEvent),
    /// A checked endpoint closes asynchronously without borrowing its App.
    Close(SurfaceId),
    /// Shared source/device completion wakes every context user without a new output burst.
    FallbackAll,
    /// Delivered after driver destruction, even though the surface endpoint is invalid.
    Released(SurfaceId),
}
/// Main-thread record of a surface whose mutable owners live on workers.
struct Entry {
    /// CALayer access never leaves the creating main thread.
    #[cfg(target_os = "macos")]
    presentation: Option<presentation::Presentation>,
    /// Session capability is checked before every public host call.
    surface: Surface,
    /// The actor has its own monotonic identity, independent of failed startup attempts.
    render_id: SurfaceId,
    /// The host consumes this receiver only during tick.
    events: Events,
    /// Cancellation and join ownership starts before the surface can be used.
    pipeline: Pipeline,
    /// Native release completes independently of UI callback delivery (D-56).
    released: mpsc::Receiver<()>,
    /// Cold child lifecycle is retired through the distribution hook at tick.
    provisioned: pty_bridge::Provisioned,
}

/// Process rendering and native pipeline lifetime; construct and drop on the host thread.
///
/// The Rc marker prevents safe Rust from moving native teardown to another thread.
/// Surface capabilities remain Send so wrong-thread calls can explicitly return errors.
/// App does not own windows: safe wgpu targets retain their native Arc until teardown.
///
/// ```no_run
/// use puzzle_embed::{App, ApiError, SurfaceOptions};
/// use puzzle_embed::{SurfaceInfo, Visibility};
/// // The host creates its views on its UI thread, then passes owned safe targets.
/// fn attach_two(app: &mut App, first: impl Into<wgpu::SurfaceTarget<'static>>,
///     second: impl Into<wgpu::SurfaceTarget<'static>>, info: SurfaceInfo,
///     visible: Visibility) -> Result<(), ApiError> {
///     let a = app.create_surface(first, info, visible.clone(), SurfaceOptions::default())?;
///     let b = app.create_surface(second, info, visible, SurfaceOptions::default())?;
///     a.focus(true)?;
///     b.focus(false)?;
///     app.tick(|id, event| { /* apply event to the host view identified by id */ })?;
///     Ok(())
/// }
/// ```
pub struct App {
    /// Thread affinity is also checked at runtime for the later C interface.
    owner: ThreadId,
    /// Safe native targets are created from this one process GPU instance.
    instance: wgpu::Instance,
    /// Serial frame building owns all registered native render surfaces.
    render: RenderThread,
    /// One bounded native image worker is shared by every Session in this App.
    decoder: crate::image_decode::worker::Worker,
    /// Live host endpoints, in stable creation order.
    surfaces: BTreeMap<SurfaceId, Entry>,
    /// Distribution-specific cold state never enters the frame/input hot path.
    extensions: extensions::State,
    /// Closed pipelines are cancelled now and joined only at cold App teardown.
    retired: Vec<Pipeline>,
    /// Never-reused identities reject stale endpoints without raw pointers.
    next: u64,
    /// Defaults are cloned only for cold surface creation.
    config: AppConfig,
    /// All workers post owned effects to this queue before calling wakeup.
    posts: mpsc::Sender<Posted>,
    /// This receiver and every host sink belong exclusively to the App thread.
    posted: mpsc::Receiver<Posted>,
    /// AppKit/native-handle destruction cannot move to an arbitrary Rust worker.
    _main_thread: PhantomData<Rc<()>>,
}
impl App {
    /// Starts one rendering actor; no shell or GPU adapter is needed until surface creation.
    pub fn new(config: AppConfig) -> Result<Self, ApiError> {
        let (posts, posted) = mpsc::channel();
        Ok(Self {
            owner: thread::current().id(),
            instance: wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle()),
            render: RenderThread::start()?,
            decoder: crate::image_decode::worker::Worker::start()?,
            surfaces: BTreeMap::new(),
            extensions: extensions::State::default(),
            retired: Vec::new(),
            next: 0,
            config,
            posts,
            posted,
            _main_thread: PhantomData,
        })
    }
    /// Registration transfers cancellation ownership before returning a host endpoint.
    fn register(
        &mut self,
        id: SurfaceId,
        driver: Box<dyn crate::render_thread::Driver>,
        session: crate::session_thread::Handle,
        events: Events,
        pipeline: Pipeline,
        provisioned: pty_bridge::Provisioned,
    ) -> Result<Surface, ApiError> {
        let (driver, released) = release::ReleasedDriver::wrap(
            id,
            driver,
            self.posts.clone(),
            Arc::clone(&self.config.wakeup),
        );
        let (render_id, render) = self.render.add(driver)?;
        let surface = Surface {
            id,
            owner: self.owner,
            valid: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            session,
            feed: pipeline.host_feed(),
            render,
            posts: self.posts.clone(),
            wake: Arc::clone(&self.config.wakeup),
            #[cfg(target_os = "macos")]
            snapshot: Arc::new(std::sync::OnceLock::new()),
        };
        self.surfaces.insert(
            id,
            Entry {
                surface: surface.clone(),
                render_id,
                events,
                pipeline,
                released,
                provisioned,
                #[cfg(target_os = "macos")]
                presentation: None,
            },
        );
        Ok(surface)
    }
    /// Applies a resolved settings snapshot to every live surface and future creation.
    pub fn settings(&mut self, settings: DesktopSettings) -> Result<(), ApiError> {
        self.check_thread()?;
        for entry in self.surfaces.values() {
            entry.surface.settings(settings.clone())?;
        }
        self.config.settings = settings;
        Ok(())
    }
    /// Consumes a never-reused identity even when later surface startup fails.
    fn next_id(&mut self) -> Result<SurfaceId, ApiError> {
        let id = SurfaceId(self.next);
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| ApiError::Platform("surface identity exhausted".into()))?;
        Ok(id)
    }
    /// Rejects a foreign host thread in release as well as debug builds.
    fn check_thread(&self) -> Result<(), ApiError> {
        if thread::current().id() != self.owner {
            return Err(ApiError::WrongThread);
        }
        Ok(())
    }
    /// Closes and waits until the render driver releases its native target (D-56).
    ///
    /// This cold lifecycle barrier is the explicit exception to message-only input.
    /// It waits for the current render turn and joined PTY teardown, never a host
    /// tick or callback: cancellation precedes actor removal. Native reads have
    /// a bounded discard deadline; process signals escalate HUP → TERM → KILL. The host may destroy its
    /// borrowed layer after success. Other calls keep their nonblocking contract.
    pub fn release_surface(&mut self, id: SurfaceId) -> Result<(), ApiError> {
        self.check_thread()?;
        let mut entry = self.surfaces.remove(&id).ok_or(ApiError::Closed)?;
        #[cfg(target_os = "macos")]
        if let Some(presentation) = &entry.presentation {
            presentation.close();
        }
        entry.surface.invalidate();
        entry.pipeline.cancel();
        self.render.remove(entry.render_id);
        // Run signal escalation before waiting for a possibly expensive render
        // turn: the read deadline must not reap the shell before TERM reaches
        // a HUP-ignoring background slave holder. Drivers own sealed snapshots
        // and never need a live Session to complete their native release.
        drop(entry.pipeline);
        entry.released.recv().map_err(|_| {
            ApiError::Platform("render driver disconnected before native release".into())
        })
    }
    /// Cancels immediately without waiting for a frame or a blocked native read.
    pub fn close_surface(&mut self, id: SurfaceId) -> Result<(), ApiError> {
        self.check_thread()?;
        let mut entry = self.surfaces.remove(&id).ok_or(ApiError::Closed)?;
        #[cfg(target_os = "macos")]
        if let Some(presentation) = &entry.presentation {
            presentation.close();
        }
        entry.surface.invalidate();
        // Held EOF and saturated IO wake before the actor releases its native target.
        entry.pipeline.cancel();
        self.render.remove(entry.render_id);
        self.retired.push(entry.pipeline);
        Ok(())
    }
}
impl Drop for App {
    /// Joins all workers after cancellation, retaining host native views through the actor join.
    fn drop(&mut self) {
        for entry in self.surfaces.values_mut() {
            #[cfg(target_os = "macos")]
            if let Some(presentation) = &entry.presentation {
                presentation.close();
            }
            entry.surface.invalidate();
            entry.pipeline.cancel();
        }
        for pipeline in &mut self.retired {
            pipeline.cancel();
        }
        // No Session depends on sender destruction to wake its held-screen wait.
        // Escalation cannot wait behind a render turn for the same reason as
        // release_surface. Sealed publications let rendering finish independently.
        self.surfaces.clear();
        self.retired.clear();
        self.render.shutdown();
    }
}

#[cfg(test)]
/// Host thread affinity, deferred callbacks, stale surfaces and joined shutdown.
mod tests;
