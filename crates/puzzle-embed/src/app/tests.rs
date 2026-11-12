//! Public host capabilities reject misuse, defer callbacks and join live children.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "functional failures identify native fixture preconditions"
)]
use super::*;
use crate::{pty, pty_pool, session_thread, test_support, window::live::start_reader};
use puzzle_term::{
    HostEvent, Session, SessionConfig,
    messages::Event,
    platform::{PtySize, PublicationWaker, Services},
};
use std::time::{Duration, Instant};
// Distribution-specific tests stay beside their implementations.
include!("tests/optional_items.rs");
/// Host output uses the real fixed-pool/Session path without a child.
mod transport;
/// Native GPU submissions remain independent of a host-fed output backlog.
mod transport_flood;

/// A driver retains real snapshot ownership without involving a GPU in input/lifetime tests.
struct IdleDriver(session_thread::Frames);
impl crate::render_thread::Driver for IdleDriver {
    /// The receiver must stay alive until actor removal, just as in a native render driver.
    fn advance(
        &mut self,
        _: crate::render_thread::surface::Pending,
        _: Instant,
    ) -> Option<Instant> {
        let _ = &self.0;
        None
    }
}

/// Builds the real fixed-pool reader, ordered writer and Session, registered under App ownership.
fn live(app: &mut App, script: &str) -> Surface {
    let mut command = portable_pty::CommandBuilder::new("/bin/sh");
    command.args(["-c", script]);
    let mut child = pty::spawn(
        command,
        PtySize {
            columns: 80,
            rows: 24,
            pixel_width: 800,
            pixel_height: 480,
        },
    )
    .unwrap();
    let killer = child.child.clone_killer();
    let writer = child.transport.start_writer().unwrap();
    let (producer, consumer) = pty_pool::pair();
    let (send_exit, exit) = mpsc::channel();
    let drain = start_reader(
        child.reader,
        producer,
        child.child,
        child.started,
        send_exit,
    );
    let mut pipeline = Pipeline::new(killer, writer, drain);
    let session = Session::new(
        SessionConfig {
            on_exit: puzzle_term::session::OnExit::Hold,
            ..SessionConfig::default()
        },
        Services::new(
            Box::new(child.transport),
            Box::<PublicationWaker>::default(),
        ),
    );
    let started = session_thread::start(
        session,
        consumer,
        session_thread::Config {
            exit: Some(exit),
            notify: Some(Arc::clone(&app.config.wakeup)),
            recorder: None,
            decoder: None,
        },
    )
    .unwrap();
    pipeline.attach(started.handle.clone(), started.worker);
    let id = SurfaceId(app.next);
    app.next += 1;
    app.register(
        id,
        Box::new(IdleDriver(started.frames)),
        started.handle,
        started.events,
        pipeline,
        None,
    )
    .unwrap()
}

/// No personal configuration or OS callback participates in a functional fixture.
pub(super) fn app() -> (App, mpsc::Receiver<()>) {
    let (send, receive) = mpsc::channel();
    let path = std::env::temp_dir().join("puzzle-embed-app-nonexistent-config.json");
    let (settings, _) = DesktopSettings::open(Some(path.to_str().unwrap()), None, &[]).unwrap();
    let app = App::new(AppConfig {
        settings,
        wakeup: Arc::new(move || {
            let _ = send.send(());
        }),
    })
    .unwrap();
    (app, receive)
}

/// A retained clone cannot submit from a foreign host thread or address a later surface.
#[test]
fn wrong_thread_and_closed_surfaces_are_rejected() {
    let (mut app, _) = app();
    let surface = live(&mut app, "exec sleep 60");
    let foreign = surface.clone();
    let callback = Instant::now();
    let refresh = crate::Refresh {
        display: 3,
        index: 1,
        callback,
        output: callback + Duration::from_millis(16),
        period: Duration::from_nanos(16_666_667),
    };
    std::thread::spawn(move || {
        assert!(matches!(
            foreign.text("unwanted".into()),
            Err(ApiError::WrongThread)
        ));
        assert!(matches!(foreign.focus(true), Err(ApiError::WrongThread)));
        assert!(matches!(
            foreign.refresh(refresh),
            Err(ApiError::WrongThread)
        ));
        assert!(matches!(
            foreign.input(crate::RawInput::Focus(true)),
            Err(ApiError::WrongThread)
        ));
    })
    .join()
    .unwrap();
    app.close_surface(surface.id()).unwrap();
    assert!(matches!(surface.refresh(refresh), Err(ApiError::Closed)));
    assert!(matches!(
        surface.input(crate::RawInput::Focus(true)),
        Err(ApiError::Closed)
    ));
    assert!(matches!(
        surface.paste("stale".into()),
        Err(ApiError::Closed)
    ));
    assert!(matches!(
        app.close_surface(surface.id()),
        Err(ApiError::Closed)
    ));
    let replacement = live(&mut app, "exec sleep 60");
    assert_ne!(surface.id(), replacement.id());
    assert!(matches!(
        surface.text("still stale".into()),
        Err(ApiError::Closed)
    ));
    assert!(replacement.focus(true).is_ok());
    replacement.close().unwrap();
    assert!(matches!(replacement.focus(true), Err(ApiError::Closed)));
    app.tick(|_, _| {}).unwrap();
    assert!(matches!(
        app.close_surface(replacement.id()),
        Err(ApiError::Closed)
    ));
}

/// Child facts and worker effects are delivered only inside tick on the creating thread.
#[test]
fn tick_delivers_child_facts_and_effects_on_host_thread() {
    use puzzle_term::platform::{Notifier, UrlOpener};
    let (mut app, wake) = app();
    let surface = live(
        &mut app,
        r"printf ']2;host titledone
'",
    );
    let mut effects = services::Effects {
        id: surface.id(),
        posts: app.posts.clone(),
        wake: Arc::clone(&app.config.wakeup),
    };
    std::thread::spawn(move || {
        effects.open("https://example.test/").unwrap();
        effects.notify("title", "body").unwrap();
    })
    .join()
    .unwrap();
    let owner = thread::current().id();
    let (mut title, mut bell, mut exit, mut url, mut notification) =
        (false, false, false, false, false);
    let end = Instant::now() + test_support::HANG_GUARD;
    while !(title && bell && exit && url && notification) {
        app.tick(|id, event| {
            assert_eq!(thread::current().id(), owner);
            assert_eq!(id, surface.id());
            match event {
                AppEvent::Host(HostEvent::Fact(Event::TitleChanged(value))) => {
                    assert_eq!(value, "host title");
                    title = true;
                }
                AppEvent::Host(HostEvent::Fact(Event::Bell)) => bell = true,
                AppEvent::Host(HostEvent::Fact(Event::SessionExited(_))) => exit = true,
                AppEvent::OpenLink(value) => {
                    assert_eq!(value, "https://example.test/");
                    url = true;
                }
                AppEvent::Notification { title, body } => {
                    assert_eq!((title.as_str(), body.as_str()), ("title", "body"));
                    notification = true;
                }
                _ => {}
            }
        })
        .unwrap();
        assert!(Instant::now() < end, "host events never arrived");
        // A short receive is only an event-pump convenience; the sole assertion is a hang guard.
        let _ = wake.recv_timeout(Duration::from_millis(10));
    }
    app.close_surface(surface.id()).unwrap();
    app.posts
        .send(Posted::Event(
            surface.id(),
            AppEvent::OpenLink("late".into()),
        ))
        .unwrap();
    app.tick(only_release_after_close).unwrap();
}

/// Dropping App with a flooding and a held child joins all workers even with host clones alive.
#[test]
fn app_drop_joins_open_surfaces() {
    let (send, receive) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let (mut app, wake) = app();
        let flood = live(&mut app, "stty raw -echo; exec yes flood");
        let held = live(&mut app, "printf 'held final screen\r\n'");
        let mut exited = false;
        let end = Instant::now() + test_support::HANG_GUARD;
        while !exited || flood.session.parsed_total() < 65_536 {
            app.tick(|id, event| {
                if id == held.id()
                    && matches!(
                        event,
                        AppEvent::Host(HostEvent::Fact(Event::SessionExited(_)))
                    )
                {
                    exited = true;
                }
            })
            .unwrap();
            assert!(
                Instant::now() < end,
                "fixture never reached flood and held states"
            );
            let _ = wake.recv_timeout(Duration::from_millis(10));
        }
        drop(app);
        assert!(matches!(flood.text("closed".into()), Err(ApiError::Closed)));
        assert!(matches!(held.focus(true), Err(ApiError::Closed)));
        send.send(()).unwrap();
    });
    receive
        .recv_timeout(test_support::HANG_GUARD)
        .expect("App teardown deadlocked");
    worker.join().unwrap();
}

/// Native release follows actor destruction, never just endpoint invalidation.
mod release;

/// Lifecycle completion remains deliverable, but a closed endpoint has no effects.
fn only_release_after_close(_: SurfaceId, event: AppEvent) {
    assert!(
        matches!(event, AppEvent::SurfaceReleased),
        "closed surface delivered a stale effect"
    );
}
