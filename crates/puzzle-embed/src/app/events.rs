//! Main-thread event delivery and internal render scheduling (D-43/D-50).
//!
//! There are two producers: the Session outbox contains policy facts, while
//! render and platform services post owned actions to the App inbox. Neither
//! queue carries a reference to a mutable Session or a toolkit window. The
//! host can therefore drain both without borrowing a worker-owned object.
//!
//! Scheduling facts are consumed here instead of leaking a second scheduler
//! into each embedding host. Host sinks see title, bell, clipboard and exit
//! results; output arrival and absolute timer deadlines go straight to the
//! rendering mailbox. Delivery time never replaces the original damage time.
//!
//! The wakeup is only a notification that queues may contain work. Draining
//! a stale notification is harmless, and resetting the Session wake latch
//! before reading closes the race with a producer publishing during tick.
//! Closing a surface invalidates its identity before worker cancellation;
//! a queued callback can therefore never target a recycled or destroyed view.
use super::*;
use crate::render_thread::surface::{DamageArrival, SurfaceUpdate};
use puzzle_term::{HostEvent, messages::Event};

impl App {
    /// Delivers every pending host result on the creating thread, without a Session wait.
    ///
    /// The sink may perform OS work. It must return before calling tick again; core
    /// commands can be queued through a cloned Surface from inside the callback.
    pub fn tick(&mut self, mut sink: impl FnMut(SurfaceId, AppEvent)) -> Result<(), ApiError> {
        self.check_thread()?;
        // Worker notifications never invoke the sink. Stale effects after close are discarded.
        while let Ok(post) = self.posted.try_recv() {
            match post {
                Posted::Released(id) => sink(id, AppEvent::SurfaceReleased),
                Posted::Close(id) => {
                    // Endpoint invalidation precedes posting; teardown never waits in close().
                    if self.surfaces.contains_key(&id) {
                        self.close_surface(id)?;
                    }
                }
                Posted::Event(id, event) if self.surfaces.contains_key(&id) => {
                    if matches!(event, AppEvent::Render(HostAction::Frame)) {
                        self.surfaces[&id]
                            .surface
                            .render
                            .update(SurfaceUpdate::Frame);
                    }
                    // Keep the optional rectangle before moving its owned event:
                    // outer None is another event; Some(None) is an offscreen caret.
                    #[cfg(target_os = "macos")]
                    let caret = match &event {
                        AppEvent::Render(HostAction::CursorRect(rect)) => Some(*rect),
                        _ => None,
                    };
                    sink(id, event);
                    // Count after the sink runs; queued-but-undrained facts stay pending.
                    // C translation later has its own marker, not another scheduling wake.
                    #[cfg(target_os = "macos")]
                    if let (Some(rect), Some(native)) = (caret, &self.surfaces[&id].presentation) {
                        native.caret_delivered(rect);
                    }
                }
                // A device error affects every drawable made from its queue/device.
                Posted::FallbackAll => {
                    for entry in self.surfaces.values() {
                        entry.surface.render.update(SurfaceUpdate::Fallback);
                    }
                }
                // A late result cannot act on a closed native view.
                _ => {}
            }
        }
        for (id, entry) in &self.surfaces {
            #[cfg(target_os = "macos")]
            if let Some(native) = &entry.presentation {
                native.flush();
            }
            // Clear before draining; concurrently queued results must post another wake.
            entry.events.begin_drain();
            while let Ok(event) = entry.events.try_recv() {
                // Lifecycle-only receipts are consumed before ordinary host delivery.
                if super::pty_bridge::observe(&entry.provisioned, &event) {
                    continue;
                }
                if let Some(event) = route(event, &entry.surface.render) {
                    sink(*id, AppEvent::Host(event));
                }
            }
        }
        Ok(())
    }
}

/// Internal scheduling facts stay in the library; every externally relevant result is delivered.
fn route(event: HostEvent, render: &SurfaceHandle) -> Option<HostEvent> {
    match event {
        HostEvent::AnimationChanged(active) => render.update(SurfaceUpdate::Animation(active)),
        HostEvent::Wake(wake) => {
            // Preserve source timestamps; host delivery delay must never restart output settling.
            if let Some(at) = wake.arrival {
                render.update(SurfaceUpdate::Output(DamageArrival {
                    first: at.first,
                    previous: at.previous,
                }));
            } else if wake.frame {
                render.update(SurfaceUpdate::Frame);
            }
            if let Some(due) = wake.deadline {
                render.update(SurfaceUpdate::Timer(due));
            }
        }
        HostEvent::RegionsReturned(regions) => render.update(SurfaceUpdate::Regions(regions)),
        HostEvent::Fact(Event::TitleChanged(title)) => {
            // Diagnostic metadata is separate from the host's actual window title setter.
            render.update(SurfaceUpdate::Title(title.clone()));
            return Some(HostEvent::Fact(Event::TitleChanged(title)));
        }
        other => return Some(other),
    }
    None
}
