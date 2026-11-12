//! CoreVideo ownership and callback glue under the narrow D-53/R8 exception.
//!
//! Native creation transfers a retained object into a permanent reference. There
//! is intentionally no release call: stop only asks the OS worker to wind down.
//! The raw callback context is a leaked Box, with one allocation per display,
//! containing immutable fanout snapshots and atomics, never a window pointer.
//! Registry methods serialize native controls across render actors. The callback never
//! calls these methods and cannot acquire an actor/registry lock.
use super::{ActorId, LinkError, Refresh, Wake, signal::Signal};
use objc2_core_video::{
    CVDisplayLink, CVGetCurrentHostTime, CVGetHostClockFrequency, CVOptionFlags, CVReturn,
    CVTimeStamp, CVTimeStampFlags,
};
use std::{
    collections::BTreeMap,
    ffi::c_void,
    ptr::NonNull,
    sync::Arc,
    time::{Duration, Instant},
};

/// Owner-only native controls; only the stable Signal crosses into the callback.
pub(super) struct Entry {
    /// Creation's retained reference is never released, even on registry drop.
    link: usize,
    /// Stable context outlives both the native worker and the process actor.
    signal: &'static Signal,
    /// Registration failures are retained too, preventing repeated native leaks.
    registration: CVReturn,
    /// Each App contributes one wake and an independent clock-consumption cursor.
    subscribers: BTreeMap<ActorId, Subscriber>,
    /// Actual native running state survives a failed start for a later retry.
    running: bool,
    /// Each zero-to-active edge invalidates ticks from the preceding subscription.
    generation: u64,
    /// Exact start-call entry, reported once to each actor present at restart.
    started: Option<Instant>,
}

/// One App's membership cannot consume or remove another App's clock facts.
struct Subscriber {
    /// Send-only actor inbox capability, copied into the callback snapshot.
    wake: Wake,
    /// Only this actor advances this cursor when it consumes a refresh.
    seen: u64,
    /// Restart observations are independently delivered once to each reader.
    restart_generation: u64,
}

/// Creating and attaching is cold; start/stop never run on the native callback.
impl Entry {
    /// Allocates the sole native object/context pair for the requested display.
    #[allow(
        unsafe_code,
        reason = "R8/D-53 permits audited display-link glue in puzzle-sys"
    )]
    pub(super) fn new(display: u32) -> Result<Self, LinkError> {
        let mut raw = std::ptr::null_mut();
        // SAFETY: CoreVideo writes one retained pointer into a live stack local;
        // the display is only an OS identifier and no callback exists yet.
        let status =
            unsafe { CVDisplayLink::create_with_cg_display(display, NonNull::from(&mut raw)) };
        // A rejected display has no transferred object or callback context.
        success(status)?;
        Self::attach(raw)
    }
    /// Registers the callback on a pointer supplied only by native creation.
    #[allow(
        unsafe_code,
        reason = "R8/D-53 permits registration on retained native objects"
    )]
    fn attach(raw: *mut CVDisplayLink) -> Result<Self, LinkError> {
        let raw = NonNull::new(raw).ok_or(LinkError::Missing)?;
        // SAFETY: Creation returned a live retained object. It is never released,
        // so the immutable reference remains valid for the process lifetime.
        let link = unsafe { raw.as_ref() };
        let signal = Box::leak(Box::new(Signal::empty()));
        // SAFETY: The callback receives only this permanent, initialized context.
        // Native timestamps are read only during the callback; no window is owned.
        let status =
            unsafe { link.set_output_callback(Some(output), (signal as *mut Signal).cast()) };
        // Store the retained address rather than asserting Send on a foreign type.
        // Only serialized registry controls reconstruct its native reference.
        Ok(Self {
            link: raw.as_ptr() as usize,
            registration: status,
            signal,
            subscribers: BTreeMap::new(),
            running: false,
            generation: 0,
            started: None,
        })
    }
    /// The native lifecycle witness discovers a real display without another dependency.
    #[cfg(test)]
    #[allow(
        unsafe_code,
        reason = "R8/D-53 permits native active-display creation in its lifecycle test"
    )]
    pub(super) fn active_for_test() -> Result<(u32, Self), LinkError> {
        let mut raw = std::ptr::null_mut();
        // SAFETY: The out pointer addresses a live stack local. No callback is
        // installed until attach permanently retains both native object and context.
        let status =
            unsafe { CVDisplayLink::create_with_active_cg_displays(NonNull::from(&mut raw)) };
        success(status)?;
        let entry = Self::attach(raw)?;
        Ok((entry.native().current_cg_display(), entry))
    }
    /// Changes only this App's membership, then reconciles aggregate native demand.
    pub(super) fn subscribe(
        &mut self,
        actor: ActorId,
        count: usize,
        wake: Wake,
    ) -> Result<(), LinkError> {
        // A failed callback registration cannot service any actor; retain its
        // original status instead of creating a second native object on retry.
        success(self.registration)?;
        // A facade's local surface count matters only as zero/nonzero here.
        // Repeated active updates must not allocate a new fanout every frame.
        let before = self.subscribers.len();
        if count == 0 {
            // Removing a missing actor is harmless, including repeated Drop cleanup.
            self.subscribers.remove(&actor);
        } else {
            // Joining a running display must not replay a pre-subscription tick
            // or report an old restart as though this App restarted the link.
            self.subscribers.entry(actor).or_insert_with(|| Subscriber {
                wake,
                seen: self.signal.cursor(),
                restart_generation: self.generation,
            });
        }
        if self.subscribers.len() != before {
            // Allocate outside Signal's tiny snapshot guard. The callback never
            // borrows this registry or waits across CoreVideo start/stop below.
            let wakes: Vec<_> = self
                .subscribers
                .values()
                .map(|s| Arc::clone(&s.wake))
                .collect();
            self.signal.set_wakes(wakes.into());
        }
        // The callback snapshot is current before any native worker is started
        // or stopped, and no snapshot guard remains held during those calls.
        self.reconcile()
    }
    /// Start for the first active App and stop only after the last detaches.
    fn reconcile(&mut self) -> Result<(), LinkError> {
        if self.subscribers.is_empty() {
            // Close the generation gate before stop; a straggler owns no windows.
            self.signal.activate(0);
            self.started = None;
            if self.running {
                self.running = false;
                success(self.native().stop())?;
            }
        } else if !self.running {
            // A second App joining an already-running source leaves this branch
            // untouched: neither its sibling's phase nor restart history changes.
            self.generation = self.generation.wrapping_add(1).max(1);
            self.signal.activate(self.generation);
            self.started = Some(Instant::now());
            if let Err(error) = success(self.native().start()) {
                // Keep membership for a later retry, but reject facts from the
                // unsuccessful activation so no reader can consume a stale tick.
                self.signal.activate(0);
                return Err(error);
            }
            self.running = true;
        }
        Ok(())
    }
    /// Reborrows the permanent retained pointer while registry controls are serialized.
    #[allow(
        unsafe_code,
        reason = "R8/D-53 permits retained CoreVideo pointer reborrowing"
    )]
    fn native(&self) -> &CVDisplayLink {
        // SAFETY: Entry::new stores creation's non-null retained pointer as an
        // exposed-provenance address, never released. Registry serializes controls;
        // the callback never dereferences this owner-only Entry or native reference.
        unsafe { &*std::ptr::with_exposed_provenance::<CVDisplayLink>(self.link) }
    }
    /// Each live App reads the same sealed clock through its independent cursor.
    pub(super) fn take_refresh(&mut self, actor: ActorId) -> Option<Refresh> {
        // A detached facade cannot read clock facts merely because a sibling
        // still keeps this permanent native entry running.
        let subscriber = self.subscribers.get_mut(&actor)?;
        let mut tick = self.signal.take(&mut subscriber.seen)?;
        if subscriber.restart_generation != self.generation {
            subscriber.restart_generation = self.generation;
            tick.restart = self.started.zip(self.signal.first_callback());
        }
        Some(tick)
    }
}

/// Reads borrowed CoreVideo timestamps and publishes an allocation-free wake.
#[allow(
    unsafe_code,
    reason = "R8/D-53 confines native callback pointer reads to puzzle-sys"
)]
unsafe extern "C-unwind" fn output(
    _link: NonNull<CVDisplayLink>,
    _now: NonNull<CVTimeStamp>,
    next: NonNull<CVTimeStamp>,
    _flags: CVOptionFlags,
    _flags_out: NonNull<CVOptionFlags>,
    context: *mut c_void,
) -> CVReturn {
    // SAFETY: Only Entry::new installs this callback, passing a permanent Signal.
    // No stop, close, display removal or registry drop can free that allocation.
    let signal = unsafe { &*context.cast::<Signal>() };
    // SAFETY: CoreVideo supplies a valid read-only timestamp for this invocation;
    // copying it does not retain a borrowed pointer beyond the callback.
    let next = unsafe { *next.as_ref() };
    if let Some((target, period)) = clock(&next) {
        signal.publish(next.hostTime, target, period, Instant::now());
    }
    0
}

/// Bridges CoreVideo's host tick frequency into Rust's monotonic Instant.
fn clock(next: &CVTimeStamp) -> Option<(Instant, Duration)> {
    // Host-time and video-period validity are independent native flags.
    // Reading a timestamp without both would invent a refresh prediction.
    let required = CVTimeStampFlags::HostTimeValid.0 | CVTimeStampFlags::VideoRefreshPeriodValid.0;
    if next.flags & required != required || next.videoTimeScale <= 0 || next.videoRefreshPeriod <= 0
    {
        return None;
    }
    // CoreVideo and Rust Instant use monotonic host clocks. Sample their bridge
    // on this callback rather than maintaining a timer-derived phase estimate.
    let frequency = CVGetHostClockFrequency();
    let host = CVGetCurrentHostTime();
    let now = Instant::now();
    // Checked conversion rejects malformed native facts without a callback panic.
    let delta =
        Duration::try_from_secs_f64(next.hostTime.abs_diff(host) as f64 / frequency).ok()?;
    // A delayed callback can report a past scanout; preserve that fact so the
    // host can count lateness instead of quietly moving the deadline forward.
    let output = if next.hostTime >= host {
        now.checked_add(delta)?
    } else {
        now.checked_sub(delta)?
    };
    // Integral video units preserve non-integer refresh frequencies such as NTSC.
    let period =
        Duration::try_from_secs_f64(next.videoRefreshPeriod as f64 / next.videoTimeScale as f64)
            .ok()?;
    Some((output, period))
}

/// Preserves the original native status for host logging and diagnostics.
fn success(status: CVReturn) -> Result<(), LinkError> {
    if status == 0 {
        Ok(())
    } else {
        Err(LinkError::Native(status))
    }
}

/// Process-lifetime entries survive actor facades and are never removed.
pub(super) mod registry;
