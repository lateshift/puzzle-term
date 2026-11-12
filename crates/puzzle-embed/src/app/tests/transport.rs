//! Host bytes retain pool bounds, input order, affinity and final-output lifecycle.
use super::*;
use puzzle_term::messages::ExitInfo;

/// Test fixtures use the real source/pool/Session assembly and checked host endpoint.
pub(super) fn host(app: &mut App) -> Surface {
    let id = app.next_id().unwrap();
    let (transport, pool, exit, mut pipeline) = super::super::transport::start(app, id);
    let mut session = Session::new(
        SessionConfig {
            on_exit: puzzle_term::session::OnExit::Hold,
            ..SessionConfig::default()
        },
        Services::new(transport, Box::<PublicationWaker>::default()),
    );
    let context = puzzle_term::frame_builder::context::TextContext::new(
        crate::font_data::bundled_font_set().unwrap(),
        14.0,
        1.0,
    )
    .unwrap();
    session.set_text_context(&context);
    let started = app.start_session(session, pool, exit, None).unwrap();
    pipeline.attach(started.handle.clone(), started.worker);
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

/// Feeding 100 MB through Surface retries credit and validates the bytes at the
/// Session boundary. The pool witness separately compares every byte in order.
/// No child process is spawned and EOF cannot overtake the final accepted byte.
#[test]
fn public_host_hundred_mb_exact_and_bounded() {
    let (mut app, wake) = app();
    let surface = host(&mut app);
    let source = b"abcdef0123456789\r\n".repeat(4096);
    let mut sent = 0;
    let end = Instant::now() + test_support::HANG_GUARD;
    while sent < 100_000_000 {
        let length = source.len().min(100_000_000 - sent);
        let accepted = surface.feed_output(&source[..length]).unwrap();
        sent += accepted;
        assert!(surface.session.pending_bytes() <= 4 * 1024 * 1024);
        app.tick(|_, _| {}).unwrap();
        assert!(Instant::now() < end, "host transfer hung");
        if accepted == 0 {
            let _ = wake.recv_timeout(Duration::from_millis(1));
        }
    }
    surface
        .exited(ExitInfo {
            code: Some(0),
            signal: None,
            runtime_ms: 1000,
        })
        .unwrap();
    let mut exited = false;
    while !exited {
        app.tick(|_, event| {
            if matches!(
                event,
                AppEvent::Host(HostEvent::Fact(Event::SessionExited(_)))
            ) {
                exited = true;
            }
        })
        .unwrap();
        assert!(Instant::now() < end, "exit overtook or lost final bytes");
        let _ = wake.recv_timeout(Duration::from_millis(1));
    }
    assert_eq!(surface.session.parsed_total(), 100_000_000);
    assert!(matches!(
        surface.feed_output(b"late"),
        Err(ApiError::Closed)
    ));
    drop(app);
    assert!(matches!(
        surface.feed_output(b"closed"),
        Err(ApiError::Closed)
    ));
}

/// The host echo source gets FIFO input/replies only during tick, on its own thread.
#[test]
fn host_echo_order_resize_exit_and_affinity() {
    let (mut app, wake) = app();
    let surface = host(&mut app);
    assert_eq!(surface.feed_output(b"\x1b[5n").unwrap(), 4);
    // Wait for the parser's first query before submitting a later key.
    let end = Instant::now() + test_support::HANG_GUARD;
    while surface.session.parsed_total() < 4 {
        assert!(Instant::now() < end);
        thread::yield_now();
    }
    surface.text("echo".into()).unwrap();
    surface
        .send(puzzle_term::SessionMessage::GridResize {
            columns: 90,
            rows: 25,
            surface: [900, 500],
        })
        .unwrap();
    surface
        .send(puzzle_term::SessionMessage::PtyResize([900, 500]))
        .unwrap();
    let owner = thread::current().id();
    let mut input = Vec::new();
    let mut resized = false;
    while input.len() < 8 || !resized {
        app.tick(|_, event| {
            assert_eq!(thread::current().id(), owner);
            match event {
                AppEvent::TransportInput(bytes) => {
                    input.extend_from_slice(&bytes);
                    assert_eq!(surface.feed_output(&bytes).unwrap(), bytes.len());
                }
                AppEvent::TransportReply(reply) => {
                    consume_reply(&surface, reply, &mut input);
                }
                AppEvent::TransportResize(size) => {
                    // Construction reports initial dimensions before the requested change.
                    if (size.columns, size.rows) == (90, 25) {
                        resized = true;
                    } else {
                        assert_eq!((size.columns, size.rows), (80, 24));
                    }
                }
                _ => {}
            }
        })
        .unwrap();
        surface
            .session
            .request_frame(Instant::now(), 0, 0, Instant::now())
            .unwrap();
        assert!(Instant::now() < end, "host echo/resize hung");
        let _ = wake.recv_timeout(Duration::from_millis(10));
    }
    assert_eq!(input, b"\x1b[0necho");
    reject_foreign_and_closed(&mut app, &surface);
}

/// The host completes only after its actual echo writer consumes the whole reply.
fn consume_reply(
    surface: &Surface,
    reply: puzzle_term::platform::reply::ReplyWrite,
    input: &mut Vec<u8>,
) {
    input.extend_from_slice(&reply.bytes);
    assert_eq!(
        surface.feed_output(&reply.bytes).unwrap(),
        reply.bytes.len()
    );
    surface.reply_complete(reply.ticket.id()).unwrap();
}

/// Affinity is checked before admission, and closing invalidates retained endpoints.
fn reject_foreign_and_closed(app: &mut App, surface: &Surface) {
    let foreign = surface.clone();
    thread::spawn(move || {
        assert!(matches!(
            foreign.feed_output(b"wrong"),
            Err(ApiError::WrongThread)
        ));
    })
    .join()
    .unwrap();
    surface.close().unwrap();
    app.tick(|_, _| {}).unwrap();
    assert!(matches!(
        surface.feed_output(b"stale"),
        Err(ApiError::Closed)
    ));
}

/// PTY-backed endpoints reject host output; no source can interleave another's bytes.
#[test]
fn pty_surface_rejects_host_output() {
    let (mut app, _) = app();
    let surface = live(&mut app, "exec sleep 60");
    assert!(matches!(
        surface.feed_output(b"injected"),
        Err(ApiError::InvalidArgument(_))
    ));
    assert!(matches!(
        surface.exited(ExitInfo {
            code: Some(0),
            signal: None,
            runtime_ms: 0
        }),
        Err(ApiError::InvalidArgument(_))
    ));
}

/// Timed raw keys, typed keys and IME commits retain the ordinary transport FIFO.
#[test]
fn timed_host_inputs_preserve_bytes_and_session_policy() {
    let (mut app, wake) = app();
    let surface = host(&mut app);
    surface
        .input_timed(
            crate::RawInput::Key(crate::RawKey::character("x", Some("x"))),
            crate::InputTiming::new(10.0, 10.125).unwrap(),
        )
        .unwrap();
    surface
        .key_timed(
            puzzle_term::vt::input::KeyEvent::press(puzzle_term::vt::input::Key::Enter),
            crate::InputTiming::new(10.0, 10.25).unwrap(),
        )
        .unwrap();
    surface
        .text_timed("é".into(), crate::InputTiming::new(10.0, 10.5).unwrap())
        .unwrap();
    let mut bytes = Vec::new();
    let end = Instant::now() + test_support::HANG_GUARD;
    let mut native_commit = false;
    while bytes.len() < 3 || !native_commit {
        app.tick(|_, event| match event {
            AppEvent::TransportInput(input) => bytes.extend(input),
            AppEvent::Host(HostEvent::NativeTextAllowed(true)) => native_commit = true,
            _ => {}
        })
        .unwrap();
        assert!(Instant::now() < end, "timed input delivery hung");
        let _ = wake.recv_timeout(Duration::from_millis(1));
    }
    // Surface::key preserves native insertion ownership; raw callbacks encode
    // their own bytes, while an unhandled typed key asks the host for a commit.
    assert_eq!(bytes, "xé".as_bytes());
    // Timing metadata never bypasses the Session-owned built-in search input mode.
    surface
        .dispatch(puzzle_term::messages::Command::Search(
            puzzle_term::search::SearchCommand::Request,
        ))
        .unwrap();
    surface
        .text_timed(
            "needle".into(),
            crate::InputTiming::new(11.0, 11.0).unwrap(),
        )
        .unwrap();
    surface
        .session
        .probe(Instant::now() + test_support::HANG_GUARD, |session| {
            assert_eq!(session.search_bar_query(), Some("needle"));
        })
        .unwrap();
    app.tick(|_, event| assert!(!matches!(event, AppEvent::TransportInput(_))))
        .unwrap();
}
