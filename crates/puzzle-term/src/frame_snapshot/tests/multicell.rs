//! Publication-local OSC 66 payloads survive live slot reuse and clipped roots.
#![allow(
    clippy::expect_used,
    reason = "test setup and assertions identify missing sealed records"
)]
use super::*;

/// Reusing slot zero in VT must not change an already sealed heading.
#[test]
fn text_sizing_sealed_payload_and_warm_capture() {
    let mut terminal = Terminal::new(12, 5, 10, 32_768);
    terminal.advance(b"\x1b]66;s=2:w=2;OLD\x07");
    let mut snapshot = FrameSnapshot::default();
    snapshot.capture(&terminal, 0).expect("sealed heading");
    assert_eq!(snapshot.blocks().next().expect("record").value.text, "OLD");
    // Force recapture to test copies rather than merely an unchanged cache hit.
    let allocations = allocation_counter::measure(|| {
        snapshot.invalidate();
        snapshot.capture(&terminal, 0).expect("warm recapture");
    });
    assert_eq!(allocations.count_total, 0);
    terminal.advance(b"\x1b[H\x1b[2J\x1b]66;s=2:w=2;NEW\x07");
    assert_eq!(
        snapshot.blocks().next().expect("sealed record").value.text,
        "OLD"
    );
    snapshot.capture(&terminal, 0).expect("replacement");
    assert_eq!(
        snapshot.blocks().next().expect("new record").value.text,
        "NEW"
    );
}

/// A lower fragment changes ink-owner role without changing its VT version.
#[test]
fn text_sizing_clipped_root_invalidates_owner_role() {
    let mut terminal = Terminal::new(12, 4, 10, 32_768);
    terminal.advance(b"\x1b]66;s=3;A\x07\x1b[4;1H\r\n");
    let mut snapshot = FrameSnapshot::default();
    snapshot.capture_viewport(&terminal, 0, 0).expect("root");
    let first = snapshot.row_meta(1).expect("second row");
    snapshot
        .capture_viewport(&terminal, 0, 1)
        .expect("lower owner");
    let block = snapshot.blocks().next().expect("clipped record");
    assert_eq!((block.row, block.offset), (0, 1));
    let next = snapshot.row_meta(0).expect("first visible row");
    assert_eq!(first.id, next.id);
    assert_eq!(first.version, next.version);
    assert_ne!(first.block_signature, next.block_signature);
}

/// Slot keys that happen to equal blank scalar values still schedule blinking ink.
#[test]
fn text_sizing_blink_keys_are_not_blank_scalars() {
    let mut terminal = Terminal::new(40, 2, 0, 0);
    for _ in 0..32 {
        terminal.advance(b"\x1b]66;s=1;A\x07");
    }
    terminal.advance(b"\x1b[5m\x1b]66;s=1;B\x07");
    let mut snapshot = FrameSnapshot::default();
    snapshot.capture(&terminal, 0).expect("blink publication");
    assert!(snapshot.has_blinking_ink());
    terminal.advance(b"\x1b[H\x1b[2J\x1b[5m\x1b]66;s=2;C\x07");
    snapshot.capture(&terminal, 0).expect("reused blink slot");
    assert!(snapshot.has_blinking_ink());
}
