//! Process-owned rendering actor for native hosts (D-41, D-43).
//!
//! The actor owns every registered surface driver and runs them serially, so
//! fonts and GPU resources can be shared without locks. Neither this module
//! nor its surface contract imports winit: the harness and future embedding
//! hosts provide native surfaces and observations at the same boundary.
//! Main-thread input sends control values to the sole owner and never calls a driver.
//! The resource registry belongs to this loop, just like its drivers. A driver
//! takes a moving loan of its text/GPU bundle for one serialized turn, then
//! returns it before the next surface advances. That keeps atlases lock-free
//! while allowing sibling windows to reuse ink uploaded by a preceding frame.
//! Device replacement runs before taking any loan and invalidates every key.
//! Native surface lifetime is independent: its lease posts a cold release when
//! the driver drops, and the registry frees the final user's context on this
//! owner. Startup resource handshakes are the only synchronous host path; keys
//! and geometry continue through the existing nonblocking mailbox endpoints.
//! High-frequency damage and resize changes use one mailbox per surface,
//! rather than growing an output-dependent command queue. The worker sleeps
//! until a deadline or its own coalesced wake; an update cannot race past that sleep.
//! Shutdown order: hosts cancel their Session owners first, then request and
//! join this worker while retaining native window owners. Drivers finish their
//! current turn before dropping; no held Session requires those handles to drop.
//! Wake tokens use a dedicated channel because nested GPU and diagnostic waits
//! may consume the thread-global park token before this outer actor can sleep.
//! On macOS each display's retained link wakes this same independent inbox.
//! Lifecycle removal precedes tick delivery; subscription reconciliation follows
//! drawing, including output posted while a preceding frame was being built.
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
    time::Instant,
};
/// Native refresh subscriptions are process-owned and never contain window pointers.
#[cfg(target_os = "macos")]
mod display;
/// Native-thread notifications and copied visibility observations.
pub mod host;
/// Device loss wakes retain no native-window or Session reference.
pub mod recovery;
/// Render-owned shared GPU and text bundles, with no cross-thread loans.
pub mod resources;
/// Host observations and coalesced per-surface wake state.
pub mod surface;
/// Wait boundary clocks have no scheduling authority.
pub mod wake;
use surface::{Pending, SurfaceId, SurfaceUpdate};

/// One surface's frame machinery, exclusively called on the render thread.
pub trait Driver: Send {
    /// Supplies the host-only mailbox for cancelling obsolete acquisitions during a build.
    fn attach_updates(&mut self, _updates: std::sync::Weak<Mutex<Pending>>) {}
    /// Applies captured host changes and performs work whose deadline has arrived.
    /// The returned deadline is the next useful wake, or None for an idle surface.
    fn advance(&mut self, updates: Pending, now: Instant) -> Option<Instant>;
    /// Product drivers borrow their text resources only during this actor turn.
    fn advance_shared(
        &mut self,
        updates: Pending,
        now: Instant,
        _resources: &mut resources::Resources,
    ) -> Option<Instant> {
        self.advance(updates, now)
    }
    /// Returns a visible display only while this surface needs physical refreshes.
    #[cfg(target_os = "macos")]
    fn refresh_display(&self, _pending: &Pending) -> Option<u32> {
        None
    }
}

/// Cold lifecycle commands; damage never enters this queue.
enum Command {
    /// Cold assembly may read copied metrics before starting the PTY.
    Resources(Box<dyn FnOnce(&mut resources::Resources) + Send>),
    /// Transfers a new driver and its mailbox to the one process worker.
    Add(SurfaceId, Box<dyn Driver>, Arc<Mutex<Pending>>),
    /// Releases only this surface after any current frame finishes.
    Remove(SurfaceId),
    /// Ends the process worker after dropping all remaining drivers.
    Shutdown,
}

/// Input-side endpoint with no access to text contexts, surfaces or frame builders.
#[derive(Clone)]
pub struct SurfaceHandle {
    /// A mailbox guard merges copied host facts and is released before rendering.
    pending: Arc<Mutex<Pending>>,
    /// A dedicated coalesced wake survives nested waits inside a driver.
    wake: mpsc::SyncSender<()>,
    /// Numeric CA geometry forwarding; no native object crosses this endpoint.
    #[cfg(target_os = "macos")]
    presentation: Arc<std::sync::OnceLock<crate::app::presentation::GeometryControl>>,
}

/// Mailbox updates are the sole high-frequency host-to-render operation.
impl SurfaceHandle {
    /// Retains only the latest geometry while unioning damage and earliest timers.
    pub fn update(&self, update: SurfaceUpdate) {
        // Forward numeric scale/rate before the native CA transaction runs.
        // This does not call AppKit or wait for the actor to apply the resize.
        #[cfg(target_os = "macos")]
        if let SurfaceUpdate::Geometry(info) = &update
            && let Some(control) = self.presentation.get()
        {
            control.update(*info);
        }
        // Poisoning is recoverable here: the mailbox contains only owned plain data.
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        // Latest-size replacement bounds live-drag storage independently of event rate.
        pending.merge(update);
        drop(pending);
        // This nonblocking wake forwards host facts without calling the Session owner.
        let _ = self.wake.try_send(());
    }
}

/// Explicit process lifetime; the application creates exactly one of these.
pub struct RenderThread {
    /// Lifecycle changes are rare, unlike PTY damage and pointer samples.
    commands: mpsc::Sender<Command>,
    /// Join occurs after the UI event loop ends, never inside an input callback.
    join: Option<JoinHandle<()>>,
    /// Independent wake channel cannot be consumed by a nested GPU executor.
    wake: mpsc::SyncSender<()>,
    /// IDs are process-local and never reused while a stale host handle may exist.
    next: u64,
}

/// Process lifetime is explicit so embedding hosts can join after native teardown.
impl RenderThread {
    /// Starts the sole worker without creating a window or choosing a toolkit.
    pub fn start() -> Result<Self, String> {
        // Only lifecycle messages allocate channel nodes; damage uses reused mailboxes.
        let (commands, receiver) = mpsc::channel();
        // One pending wake represents all mailbox/lifecycle changes: no payload
        // or tuned timing margin enters this structurally coalesced channel.
        let (wake, wakes) = mpsc::sync_channel(1);
        // Native contexts retain only this process inbox, never a surface handle.
        let source_wake = wake.clone();
        // Thread creation failure propagates before any surface ownership is transferred.
        let join = thread::Builder::new()
            .name("puzzle-render".into())
            .spawn(move || run(receiver, wakes, source_wake))
            .map_err(|error| error.to_string())?;
        // Retain only notification authority; hosts cannot consume wake tokens.
        Ok(Self {
            commands,
            join: Some(join),
            wake,
            next: 0,
        })
    }

    /// Executes cold resource selection on the render owner, never on a Session.
    pub(crate) fn resources<R: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut resources::Resources) -> R + Send + 'static,
    ) -> Result<R, String> {
        let (reply, answer) = mpsc::channel();
        // Only surface creation uses this handshake. Input methods never wait here.
        self.commands
            .send(Command::Resources(Box::new(move |resources| {
                let _ = reply.send(operation(resources));
            })))
            .map_err(|_| "render owner ended during cold resource selection".to_owned())?;
        let _ = self.wake.try_send(());
        answer
            .recv()
            .map_err(|_| "render owner ended before cold resource reply".to_owned())
    }

    /// Retains a context through startup and posts release to its exclusive owner.
    pub(crate) fn lease(&self, key: u64) -> resources::Lease {
        resources::Lease {
            key,
            commands: self.commands.clone(),
            wake: self.wake.clone(),
        }
    }

    /// Moves a host-created surface driver onto the process worker.
    pub fn add(&mut self, driver: Box<dyn Driver>) -> Result<(SurfaceId, SurfaceHandle), String> {
        // A stale handle cannot alias a newly registered surface after close.
        let id = SurfaceId(self.next);
        self.next = self
            .next
            .checked_add(1)
            .ok_or("surface identity exhausted")?;
        // The driver receives this same mailbox before the host can publish updates.
        let pending = Arc::new(Mutex::new(Pending::default()));
        // Sending Add transfers surface ownership; it does not wait for a current frame.
        self.commands
            .send(Command::Add(id, driver, Arc::clone(&pending)))
            .map_err(|_| "render thread ended before surface registration".to_owned())?;
        let _ = self.wake.try_send(());
        Ok((
            id,
            SurfaceHandle {
                pending,
                wake: self.wake.clone(),
                #[cfg(target_os = "macos")]
                presentation: Arc::default(),
            },
        ))
    }

    /// Requests asynchronous surface release; closing a window never waits for a frame.
    pub fn remove(&self, id: SurfaceId) {
        // Removal stays behind the current frame, whose GPU texture still needs its surface.
        let _ = self.commands.send(Command::Remove(id));
        let _ = self.wake.try_send(());
    }

    /// Joins after all host callbacks have stopped and all owned surfaces are released.
    pub fn shutdown(&mut self) {
        // End the receiver loop even if it currently sleeps with no registered windows.
        let _ = self.commands.send(Command::Shutdown);
        let _ = self.wake.try_send(());
        if let Some(join) = self.join.take()
            && join.join().is_err()
        {
            tracing::error!(
                event = "desktop.render_thread_panicked",
                "render worker terminated unexpectedly"
            );
        }
    }
}

/// Error exits follow the same shutdown barrier as normal application exits.
impl Drop for RenderThread {
    /// Startup errors also release the worker, without leaving an orphan thread.
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// One registered surface and its next absolute monotonic deadline.
struct Entry {
    /// Host pacing is a per-surface choice, never a process-wide clock override.
    host_driven: bool,
    /// Only this thread may invoke the driver or touch its rendering resources.
    driver: Box<dyn Driver>,
    /// Mailbox ownership is shared only for small coalesced host facts.
    pending: Arc<Mutex<Pending>>,
    /// None denotes a genuinely idle surface with no recurring work.
    due: Option<Instant>,
}

/// Mailbox collection is separate from frame execution and source reconciliation.
impl Entry {
    /// Copies host facts and this turn's tick, releasing the mailbox before drawing.
    fn take_updates(
        &mut self,
        #[cfg(target_os = "macos")] displays: &display::Displays,
    ) -> Pending {
        let mut updates = {
            // A driver can take arbitrarily long; its producer only waits here.
            let mut pending = self
                .pending
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            std::mem::take(&mut *pending)
        };
        // Keep source selection across empty mailboxes and device/geometry resets.
        self.host_driven |= updates.host_driven;
        let has_updates = !updates.is_empty();
        #[cfg(target_os = "macos")]
        {
            if !self.host_driven
                && let Some(display) = self.driver.refresh_display(&updates)
                && let Some(refresh) = displays.tick(display)
            {
                // All siblings receive the same sealed physical refresh identity.
                updates.merge(SurfaceUpdate::Refresh(refresh));
                #[cfg(feature = "lock-metrics")]
                {
                    updates.display_restart = displays.restart(display);
                }
            }
        }
        // An empty mailbox must stay idle; source state accompanies real turns only.
        updates.host_driven = self.host_driven && (has_updates || self.due.is_some());
        updates
    }
}

/// Services every surface on one worker, sleeping without a recurring polling timer.
fn run(
    commands: mpsc::Receiver<Command>,
    wakes: mpsc::Receiver<()>,
    source_wake: mpsc::SyncSender<()>,
) {
    // Native sources stop before drivers drop. Session cancellation by the host
    // precedes actor shutdown, so neither owner waits for another's destruction.
    let mut resources = resources::Resources::default();
    let mut surfaces = BTreeMap::<SurfaceId, Entry>::new();
    #[cfg(target_os = "macos")]
    let mut displays = display::Displays::new(source_wake);
    #[cfg(not(target_os = "macos"))]
    let _ = source_wake;
    #[cfg(feature = "lock-metrics")]
    let mut last_wake = None;
    loop {
        // Remove closed surfaces before distributing any callback captured earlier.
        if !lifecycle(&commands, &mut surfaces, &mut resources) {
            return;
        }
        #[cfg(target_os = "macos")]
        // Capture each source once, then share that immutable refresh across siblings.
        displays.receive();
        for entry in surfaces.values_mut() {
            let updates = entry.take_updates(
                #[cfg(target_os = "macos")]
                &displays,
            );
            #[cfg(feature = "lock-metrics")]
            let updates = Pending {
                actor_wake: last_wake,
                ..updates
            };
            // Re-read after preceding surfaces, whose complete builds may run long.
            let now = Instant::now();
            if !updates.is_empty() || entry.due.is_some_and(|due| now >= due) {
                entry.due = advance_driver(entry.driver.as_mut(), updates, now, &mut resources);
            }
        }
        #[cfg(target_os = "macos")]
        // Damage posted during a build is included before stopping an idle link.
        displays.reconcile(&surfaces);
        // Only this outer inbox consumes actor wakes, including link callbacks.
        let due = surfaces.values().filter_map(|entry| entry.due).min();
        let waited = wake::receive(&wakes, due);
        // A native tick and a host update use the same preserved coalesced token.
        #[cfg(feature = "lock-metrics")]
        {
            last_wake = Some(waited.sample);
        }
        if !waited.connected {
            return;
        }
    }
}

/// Native temporary resources end before the actor sleeps or advances a sibling.
/// The driver returns only a deadline; persistent resources retain their own native
/// references. Per-surface draining keeps a busy sibling from extending another
/// surface's temporary command/texture ownership across actor turns.
fn advance_driver(
    driver: &mut dyn Driver,
    updates: Pending,
    now: Instant,
    resources: &mut resources::Resources,
) -> Option<Instant> {
    // This worker has no AppKit run loop. Release Metal's temporary native
    // ownership at each frame boundary instead of accumulating it until exit.
    #[cfg(target_os = "macos")]
    {
        puzzle_sys::with_autorelease_pool(|| driver.advance_shared(updates, now, resources))
    }
    #[cfg(not(target_os = "macos"))]
    {
        driver.advance_shared(updates, now, resources)
    }
}

/// Transfers lifecycle ownership between frames, before consuming any link tick.
fn lifecycle(
    commands: &mpsc::Receiver<Command>,
    surfaces: &mut BTreeMap<SurfaceId, Entry>,
    resources: &mut resources::Resources,
) -> bool {
    while let Ok(command) = commands.try_recv() {
        match command {
            Command::Resources(operation) => operation(resources),
            Command::Add(id, mut driver, pending) => {
                // The driver observes crossings queued after this turn's sealed updates.
                driver.attach_updates(Arc::downgrade(&pending));
                surfaces.insert(
                    id,
                    Entry {
                        host_driven: false,
                        driver,
                        pending,
                        due: Some(Instant::now()),
                    },
                );
            }
            Command::Remove(id) => {
                // The callback only holds the process wake, so removal cannot
                // invalidate any native context even if it overlaps that callback.
                surfaces.remove(&id);
            }
            Command::Shutdown => return false,
        }
    }
    true
}

#[cfg(test)]
/// Long-frame input isolation, latest-size coalescing and lifecycle witnesses.
mod tests;

#[cfg(target_os = "macos")]
impl SurfaceHandle {
    /// Install once on the host thread, before CA can call the new layer.
    pub(crate) fn presentation(&self, control: crate::app::presentation::GeometryControl) {
        let _ = self.presentation.set(control);
    }
}
