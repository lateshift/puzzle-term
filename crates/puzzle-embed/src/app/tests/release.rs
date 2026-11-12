//! Closing mid-frame preserves a borrowed native view until the release event.
use super::*;

/// A deterministic actor turn holds a stand-in native resource across close.
struct BlockedDriver {
    entered: mpsc::Sender<()>,
    resume: mpsc::Receiver<()>,
    dropped: mpsc::Sender<()>,
}
impl crate::render_thread::Driver for BlockedDriver {
    fn advance(
        &mut self,
        _: crate::render_thread::surface::Pending,
        _: Instant,
    ) -> Option<Instant> {
        self.entered.send(()).unwrap();
        self.resume.recv_timeout(test_support::HANG_GUARD).unwrap();
        None
    }
}
impl Drop for BlockedDriver {
    fn drop(&mut self) {
        self.dropped.send(()).unwrap();
    }
}

#[test]
fn native_release_is_after_mid_frame_driver_drop() {
    let (mut app, _) = app();
    let surface = live(&mut app, "exec sleep 60");
    // This separate actor target exercises the exact wrapper used by registration.
    let (entered, entering) = mpsc::channel();
    let (resume, continuing) = mpsc::channel();
    let (dropped, dropping) = mpsc::channel();
    let id = SurfaceId(999);
    let (driver, completion) = crate::app::release::ReleasedDriver::wrap(
        id,
        Box::new(BlockedDriver {
            entered,
            resume: continuing,
            dropped,
        }),
        app.posts.clone(),
        Arc::clone(&app.config.wakeup),
    );
    let (render_id, handle) = app.render.add(driver).unwrap();
    handle.update(crate::render_thread::surface::SurfaceUpdate::Frame);
    entering.recv_timeout(test_support::HANG_GUARD).unwrap();
    app.render.remove(render_id);
    let mut released = false;
    app.tick(|event_id, event| {
        if event_id == id && matches!(event, AppEvent::SurfaceReleased) {
            released = true;
        }
    })
    .unwrap();
    assert!(!released);
    assert!(dropping.try_recv().is_err());
    assert!(completion.try_recv().is_err());
    resume.send(()).unwrap();
    completion.recv_timeout(test_support::HANG_GUARD).unwrap();
    dropping.recv_timeout(test_support::HANG_GUARD).unwrap();
    let end = Instant::now() + test_support::HANG_GUARD;
    while !released {
        app.tick(|event_id, event| {
            if event_id == id && matches!(event, AppEvent::SurfaceReleased) {
                released = true;
            }
        })
        .unwrap();
        assert!(Instant::now() < end, "native release was never delivered");
        std::thread::yield_now();
    }
    app.close_surface(surface.id()).unwrap();
}

/// The actual host barrier cannot return while a render turn retains its target.
#[test]
fn synchronous_release_waits_for_mid_frame_drop() {
    let (mut app, _) = app();
    let surface = live(&mut app, "while :; do printf 'flood\\r\\n'; done");
    let (entered, entering) = mpsc::channel();
    let (resume, continuing) = mpsc::channel();
    let (dropped, dropping) = mpsc::channel();
    let (driver, completion) = crate::app::release::ReleasedDriver::wrap(
        surface.id(),
        Box::new(BlockedDriver {
            entered,
            resume: continuing,
            dropped,
        }),
        app.posts.clone(),
        Arc::clone(&app.config.wakeup),
    );
    // Replace only the fixture driver; keep the real flooding Session and cancellation.
    let entry = app.surfaces.get_mut(&surface.id()).unwrap();
    app.render.remove(entry.render_id);
    let (render_id, handle) = app.render.add(driver).unwrap();
    entry.render_id = render_id;
    entry.released = completion;
    handle.update(crate::render_thread::surface::SurfaceUpdate::Frame);
    entering.recv_timeout(test_support::HANG_GUARD).unwrap();
    let valid = Arc::clone(&surface.valid);
    let unblock = std::thread::spawn(move || {
        let end = Instant::now() + test_support::HANG_GUARD;
        // Invalidation proves release_surface entered before this frame can finish.
        while valid.load(std::sync::atomic::Ordering::Acquire) {
            assert!(
                Instant::now() < end,
                "release never invalidated the capability"
            );
            std::thread::yield_now();
        }
        resume.send(()).unwrap();
    });
    app.release_surface(surface.id()).unwrap();
    // Completion follows driver Drop, not merely receipt of actor removal.
    dropping.try_recv().unwrap();
    unblock.join().unwrap();
    assert!(matches!(surface.focus(true), Err(ApiError::Closed)));
}

/// A held render turn cannot defer process-group escalation until after reaping.
/// The renderer resumes only after all slave holders disappear, proving shutdown
/// has no dependency on driver destruction to reach TERM/KILL or join its IO.
#[test]
#[cfg(unix)]
fn slave_holders_are_reaped_before_waiting_for_mid_frame_release() {
    let (mut app, _) = app();
    let surface = live(
        &mut app,
        "nohup sleep 1000 >/dev/tty 2>&1 & printf ready; exit",
    );
    let pid = app.surfaces[&surface.id()].pipeline.drain_pid();
    let end = Instant::now() + test_support::HANG_GUARD;
    while surface.session.parsed_total() < 5 {
        assert!(Instant::now() < end, "background fixture never started");
        std::thread::yield_now();
    }
    let (entered, entering) = mpsc::channel();
    let (resume, continuing) = mpsc::channel();
    let (dropped, dropping) = mpsc::channel();
    let (driver, completion) = crate::app::release::ReleasedDriver::wrap(
        surface.id(),
        Box::new(BlockedDriver {
            entered,
            resume: continuing,
            dropped,
        }),
        app.posts.clone(),
        Arc::clone(&app.config.wakeup),
    );
    let entry = app.surfaces.get_mut(&surface.id()).unwrap();
    app.render.remove(entry.render_id);
    let (render_id, handle) = app.render.add(driver).unwrap();
    entry.render_id = render_id;
    entry.released = completion;
    handle.update(crate::render_thread::surface::SurfaceUpdate::Frame);
    entering.recv_timeout(test_support::HANG_GUARD).unwrap();
    let valid = Arc::clone(&surface.valid);
    let unblock = std::thread::spawn(move || {
        let end = Instant::now() + test_support::HANG_GUARD;
        while valid.load(std::sync::atomic::Ordering::Acquire) {
            assert!(Instant::now() < end, "release never began");
            std::thread::yield_now();
        }
        crate::pipeline::tests::assert_group_gone(pid);
        resume.send(()).unwrap();
    });
    app.release_surface(surface.id()).unwrap();
    dropping.try_recv().unwrap();
    unblock.join().unwrap();
}

/// Native pane destruction must also reap tty descendants, not merely drop a driver.
#[test]
#[cfg(unix)]
fn native_free_reaps_background_and_hup_ignoring_children() {
    let (send, receive) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        for script in [
            "stty raw -echo; exec yes output",
            "stty raw -echo; trap '' HUP; exec yes output",
            "sleep 1000 & printf ready; exit",
            "nohup sleep 1000 >/dev/tty 2>&1 & printf ready; exit",
        ] {
            for surface_free in [true, false] {
                let (mut app, _) = app();
                let surface = live(&mut app, script);
                let pid = app
                    .surfaces
                    .get(&surface.id())
                    .unwrap()
                    .pipeline
                    .drain_pid();
                let end = Instant::now() + test_support::HANG_GUARD;
                while surface.session.parsed_total() < 5 {
                    assert!(Instant::now() < end, "fixture never started: {script}");
                    std::thread::yield_now();
                }
                if surface_free {
                    app.release_surface(surface.id()).unwrap();
                }
                drop(app);
                crate::pipeline::tests::assert_group_gone(pid);
            }
        }
        send.send(()).unwrap();
    });
    receive
        .recv_timeout(test_support::HANG_GUARD)
        .expect("native free deadlocked");
    worker.join().unwrap();
}
