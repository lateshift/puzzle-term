//! D-29's first event owns the timer; continuous drags never defer every SIGWINCH.
use super::*;
use crate::render_thread::surface::{Extent, Pending, SurfaceInfo, SurfaceUpdate};
use std::time::Duration;

#[test]
fn continuous_drag_notifies_latest_grid_without_resetting_the_deadline() {
    let origin = Instant::now();
    let mut driver = RenderApp {
        live: None,
        timer_due: None,
        grid_pending: false,
        #[cfg(target_os = "macos")]
        geometry_deadline: None,
        pty_resize_due: None,
        perf: crate::window::perf::Counters::new(origin),
        pacer: None,
        render_settle: Duration::ZERO,
        #[cfg(feature = "lock-metrics")]
        observer: None,
    };
    let mut latest = 0;
    let mut notifications = Vec::new();
    // Drive a timer independently of the 10-ms events, like the actor's idle wake.
    // No wall-time assertion or actual sleeping is involved.
    for ms in 0..=225 {
        let now = origin + Duration::from_millis(ms);
        if ms <= 200 && ms % 10 == 0 {
            latest = ms;
            driver.arm_pty_resize(now);
        }
        if driver.pty_resize_due.is_some_and(|due| due <= now) {
            notifications.push((ms, latest));
            driver.flush_resize_due(now);
        }
    }
    assert_eq!(
        notifications,
        [
            (25, 20),
            (55, 50),
            (85, 80),
            (115, 110),
            (145, 140),
            (175, 170),
            (205, 200)
        ]
    );
    assert!(driver.pty_resize_due.is_none());
}

#[test]
fn mailbox_keeps_first_arrival_but_latest_size_for_both_host_updates() {
    let info = SurfaceInfo {
        extent: Extent {
            width: 800,
            height: 600,
        },
        scale: 2.,
        display: 1,
        millihertz: 60000,
    };
    let mut pending = Pending::default();
    pending.merge(SurfaceUpdate::Geometry(info));
    let first = pending.geometry_at;
    let latest = SurfaceInfo {
        extent: Extent {
            width: 1200,
            height: 900,
        },
        ..info
    };
    pending.merge(SurfaceUpdate::Geometry(latest));
    #[cfg(target_os = "macos")]
    pending.merge(SurfaceUpdate::GeometryFrame(latest, Instant::now()));
    assert_eq!(pending.geometry_at, first);
    assert_eq!(pending.geometry, Some(latest));
}
