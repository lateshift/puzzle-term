//! Bounded render targets exclude GPU-in-flight, pending and displayed images.
//!
//! A slot's completion block is allocated once. It captures weak ownership, so
//! neither device loss nor a closed view can form a callback/resource cycle.
//!
//! A pool entry has four distinct exclusion conditions. An in-flight command
//! owns the GPU write, a ready mailbox entry awaits the UI, the current entry is
//! still assigned to the layer, and IOSurface's use count covers delayed reads
//! retained by the compositor after the layer replaces its contents. All four
//! must be absent before the render thread may write the allocation again.
//!
//! The configured queue latency bounds in-flight storage; one additional slot
//! represents the front image already being displayed. An occupied pool yields
//! rather than parking the process render worker and delaying another surface.
//! The next physical refresh retries through the ordinary damage/pacer path.
//!
//! Completion uses the very Metal queue underlying wgpu. An empty command buffer
//! ordered after the render submit provides an asynchronous fence with no extra
//! worker thread. Its handler publishes ownership, not UI state. Native layer
//! mutation is deliberately absent here so teardown and GPU completion can race
//! safely: closed mailboxes reject results and weak captures break every cycle.
use super::{Geometry, Ready, Shared, image::Image};
use block2::RcBlock;
use objc2::runtime::ProtocolObject;
use objc2_metal::{MTLCommandBuffer, MTLCommandBufferStatus, MTLCommandQueue};
use std::{
    ptr::NonNull,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

/// Permanent callback type matches Apple's MTLCommandBufferHandler encoding.
type Completion = RcBlock<dyn Fn(NonNull<ProtocolObject<dyn MTLCommandBuffer>>)>;
/// Atomics separate completion ownership from the render-owned texture pool.
pub(super) struct Slot {
    /// Storage is immutable except through serial GPU rendering on its owner.
    pub image: Image,
    /// Rendered extent and request credit are copied before the completion fence.
    geometry: std::sync::Mutex<(Geometry, u64)>,
    /// Set before commit and cleared only by that commit's Metal completion.
    busy: AtomicBool,
    /// Device restoration retires this texture without freeing in-flight storage.
    pub(super) valid: AtomicBool,
    /// No two submissions use a slot concurrently, so one scalar identifies it.
    pub(super) serial: AtomicU64,
    /// Weak captures avoid retaining this slot through its own callback.
    completion: Completion,
}
// SAFETY: completion captures only weak Arcs and atomics; Metal copies the
// immutable block before invoking it. Texture ownership and reuse are checked
// by the sole render worker; no Objective-C UI object lives in a slot.
#[allow(
    unsafe_code,
    reason = "R8/D-60: immutable completion block with thread-safe captures"
)]
// SAFETY: the immutable resource and callback contract is documented above.
unsafe impl Send for Slot {}
// SAFETY: concurrent accesses are immutable/atomic; the callback never renders
// or changes texture properties. Only target acquisition admits a new writer.
#[allow(
    unsafe_code,
    reason = "R8/D-60: slot completion and ownership checks are synchronized"
)]
// SAFETY: shared readers use atomics/retained ownership; writes have one owner.
unsafe impl Sync for Slot {}

/// Acquired native image; callers render only into its validated wgpu texture.
pub struct RenderImage {
    /// The render owner holds exclusive writing authority until submission.
    pub(super) slot: Arc<Slot>,
    /// Exact device rectangle rendered at the allocation's top left.
    geometry: Geometry,
    /// Geometry request answered by this image, or zero for ordinary frames.
    request: u64,
}
impl RenderImage {
    /// Colour matching travels with native IOSurface contents, not the unused Metal container.
    pub fn is_srgb(&self) -> bool {
        super::image::is_srgb(&self.slot.image.surface)
    }
    /// The actual imported allocation; its capacity can exceed the rendered crop.
    pub fn texture(&self) -> &wgpu::Texture {
        &self.slot.image.texture
    }
}
/// Renderer-local pool, transferred once with its native surface driver.
pub struct Target {
    /// UI and GPU completion share only bounded immutable frame ownership.
    pub(super) shared: Arc<Shared>,
    /// At most configured in-flight images plus the currently displayed image.
    pub(super) slots: Vec<Arc<Slot>>,
    /// Derived from desired_maximum_frame_latency + one compositor front image.
    pub(super) capacity: usize,
}
impl Target {
    /// Publish frame-default sRGB even when no image can currently be acquired.
    pub fn background(&self, colour: [f32; 4]) {
        let changed = {
            let mut state = self.shared.lock();
            let changed = state.background != Some(colour);
            state.background = Some(colour);
            changed
        };
        // Main alone changes the layer; completion is not required for a theme
        // change to paint newly exposed bounds while retaining the old image.
        if changed {
            self.shared.wake.notify();
        }
    }
    /// Invalidates size/device-specific storage; compositor references stay alive.
    pub fn reset(&mut self) {
        // Keep occupied old-device images inside the same capacity bound. They
        // can be replaced only after GPU, mailbox and compositor ownership ends.
        for slot in &self.slots {
            slot.valid.store(false, Ordering::Release);
        }
        self.shared.lock().ready = None;
        self.shared.lock().retired.clear();
    }
    /// Metadata for a coalesced image cannot wait for a future UI tick to retire.
    pub fn take_retired(&self) -> Option<u64> {
        // Drain before another submit: at most the fixed pool's outstanding
        // completions can enqueue ordinals between drains. Even with no UI tick,
        // neither these preallocated tokens nor the host metadata grow with output.
        self.shared.lock().retired.pop_front()
    }
    /// Returns None rather than waiting for Core Animation or an occupied GPU slot.
    pub fn acquire(
        &mut self,
        device: &wgpu::Device,
        geometry: Geometry,
    ) -> Result<Option<RenderImage>, String> {
        if self.shared.lock().closed {
            return Ok(None);
        }
        // Round only valid requests: capacity must never hide an oversized frame.
        let limit = device.limits().max_texture_dimension_2d;
        if geometry.size.iter().any(|n| *n == 0 || *n > limit) {
            return Err("IOSurface dimensions outside device limits".into());
        }
        // Capture before encoding; a later size request cannot claim this image.
        let ordinal = self.shared.lock().requests.ordinal(geometry);
        for index in 0..self.slots.len() {
            let slot = &self.slots[index];
            // Geometry changes cannot discard in-flight slots and grow a second
            // pool. Replace only an idle image, within the same fixed capacity.
            if available(slot) {
                if !slot.valid.load(Ordering::Acquire)
                    || slot
                        .image
                        .size
                        .iter()
                        .zip(geometry.size)
                        .any(|(have, need)| *have < need)
                {
                    // Capacity never shrinks during a drag; changing the other
                    // axis cannot throw away room already reserved on this device.
                    let grown = Geometry {
                        size: std::array::from_fn(|axis| {
                            if slot.valid.load(Ordering::Acquire) {
                                geometry.size[axis].max(slot.image.size[axis])
                            } else {
                                // Old-device limits do not constrain the replacement.
                                geometry.size[axis]
                            }
                        }),
                        ..geometry
                    };
                    self.slots[index] = self.allocate(device, grown)?;
                }
                // The slot is exclusive until submission. Keep content extent
                // separate so reuse cannot stretch it to the allocation bucket.
                return Ok(Some(self.acquired(
                    Arc::clone(&self.slots[index]),
                    geometry,
                    ordinal,
                )));
            }
        }
        if self.slots.len() == self.capacity {
            self.shared.lock().pool_blocked = true;
            self.trace_occupied();
            return Ok(None);
        }
        let slot = self.allocate(device, geometry)?;
        self.slots.push(Arc::clone(&slot));
        Ok(Some(self.acquired(slot, geometry, ordinal)))
    }
    /// Publish conservative waiting admission while this acquired slot is retained.
    fn acquired(&self, slot: Arc<Slot>, geometry: Geometry, request: u64) -> RenderImage {
        let image = RenderImage {
            slot,
            geometry,
            request,
        };
        // is_in_use is queried only on render, never in the main display callback.
        // A completed GPU command cannot certify release by the CA server. Main
        // must return instead of waiting for a target its own transaction retains.
        let blocked = self.slots.len() == self.capacity && !self.slots.iter().any(available);
        self.shared.lock().pool_blocked = blocked;
        image
    }
    /// Failure evidence distinguishes native compositor retention from GPU work.
    /// A zero local use count is insufficient: another process can retain the
    /// IOSurface, which is why the global is_in_use predicate stays mandatory.
    /// Rust owners separately identify current/ready images even before the
    /// compositor has incremented its native count. These checks are not interchangeable.
    /// Keeping all three observations prevents a faster but unsafe reuse policy.
    fn trace_occupied(&self) {
        // Trace only when acquisition fails: distinguish compositor use from
        // GPU work and Rust mailbox ownership without changing admission.
        if tracing::enabled!(target:"puzzle_desktop::resize",tracing::Level::TRACE) {
            for (index, slot) in self.slots.iter().enumerate() {
                tracing::trace!(target:"puzzle_desktop::resize",event="native.slot_owned",index,
                busy=slot.busy.load(Ordering::Acquire),owners=Arc::strong_count(slot),
                compositor=slot.image.surface.is_in_use(),use_count=slot.image.surface.use_count(),
                serial=slot.serial.load(Ordering::Acquire));
            }
        }
    }
    /// A slot's completion block is allocated once with its native image.
    fn allocate(&self, device: &wgpu::Device, geometry: Geometry) -> Result<Arc<Slot>, String> {
        // Geometric buckets amortize intermediate-size allocation without any
        // change to the front/in-flight slot bound or the glyph memory budget.
        let allocation = Geometry {
            size: super::geometry::capacity(
                geometry.size,
                device.limits().max_texture_dimension_2d,
            ),
            scale: geometry.scale,
        };
        let image = Image::new(device, allocation)?;
        let shared = Arc::downgrade(&self.shared);
        Ok(Arc::new_cyclic(|slot| Slot {
            image,
            geometry: std::sync::Mutex::new((geometry, 0)),
            busy: AtomicBool::new(false),
            valid: AtomicBool::new(true),
            serial: AtomicU64::new(0),
            completion: completion(Weak::clone(slot), shared),
        }))
    }

    /// Appends an empty completion fence to wgpu's *same* Metal command queue.
    /// The handler only publishes ownership and posts a wake, never Session work.
    #[allow(unsafe_code, reason = "R8/D-60: borrowed same-queue completion fence")]
    pub fn submitted(
        &self,
        image: RenderImage,
        queue: &wgpu::Queue,
        serial: u64,
    ) -> Result<(), String> {
        // SAFETY: queue outlives the guard; no queue ownership or wgpu resources
        // are modified. The empty native command follows the preceding wgpu submit.
        let hal = unsafe { queue.as_hal::<wgpu::hal::api::Metal>() }
            .ok_or("IOSurface needs Metal queue")?;
        let command = hal
            .as_raw()
            .commandBuffer()
            .ok_or("cannot allocate Metal completion fence")?;
        // Publish the ordinal before committing: a tiny command can complete
        // immediately, and its callback must never observe the preceding frame.
        // The callback copies both values while its Arc still excludes reuse.
        *image
            .slot
            .geometry
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = (image.geometry, image.request);
        image.slot.serial.store(serial, Ordering::Release);
        image.slot.busy.store(true, Ordering::Release);
        // SAFETY: RcBlock remains live in the pool and Metal copies it before
        // returning. Signature exactly matches MTLCommandBufferHandler.
        unsafe { command.addCompletedHandler(RcBlock::as_ptr(&image.slot.completion)) };
        command.commit();
        Ok(())
    }
}
/// Allocated once per slot, not for every frame; weak captures outlive close safely.
fn completion(slot: Weak<Slot>, shared: Weak<Shared>) -> Completion {
    RcBlock::new(
        move |command: NonNull<ProtocolObject<dyn MTLCommandBuffer>>| {
            complete(&slot, &shared, command);
        },
    )
}
/// Completion touches no UI object, Session, parser or rendering cache.
#[allow(
    unsafe_code,
    reason = "R8/D-60: Metal supplies a live command to its completion handler"
)]
fn complete(
    slot: &Weak<Slot>,
    shared: &Weak<Shared>,
    command: NonNull<ProtocolObject<dyn MTLCommandBuffer>>,
) {
    let (Some(slot), Some(shared)) = (slot.upgrade(), shared.upgrade()) else {
        return;
    };
    // SAFETY: Metal invokes this block with a nonnull live command buffer and
    // keeps it alive until the callback returns. We read completion status only.
    let succeeded = unsafe { command.as_ref() }.status() == MTLCommandBufferStatus::Completed;
    slot.busy.store(false, Ordering::Release);
    let serial = slot.serial.load(Ordering::Acquire);
    let (geometry, ordinal) = *slot.geometry.lock().unwrap_or_else(|e| e.into_inner());
    {
        let mut state = shared.lock();
        // Host changes and GPU callbacks share retirement. A completion at the
        // old size may answer wait credit, but cannot occupy the latest mailbox.
        state.publish(
            Ready {
                slot,
                serial,
                geometry,
            },
            ordinal,
            succeeded,
        );
    }
    shared.ready.notify_all();
    shared.wake.notify();
}
/// Cold construction derives pool capacity from the existing native latency policy.
pub(super) fn target(shared: Arc<Shared>, latency: u32) -> Target {
    Target {
        shared,
        slots: Vec::with_capacity(latency as usize + 1),
        capacity: latency as usize + 1,
    }
}

/// One shared exclusion rule protects both acquisition and wait preflight.
fn available(slot: &Arc<Slot>) -> bool {
    !slot.busy.load(Ordering::Acquire)
        && Arc::strong_count(slot) == 1
        && !slot.image.surface.is_in_use()
}
