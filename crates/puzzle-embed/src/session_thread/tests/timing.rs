//! A missed build opportunity preserves its completed slot for the following refresh.
use super::*;

/// A NothingNew acknowledgement cannot drop the dirty slot missed at the last refresh.
#[test]
fn unchanged_reply_preserves_late_complete_ink_at_an_expired_deadline() {
    let (started, producer, generation, now) = sealed_owner();
    // Capture real owner answers, then replay their already-queued ordering without
    // depending on whether this machine schedules the owner between drain calls.
    let (queued, _, sender) = Frames::channels().unwrap_or_else(|error| panic!("{error}"));
    let mut expected = None;
    for dirty in [true, false] {
        let id = started
            .handle
            .request_frame(now, generation, 0, now)
            .unwrap_or_else(|error| panic!("{error}"));
        let answer = started
            .frames
            .receive_until(Instant::now() + crate::test_support::HANG_GUARD)
            .unwrap_or_else(|| panic!("answer"));
        assert_eq!(answer.published, dirty);
        if dirty {
            expected = Some(id);
        }
        sender
            .send(answer)
            .unwrap_or_else(|error| panic!("{error}"));
    }
    let mut observed = 0;
    let (answer, answered) =
        queued.newest_until(1, now - Duration::from_secs(1), |_| observed += 1);
    assert!(answered);
    assert_eq!(observed, 2);
    let answer = answer.unwrap_or_else(|| panic!("dirty ink was lost"));
    assert_eq!(Some(answer.id), expected);
    assert_eq!(answer.parsed, 6);
    assert_eq!(
        answer
            .slot
            .retained_row(0)
            .unwrap_or_else(|| panic!("row"))
            .cells[0]
            .ch,
        u32::from('s')
    );
    started.frames.recycle(answer.slot);
    drop(producer);
    assert!(
        started
            .worker
            .join()
            .unwrap_or_else(|error| panic!("{error:?}"))
            .is_ok()
    );
}

/// A real idle owner supplies sealed rows without another producer advancing them.
fn sealed_owner() -> (Started, crate::pty_pool::Producer, u64, Instant) {
    let (writes, _) = mpsc::channel();
    let mut session = Session::new(
        SessionConfig::default(),
        Services::new(Box::new(Writer(writes)), Box::new(Quiet)),
    );
    let fonts = crate::font_data::bundled_font_set().unwrap_or_else(|error| panic!("{error}"));
    let context = puzzle_term::frame_builder::context::TextContext::new(fonts, 14.0, 1.0)
        .unwrap_or_else(|error| panic!("{error}"));
    session.set_text_context(&context);
    let now = Instant::now();
    session.feed(b"sealed", now);
    let (producer, consumer) = crate::pty_pool::pair();
    let started =
        start(session, consumer, Config::default()).unwrap_or_else(|error| panic!("{error}"));
    (started, producer, context.atlas_generation(), now)
}

/// Late content retains correlation and completeness, rather than becoming a failed build.
#[test]
fn expired_opportunity_still_publishes_a_sealed_answer_with_owner_clocks() {
    let (writes, _) = mpsc::channel();
    let mut session = Session::new(
        SessionConfig::default(),
        Services::new(Box::new(Writer(writes)), Box::new(Quiet)),
    );
    let fonts = crate::font_data::bundled_font_set().unwrap_or_else(|error| panic!("{error}"));
    let context = puzzle_term::frame_builder::context::TextContext::new(fonts, 14.0, 1.0)
        .unwrap_or_else(|error| panic!("{error}"));
    session.set_text_context(&context);
    session.feed(b"sealed", Instant::now());
    let (producer, consumer) = crate::pty_pool::pair();
    let started =
        start(session, consumer, Config::default()).unwrap_or_else(|error| panic!("{error}"));
    let now = Instant::now();
    let id = started
        .handle
        .request_frame(
            now,
            context.atlas_generation(),
            0,
            now - Duration::from_secs(1),
        )
        .unwrap_or_else(|error| panic!("{error}"));
    // The next opportunity receives the late answer without another Session query.
    let answer = started
        .frames
        .receive_until(now + crate::test_support::HANG_GUARD)
        .unwrap_or_else(|| panic!("late answer missing"));
    assert_eq!(answer.id, id);
    assert!(answer.published);
    assert_eq!(
        answer
            .slot
            .retained_row(0)
            .unwrap_or_else(|| panic!("row"))
            .cells[0]
            .ch,
        u32::from('s')
    );
    assert_timing(answer.timing);
    // The completed allocation returns by value, even after its original deadline.
    started.frames.recycle(answer.slot);
    drop(producer);
    assert!(
        started
            .worker
            .join()
            .unwrap_or_else(|error| panic!("{error:?}"))
            .is_ok()
    );
}

/// The queue and owner intervals partition request latency without cross-thread CPU reads.
fn assert_timing(timing: PublicationTiming) {
    let sample = timing.report();
    assert_eq!(sample["published_after_build_deadline"], true);
    let total = sample["request_to_publish_wall_ms"]
        .as_f64()
        .unwrap_or_else(|| panic!("total"));
    let queue = sample["control_wait_wall_ms"]
        .as_f64()
        .unwrap_or_else(|| panic!("queue"));
    let work = sample["applied_to_publish_wall_ms"]
        .as_f64()
        .unwrap_or_else(|| panic!("work"));
    assert!((total - queue - work).abs() < 1e-9);
    #[cfg(all(feature = "lock-metrics", unix))]
    assert!(
        sample["applied_to_publish_thread_cpu_ms"]
            .as_f64()
            .is_some()
    );
}
