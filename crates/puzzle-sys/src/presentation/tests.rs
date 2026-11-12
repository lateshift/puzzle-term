//! D-60 mailbox/lifetime tests use the real imported Metal texture and completion queue.
#![allow(
    clippy::unwrap_used,
    reason = "test-owned channels and retained images must be present"
)]
use super::*;

/// Pure demand rules and bounded native ownership across host transfers.
#[path = "tests/geometry.rs"]
mod geometry_cases;
/// Last-presented snapshots exclude ready images, allocation padding and closed state.
#[path = "tests/snapshot.rs"]
mod snapshot_cases;
use std::{
    future::Future,
    pin::pin,
    task::{Context, Poll, Wake as TaskWake, Waker},
    thread,
};

/// The test executor parks only while the native adapter request is outstanding.
struct ThreadWake(thread::Thread);
impl TaskWake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}
fn block_on<T>(future: impl Future<Output = T>) -> T {
    let waker = Waker::from(Arc::new(ThreadWake(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => thread::park(),
        }
    }
}
fn mailbox() -> Arc<Shared> {
    Arc::new(Shared {
        state: Mutex::new(State {
            retired: std::collections::VecDeque::with_capacity(3),
            ..State::default()
        }),
        ready: Condvar::new(),
        wake: wake::Gate::new(Arc::new(|| {})),
        scale: AtomicU64::new(2_f64.to_bits()),
        rate: AtomicU32::new(120_000),
    })
}
#[test]
fn unchanged_geometry_never_waits_and_timeout_retains_previous_frame() {
    let now = Instant::now();
    let geometry = Geometry {
        size: [300, 200],
        scale: 2.0,
    };
    assert_eq!(
        mailbox::wait_deadline(Some(geometry), geometry, true, now, 120_000),
        None
    );
    assert_eq!(
        mailbox::wait_deadline(None, geometry, false, now, 120_000),
        None
    );
    for size in [[600, 400], [150, 100], [300, 200]] {
        // Bounds changes in both directions and a scale-only move get the same
        // single-period opportunity, independent of AppKit's live-resize flag.
        let changed = Geometry { size, scale: 1.0 };
        let deadline = mailbox::wait_deadline(Some(geometry), changed, true, now, 120_000);
        assert_eq!(deadline, Some(now + Duration::from_secs_f64(1.0 / 120.0)));
    }
    let shared = mailbox();
    assert!(mailbox::take_ready(&shared, geometry, None).0.is_none());
    assert!(mailbox::take_ready(&shared, geometry, Some(now - Duration::from_secs(1))).1);
    assert!(shared.lock().current.is_none());
}
#[test]
fn close_wakes_waiting_geometry_callback_without_waiting_for_gpu() {
    let shared = mailbox();
    let waiting = Arc::clone(&shared);
    let (sent, received) = std::sync::mpsc::channel();
    let worker = thread::spawn(move || {
        let result = mailbox::take_ready(
            &waiting,
            Geometry {
                size: [10, 10],
                scale: 2.0,
            },
            Some(Instant::now() + Duration::from_secs(120)),
        );
        sent.send((result.0.is_none(), result.1)).unwrap();
    });
    shared.lock().closed = true;
    shared.ready.notify_all();
    // A hang guard, not a rendering-speed budget; close must wake it explicitly.
    assert_eq!(
        received.recv_timeout(Duration::from_secs(120)).unwrap(),
        (true, false)
    );
    worker.join().unwrap();
}
#[test]
fn imported_images_complete_in_order_preserve_colour_and_bound_pool_ownership() -> Result<(), String>
{
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .map_err(|e| e.to_string())?;
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
        .map_err(|e| e.to_string())?;
    let shared = mailbox();
    let mut target = pool::target(Arc::clone(&shared), 2);
    let geometry = Geometry {
        size: [64, 32],
        scale: 2.0,
    };
    for serial in 0..6 {
        let image = target
            .acquire(&device, geometry)?
            .ok_or("pool unexpectedly full")?;
        assert!(image.is_srgb());
        assert_eq!(image.texture().format(), wgpu::TextureFormat::Bgra8Unorm);
        draw_red(&device, &queue, &image);
        target.submitted(image, &queue, serial)?;
        let (ready, timeout) = mailbox::take_ready(
            &shared,
            geometry,
            Some(Instant::now() + Duration::from_secs(120)),
        );
        assert!(!timeout, "GPU completion hang");
        let ready = ready.ok_or("missing completed image")?;
        assert_eq!(ready.serial, serial);
        // While current is held, acquisition must select a distinct image. Its
        // bytes cannot be overwritten while Core Animation could read them.
        shared.lock().current = Some(ready.slot);
        assert!(target.slots.len() <= target.capacity);
    }
    geometry_cases::verify_bucket_reuse(&device)?;
    snapshot_cases::verify(&device, &queue)?;
    verify_coalescing(&mut target, &device, &queue, geometry)?;
    verify_equal_size_reset(&mut target, &device, geometry)?;
    verify_replacements(&mut target, &device, &queue, &shared, geometry)
}

/// A busy main loop retains one ready image, not one metadata record per frame.
fn verify_coalescing(
    target: &mut Target,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    geometry: Geometry,
) -> Result<(), String> {
    for serial in 6..26 {
        // Production drains these retire messages before each next submission.
        let retired: Vec<_> = std::iter::from_fn(|| target.take_retired()).collect();
        let expected = if serial < 8 { vec![] } else { vec![serial - 2] };
        assert_eq!(retired, expected);
        let image = target
            .acquire(device, geometry)?
            .ok_or("coalescing stalled")?;
        draw_red(device, queue, &image);
        target.submitted(image, queue, serial)?;
        wait_native_completion(&target.shared, serial)?;
        // No take_ready/App tick: completed images must retire their predecessors.
        assert!(target.slots.len() <= target.capacity);
    }
    assert_eq!(target.take_retired(), Some(24));
    assert_eq!(target.take_retired(), None);
    Ok(())
}

/// A device replacement cannot reuse any old texture, even at identical bounds.
fn verify_equal_size_reset(
    target: &mut Target,
    device: &wgpu::Device,
    geometry: Geometry,
) -> Result<(), String> {
    let previous: Vec<_> = target.slots.iter().map(Arc::downgrade).collect();
    target.reset();
    let restored = target
        .acquire(device, geometry)?
        .ok_or("missing restored image")?;
    let restored = Arc::downgrade(&restored.slot);
    // Compare allocation identity, not which idle index happens to be chosen.
    assert!(previous.iter().all(|old| !old.ptr_eq(&restored)));
    Ok(())
}

/// Busy or displayed storage remains inside the pool through jumps and device loss.
fn verify_replacements(
    target: &mut Target,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    shared: &Arc<Shared>,
    geometry: Geometry,
) -> Result<(), String> {
    // A completed old-size frame cannot satisfy a changed-geometry callback.
    let stale = target.acquire(device, geometry)?.ok_or("no spare image")?;
    target.submitted(stale, queue, 6)?;
    wait_native_completion(shared, 6)?;
    let changed = Geometry {
        size: [32, 64],
        scale: 1.0,
    };
    assert!(mailbox::take_ready(shared, changed, None).0.is_none());
    // A wake before CA layout must not destroy a completed future/other-size
    // image. The matching transaction can still consume exactly those pixels.
    assert!(shared.lock().ready.is_some());
    assert_eq!(
        mailbox::take_ready(shared, geometry, None)
            .0
            .unwrap()
            .serial,
        6
    );
    assert_eq!(
        shared.lock().current.as_ref().unwrap().image.size,
        geometry.size
    );
    // Several geometry jumps with old images held must never allocate another
    // pool. Exhaust the remaining slots, then demand a different size again.
    let mut held = Vec::new();
    for width in [80, 96, 112, 128] {
        let next = Geometry {
            size: [width, 32],
            scale: 2.0,
        };
        if let Some(image) = target.acquire(device, next)? {
            held.push(image);
        }
        assert!(target.slots.len() <= target.capacity);
    }
    assert!(target.acquire(device, changed)?.is_none());
    drop(held);
    target.reset();
    assert!(target.acquire(device, changed)?.is_some());
    shared.lock().closed = true;
    assert!(target.acquire(device, changed)?.is_none());
    Ok(())
}

/// Rendering precedes the same-queue completion fence, without a CPU GPU wait.
fn draw_red(device: &wgpu::Device, queue: &wgpu::Queue, image: &RenderImage) {
    let view = image.texture().create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::RED),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
    }
    queue.submit([encoder.finish()]);
}

#[test]
fn retained_completion_cannot_call_host_after_close() {
    let calls = Arc::new(AtomicU64::new(0));
    let count = Arc::clone(&calls);
    let gate = Arc::new(wake::Gate::new(Arc::new(move || {
        count.fetch_add(1, Ordering::Relaxed);
    })));
    gate.notify();
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    gate.close();
    let late = Arc::clone(&gate);
    thread::spawn(move || {
        for _ in 0..1000 {
            late.notify();
        }
    })
    .join()
    .unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    gate.close();
}

/// Observes the native fence without consuming/adopting its pending image.
/// wgpu poll covers only wgpu submissions, not the later empty native command.
fn wait_native_completion(shared: &Shared, serial: u64) -> Result<(), String> {
    // This is the same generous functional hang guard used by the image tests.
    // Condvar release lets completion acquire ownership; no sleep guesses its time.
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut state = shared.lock();
    while state
        .ready
        .as_ref()
        .is_none_or(|ready| ready.serial < serial)
    {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(format!("native completion hang at submission {serial}"));
        }
        let (next, _) = shared
            .ready
            .wait_timeout(state, left)
            .unwrap_or_else(|e| e.into_inner());
        state = next;
    }
    Ok(())
}
