//! Geometry demand and transfer regressions: explicit ownership, no timing budgets.
use super::*;

/// A timeout cannot turn every transaction into another blocking refresh.
#[test]
fn geometry_wait_credit_requires_an_answer_and_never_repeats_a_size() {
    let mut requests = geometry::Requests::default();
    let a = Geometry {
        size: [100, 80],
        scale: 2.,
    };
    let b = Geometry {
        size: [101, 81],
        scale: 2.,
    };
    let c = Geometry {
        size: [102, 82],
        scale: 2.,
    };
    assert_eq!(requests.request(a, true), Some(true));
    let first = requests.ordinal(a);
    // Timeout, repeated display, then a newer size before the first answer.
    assert_eq!(requests.request(a, true), None);
    assert_eq!(requests.request(b, true), Some(false));
    requests.complete(first);
    // The old answer cannot release the newer unanswered request.
    assert_eq!(requests.request(c, true), Some(false));
    requests.complete(requests.ordinal(c));
    assert_eq!(requests.request(a, true), Some(true));
    assert_eq!(requests.request(a, true), None);
}
/// Exact device-pixel crops survive many intermediate sizes inside one bucket.
#[test]
fn allocation_growth_and_contents_crop_preserve_exact_pixels() {
    for limit in [8192, 16384] {
        for size in [[1500, 900], [1599, 1001], [2048, 1024], [2049, 1025]] {
            let allocation = geometry::capacity(size, limit);
            assert!(
                allocation
                    .iter()
                    .zip(size)
                    .all(|(have, need)| *have >= need)
            );
            let crop = geometry::crop(size, allocation);
            assert_eq!(crop.origin.x, 0.);
            assert_eq!(crop.origin.y, 0.);
            assert_eq!(
                (crop.size.width * f64::from(allocation[0])).round() as u32,
                size[0]
            );
            assert_eq!(
                (crop.size.height * f64::from(allocation[1])).round() as u32,
                size[1]
            );
        }
    }
    assert_eq!(
        geometry::capacity([1500, 900], 8192),
        geometry::capacity([1599, 1001], 8192)
    );
}

/// Intermediate pixel sizes reuse one actual Metal/IOSurface allocation.
pub(super) fn verify_bucket_reuse(device: &wgpu::Device) -> Result<(), String> {
    let mut pool = pool::target(mailbox(), 2);
    let first = pool
        .acquire(
            device,
            Geometry {
                size: [65, 33],
                scale: 2.,
            },
        )?
        .ok_or("missing first image")?;
    let identity = Arc::downgrade(&first.slot);
    drop(first);
    for n in 66..128 {
        let image = pool
            .acquire(
                device,
                Geometry {
                    size: [n, 34],
                    scale: 2.,
                },
            )?
            .ok_or("reused pool full")?;
        assert!(identity.ptr_eq(&Arc::downgrade(&image.slot)));
        assert_eq!(image.texture().size().width, 128);
    }
    assert_eq!(pool.slots.len(), 1);
    Ok(())
}

/// A full compositor pool can only progress after the current transaction returns.
#[test]
fn compositor_owned_pool_never_admits_a_geometry_wait() {
    let mut requests = geometry::Requests::default();
    let a = Geometry {
        size: [100, 80],
        scale: 2.,
    };
    let b = Geometry {
        size: [101, 81],
        scale: 2.,
    };
    assert_eq!(requests.request(a, false), Some(false));
    assert_eq!(requests.request(a, false), None);
    requests.complete(requests.ordinal(a));
    assert_eq!(requests.request(b, true), Some(true));
}

/// A transferred surface must release obsolete pending pixels, even when all
/// other slots remain held by the old and new host transactions. No clock or
/// compositor scheduling is involved: explicit ownership determines progress.
#[test]
fn transferred_pending_geometry_cannot_pin_the_last_async_target() -> Result<(), String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .map_err(|e| e.to_string())?;
    let (device, _) = block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
        .map_err(|e| e.to_string())?;
    let shared = mailbox();
    let mut target = pool::target(Arc::clone(&shared), 2);
    let old = Geometry {
        size: [600, 360],
        scale: 2.,
    };
    let final_size = Geometry {
        size: [640, 360],
        scale: 2.,
    };
    let source = Control(Arc::clone(&shared));
    source.resize(old, 60_000);
    // Simulate both window transactions still retaining their front images.
    let old_front = target.acquire(&device, old)?.ok_or("no old front")?;
    let new_front = target.acquire(&device, old)?.ok_or("no new front")?;
    let pending = target.acquire(&device, old)?.ok_or("no pending image")?;
    publish_image(&shared, pending, 3, old);
    assert!(target.acquire(&device, final_size)?.is_none());
    // The same surface capability survives reparenting into a different host.
    // Preflight has declined; this update must still free the obsolete result.
    let destination = source.clone();
    destination.resize(final_size, 120_000);
    assert_eq!(target.take_retired(), Some(3));
    // An old-size GPU completion arriving after that discard must not recreate
    // the same hold. Both completion paths use the production publish function.
    let stale = target
        .acquire(&device, old)?
        .ok_or("no stale completion slot")?;
    publish_image(&shared, stale, 4, old);
    assert!(shared.lock().ready.is_none());
    assert_eq!(target.take_retired(), Some(4));
    let image = target
        .acquire(&device, final_size)?
        .ok_or("obsolete geometry permanently suppressed async delivery")?;
    publish_image(&shared, image, 5, final_size);
    let ready = mailbox::take_ready(&shared, final_size, None)
        .0
        .ok_or("final frame absent")?;
    assert_eq!(ready.geometry, final_size);
    shared.lock().current = Some(ready.slot);
    assert_eq!(target.slots.len(), 3);
    drop((old_front, new_front));
    Ok(())
}

/// The clock is irrelevant: timeout, pool denial, pending work and reparenting
/// all leave an asynchronous request for the final geometry serviceable.
#[test]
fn transferred_or_declined_waits_never_gate_latest_async_demand() {
    let mut requests = geometry::Requests::default();
    let a = Geometry {
        size: [600, 360],
        scale: 2.,
    };
    let b = Geometry {
        size: [620, 360],
        scale: 2.,
    };
    let c = Geometry {
        size: [640, 360],
        scale: 1.,
    };
    assert!(!requests.bind(1));
    assert_eq!(requests.opportunity(Some(a), b, true, true), Some(true));
    let stale = requests.ordinal(b);
    // A timed-out or preflight-declined transaction still posts on normal tick.
    assert_eq!(requests.opportunity(Some(a), b, false, false), Some(false));
    assert_eq!(requests.opportunity(Some(a), c, true, false), Some(false));
    requests.complete(stale);
    assert_eq!(requests.opportunity(Some(a), c, false, true), Some(false));
    assert!(requests.bind(2));
    let _ = requests.opportunity(None, c, false, true);
    requests.complete(stale);
    assert_eq!(requests.opportunity(None, c, true, true), Some(false));
    requests.complete(requests.ordinal(c));
    // Actual adoption, not an earlier admission flag, discharges geometry demand.
    assert_eq!(requests.opportunity(Some(c), c, false, true), None);
}

/// Inject a completion directly: ownership, not GPU speed, drives this regression.
fn publish_image(shared: &Shared, image: RenderImage, serial: u64, geometry: Geometry) {
    shared.lock().publish(
        Ready {
            slot: image.slot,
            serial,
            geometry,
        },
        0,
        true,
    );
}

/// A CA callback which already ran cannot lose the following explicit host size.
#[test]
fn geometry_changes_wake_async_delivery_once_without_idle_churn() {
    let mut shared = mailbox();
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = Arc::clone(&count);
    Arc::get_mut(&mut shared).unwrap().wake = super::super::wake::Gate::new(Arc::new(move || {
        observed.fetch_add(1, Ordering::Relaxed);
    }));
    let control = Control(Arc::clone(&shared));
    let mut geometry = Geometry {
        size: [991, 1192],
        scale: 2.,
    };
    control.resize(geometry, 60_000);
    assert_eq!(count.load(Ordering::Relaxed), 1);
    control.resize(geometry, 60_000);
    assert_eq!(count.load(Ordering::Relaxed), 1);
    geometry.size = [1001, 777];
    control.resize(geometry, 60_000);
    assert_eq!(count.load(Ordering::Relaxed), 2);
    shared.wake.close();
    geometry.size[0] += 1;
    control.resize(geometry, 60_000);
    assert_eq!(count.load(Ordering::Relaxed), 2);
}
