//! Latest geometry and input barriers use fixed storage without timers.

use super::*;
/// Metrics fields are sender clocks only, not part of geometry semantics.
fn message(value: SessionMessage) -> HostMessage {
    HostMessage {
        value,
        timing: None,
        #[cfg(feature = "lock-metrics")]
        sent: std::time::Instant::now(),
        #[cfg(feature = "lock-metrics")]
        arrival: std::time::Instant::now(),
    }
}
/// A burst retains final sizes; the returned input remains an ordering barrier.
#[test]
fn intermediate_sizes_coalesce_but_input_barriers_keep_order() {
    let mut batch = Batch::default();
    for n in 1..100 {
        assert!(
            batch
                .push(message(SessionMessage::GridResize {
                    columns: n,
                    rows: n,
                    surface: [n as u32; 2]
                }))
                .is_none()
        );
        assert!(
            batch
                .push(message(SessionMessage::PtyResize([n as u32; 2])))
                .is_none()
        );
    }
    assert!(
        batch
            .push(message(SessionMessage::SurfaceSize([200; 2])))
            .is_none()
    );
    let key = batch
        .push(message(SessionMessage::Command(
            puzzle_term::messages::Command::Text("key".into()),
            puzzle_term::Origin::User,
        )))
        .unwrap_or_else(|| panic!("input remains an ordered barrier"));
    assert!(
        matches!(key.value, SessionMessage::Command(puzzle_term::messages::Command::Text(ref value), _) if value == "key")
    );
    let mut seen = Vec::new();
    batch.flush(|v| seen.push(v.value));
    assert_eq!(seen.len(), 3);
    assert!(matches!(
        seen[0],
        SessionMessage::GridResize {
            columns: 99,
            rows: 99,
            ..
        }
    ));
    assert!(matches!(seen[1], SessionMessage::PtyResize([99, 99])));
    assert!(matches!(seen[2], SessionMessage::SurfaceSize([200, 200])));
    assert!(
        batch
            .push(message(SessionMessage::SurfaceSize([300; 2])))
            .is_none()
    );
    batch.flush(|v| seen.push(v.value));
    assert_eq!(seen.len(), 4);
}
