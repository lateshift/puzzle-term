//! A pending native refresh publishes at the next complete transaction without polling.
use super::*;
use puzzle_term::frame_builder::context::TextContext;

/// A single data buffer containing an end and next start must expose the safe boundary.
#[test]
fn outstanding_frame_survives_sync_suppression_and_publishes_completed_ink() -> Result<(), String> {
    let (writes, received) = mpsc::channel();
    let mut session = Session::new(
        SessionConfig::default(),
        Services::new(Box::new(Writer(writes)), Box::new(Quiet)),
    );
    let context = TextContext::new(crate::font_data::bundled_font_set()?, 14.0, 1.0)?;
    session.set_text_context(&context);
    let (mut producer, consumer) = crate::pty_pool::pair();
    let started = start(session, consumer, Config::default())?;
    producer.read_once(&mut Cursor::new(b"\x1b[?2026hcomplete\x1b[5n"))?;
    // DSR proves the prefix was parsed before the renderer requests its snapshot.
    assert_eq!(transaction(&received), b"\x1b[0n");
    let deadline = Instant::now() + crate::test_support::HANG_GUARD; // Deadlock guard, not a speed budget.
    let id = started
        .handle
        .request_frame(Instant::now(), 0, 0, deadline)?;
    producer.read_once(&mut Cursor::new(b"\x1b[?2026l\x1b[?2026h\rpartial"))?;
    let reply = started
        .frames
        .receive_until(deadline)
        .ok_or("frame not answered")?;
    assert_eq!(reply.id, id);
    assert!(reply.published);
    assert_eq!(
        reply.slot.retained_row(0).ok_or("row missing")?.cells[0].ch,
        u32::from('c')
    );
    started.frames.recycle(reply.slot);
    drop(producer);
    started.worker.join().map_err(|_| "owner panicked")??;
    Ok(())
}
