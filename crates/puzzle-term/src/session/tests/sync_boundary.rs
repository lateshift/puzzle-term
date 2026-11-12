//! A refresh request samples a completed sync transaction, never the next partial one.
use super::*;
use crate::{PublishOutcome, frame_snapshot::FrameSnapshot};

/// Repeated transactions can hold mode 2026 at every tick without starving publication.
#[test]
fn pending_refresh_yields_at_sync_end_with_an_injected_clock() {
    let (mut session, _, _) = fixture();
    let context = super::overlays::text_context(1.0);
    session.set_text_context(&context);
    let now = Instant::now();
    let mut slot = FrameSnapshot::default();
    for tick in 0..120 {
        let at = now + Duration::from_nanos(16_666_667) * tick;
        session.feed(b"\x1b[?2026h\rcomplete", at);
        assert_eq!(
            session.publish_frame(at, &mut slot, 0, 0).expect("request"),
            PublishOutcome::NothingNew
        );
        let next = b"\x1b[?2026l\x1b[?2026h\rpartial";
        let consumed = session.feed_to_sync_end(next, at + Duration::from_micros(30));
        assert_eq!(&next[..consumed], b"\x1b[?2026l");
        // Capture at the actual parsed boundary before accepting the unconsumed suffix.
        assert_eq!(
            session
                .publish_frame(at, &mut slot, 0, 0)
                .expect("publication"),
            PublishOutcome::Published
        );
        let row = slot.retained_row(0).expect("sealed row");
        assert_eq!(row.cells[0].ch, u32::from('c'));
        session.feed(&next[consumed..], at);
        assert_eq!(session.terminal.modes.get(true, 2026), Some(true));
    }
}

/// The parser decides the boundary, including split sequences, mode lists and resets.
#[test]
fn sync_boundary_preserves_split_sequences_and_unaccepted_suffix() {
    let (mut session, _, _) = fixture();
    let now = Instant::now();
    session.feed(b"\x1b[?2026h\x1b[?20", now);
    let end = b"26;25l\x1b[?2026hnext";
    assert_eq!(session.feed_to_sync_end(end, now), b"26;25l".len());
    assert_eq!(session.terminal.modes.get(true, 2026), Some(false));
    for reset in [b"\x1bc".as_slice(), b"\x1b[!p"] {
        session.feed(b"\x1b[?2026h", now);
        let mut bytes = reset.to_vec();
        bytes.extend_from_slice(b"\x1b[?2026hnext");
        assert_eq!(session.feed_to_sync_end(&bytes, now), reset.len());
        assert_eq!(session.terminal.modes.get(true, 2026), Some(false));
    }
}
