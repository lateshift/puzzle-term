//! Host-source selection survives coalescing, idle turns and surface removal.
use super::*;
use crate::render_thread::surface::Refresh;

/// A busy driver cannot lose the newest host callback to reordered duplicates.
#[test]
fn host_refresh_is_sticky_and_coalesces_without_touching_closed_driver() -> Result<(), String> {
    let (began, started) = mpsc::channel();
    let (seen, observed) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let drops = Arc::new(AtomicUsize::new(0));
    let mut actor = RenderThread::start()?;
    let (id, host) = actor.add(Box::new(Observed {
        began,
        seen,
        drops: Arc::clone(&drops),
        release: Some(gate),
    }))?;
    started
        .recv_timeout(crate::test_support::HANG_GUARD)
        .map_err(|e| e.to_string())?;
    let now = Instant::now();
    let refresh = Refresh {
        display: 3,
        index: 10,
        callback: now,
        output: now + Duration::from_millis(16),
        period: Duration::from_nanos(16_666_667),
    };
    host.update(SurfaceUpdate::HostRefresh(refresh));
    host.update(SurfaceUpdate::HostRefresh(Refresh {
        index: 9,
        ..refresh
    }));
    release.send(()).map_err(|e| e.to_string())?;
    let first = observed
        .recv_timeout(crate::test_support::HANG_GUARD)
        .map_err(|e| e.to_string())?;
    assert!(first.host_driven);
    assert_eq!(first.refresh.map(|r| r.index), Some(10));
    host.update(SurfaceUpdate::Frame);
    let next = observed
        .recv_timeout(crate::test_support::HANG_GUARD)
        .map_err(|e| e.to_string())?;
    assert!(next.host_driven && next.frame);
    assert!(next.refresh.is_none());
    actor.remove(id);
    actor.shutdown();
    host.update(SurfaceUpdate::HostRefresh(refresh));
    assert!(observed.try_recv().is_err());
    assert_eq!(drops.load(Ordering::Acquire), 1);
    Ok(())
}

/// The owner has already sealed a frame when the native host reports a crossing.
/// Acquisition must consult that later mailbox fact, without waiting for a new
/// Session publication or throwing away its completed glyph uploads.
#[test]
fn display_crossing_during_build_cancels_old_acquisition() -> Result<(), String> {
    use crate::render_thread::host::{HostView, Visibility};
    use crate::render_thread::surface::{Extent, SurfaceInfo};
    for (source, destination) in [(1, 3), (3, 1)] {
        for scale in [1.0, 2.0] {
            let info = SurfaceInfo {
                extent: Extent {
                    width: 800,
                    height: 600,
                },
                scale,
                display: source,
                millihertz: 60_000,
            };
            let (began, started) = mpsc::channel();
            let (resume, held) = mpsc::channel();
            let (result, observed) = mpsc::channel();
            let mut actor = RenderThread::start()?;
            let (_, endpoint) = actor.add(Box::new(Crossing {
                host: HostView::new(info, Visibility::default(), Arc::new(|_| {})),
                gate: Some((began, held, result)),
            }))?;
            started
                .recv_timeout(crate::test_support::HANG_GUARD)
                .map_err(|e| e.to_string())?;
            endpoint.update(SurfaceUpdate::Geometry(SurfaceInfo {
                display: destination,
                ..info
            }));
            resume.send(()).map_err(|e| e.to_string())?;
            let allowed = observed
                .recv_timeout(crate::test_support::HANG_GUARD)
                .map_err(|e| e.to_string())?;
            actor.shutdown();
            assert!(
                !allowed,
                "old display {source} acquired after crossing to {destination}, DPR {scale}"
            );
        }
    }
    Ok(())
}

/// A deterministic pause represents frame work after this turn sealed its updates.
struct Crossing {
    /// Uses the same late acquisition predicate as the GPU surface executor.
    host: crate::render_thread::host::HostView,
    /// First frame alone is held; subsequent mailbox turns remain ordinary.
    gate: Option<(mpsc::Sender<()>, mpsc::Receiver<()>, mpsc::Sender<bool>)>,
}
impl Driver for Crossing {
    /// Driver registration connects the production mailbox, including lifecycle wrappers.
    fn attach_updates(&mut self, updates: std::sync::Weak<Mutex<Pending>>) {
        self.host.attach_updates(updates);
    }
    /// No timer or speed assertion: the host explicitly controls the race boundary.
    fn advance(&mut self, _updates: Pending, _now: Instant) -> Option<Instant> {
        if let Some((began, held, result)) = self.gate.take() {
            let _ = began.send(());
            if held.recv_timeout(crate::test_support::HANG_GUARD).is_ok() {
                let _ = result.send(self.host.acquisition_current());
            }
        }
        None
    }
}
