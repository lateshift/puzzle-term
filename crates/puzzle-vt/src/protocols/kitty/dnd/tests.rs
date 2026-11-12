//! OSC 72 executable wire contract; native data enters only through a real drop.
#![allow(
    clippy::expect_used,
    reason = "test fixtures fail with a useful contract message"
)]
use crate::{
    Terminal,
    clipboard::MimeData,
    dnd::{Effect, Input},
    terminal::TerminalEvent,
};
/// Feed the real bounded parser, including split OSC and interleaved queries.
fn term(bytes: &[u8]) -> Terminal {
    let mut term = Terminal::new(80, 24, 10, 100_000);
    term.advance(bytes);
    term
}
/// Offer a binary representation so reads must preserve bytes rather than UTF-8 text.
fn items(bytes: &[u8]) -> Vec<MimeData> {
    vec![MimeData {
        mime: "application/octet-stream".into(),
        data: bytes.into(),
    }]
}
#[test]
fn registration_chunks_query_order_and_reset_lifetime() {
    let mut t = term(b"\x1b]72;t=q:i=7\x07\x1b[c");
    assert_eq!(t.replies, b"\x1b]72;t=q:i=7;\x07\x1b[?62;22c");
    assert!(
        t.host_protocols.dnd.state.is_none(),
        "query must not allocate a target"
    );
    t.advance(
        b"\x1b]72;t=a:i=4:m=1;text/\x1b\\ordinary\x1b]72;t=q:i=9\x1b\\\x1b]72;m=0;plain\x1b\\",
    );
    assert!(
        matches!(&t.events.last(),Some(TerminalEvent::Drop(Effect::Registration {generation:1,mimes:Some(m) })) if m=="text/plain")
    );
    assert_eq!(
        t.screen()
            .row(0)
            .expect("valid fixture")
            .cells
            .get(0)
            .expect("valid fixture")
            .ch,
        'o' as u32
    );
    t.advance(b"\x1bc");
    assert!(t.host_protocols.dnd.state.is_none());
    assert!(matches!(
        t.events.last(),
        Some(TerminalEvent::Drop(Effect::Registration {
            mimes: None,
            ..
        }))
    ));
    t.advance(b"\x1b]72;t=a;\x1b\\");
    assert_eq!(t.host_protocols.dnd.generation, 2);
    assert!(
        !t.drop_input(
            1,
            Input::Drop {
                position: [0; 4],
                operations: 1,
                items: items(b"secret")
            }
        )
        .expect("valid fixture")
    );
    t.advance(b"\x1b]72;t=A\x1b\\");
    assert!(t.host_protocols.dnd.state.is_none());
}
#[test]
fn hover_never_grants_reads_leave_and_conclusion_release_data() {
    let mut t = term(b"\x1b]72;t=a:i=8;\x1b\\");
    let names = items(b"");
    t.drop_input(
        1,
        Input::Move {
            position: [2, 3, 20, 60],
            operations: 3,
            items: names.clone(),
        },
    )
    .expect("valid fixture");
    assert_eq!(
        t.replies,
        b"\x1b]72;t=m:x=2:y=3:X=20:Y=60:o=3:m=0:i=8;application/octet-stream\x1b\\"
    );
    t.replies.clear();
    t.advance(b"\x1b]72;t=r:x=1\x1b\\");
    assert!(t.replies.ends_with(b";EPERM\x1b\\"));
    t.replies.clear();
    t.drop_input(
        1,
        Input::Move {
            position: [3, 3, 30, 60],
            operations: 3,
            items: names,
        },
    )
    .expect("valid fixture");
    assert!(t.replies.ends_with(b"i=8;\x1b\\"));
    t.drop_input(1, Input::Leave).expect("valid fixture");
    assert!(t.replies.ends_with(b"\x1b]72;t=m:x=-1:y=-1:i=8;\x1b\\"));
}
#[test]
fn binary_drop_fifo_and_conclusion_release_data() {
    let mut t = term(b"\x1b]72;t=a:i=8;\x1b\\");
    t.drop_input(
        1,
        Input::Drop {
            position: [0; 4],
            operations: 1,
            items: items(&[0, 255, 1]),
        },
    )
    .expect("valid fixture");
    t.replies.clear();
    t.advance(b"\x1b]72;t=r:x=1\x07\x1b[5n");
    assert_eq!(
        t.replies,
        b"\x1b]72;t=r:x=1:m=0:i=8;AP8B\x07\x1b]72;t=r:x=1:m=0:i=8;\x07\x1b[0n"
    );
    t.advance(b"\x1b]72;t=r:o=1\x1b\\");
    assert!(
        t.host_protocols
            .dnd
            .state
            .as_ref()
            .expect("valid fixture")
            .items
            .is_none()
    );
    assert!(matches!(
        t.events.last(),
        Some(TerminalEvent::Drop(Effect::Concluded { operation: 1, .. }))
    ));
}
#[test]
fn binary_chunks_remote_refusal_and_atomic_bounds() {
    let mut t = term(b"\x1b]72;t=a;\x1b\\");
    assert!(
        t.drop_input(
            1,
            Input::Move {
                position: [0; 4],
                operations: 1,
                items: items(b"bad")
            }
        )
        .is_err()
    );
    assert!(
        t.drop_input(
            1,
            Input::Drop {
                position: [0; 4],
                operations: 1,
                items: vec![items(b"")[0].clone(); 17]
            }
        )
        .is_err()
    );
    t.drop_input(
        1,
        Input::Drop {
            position: [0; 4],
            operations: 1,
            items: items(&vec![b'A'; 3073]),
        },
    )
    .expect("valid fixture");
    t.replies.clear();
    t.advance(b"\x1b]72;t=r:x=1\x1b\\");
    let text = std::str::from_utf8(&t.replies).expect("valid fixture");
    let packets: Vec<_> = text.split("\x1b]72;").skip(1).collect();
    assert_eq!(packets.len(), 3);
    assert!(packets[0].starts_with("t=r:x=1:m=1;"));
    assert_eq!(
        packets[0].split_once(';').expect("valid fixture").1.len(),
        4096 + 2
    );
    assert_eq!(packets[1], "t=r:x=1:m=0;QQ==\x1b\\");
    assert_eq!(packets[2], "t=r:x=1:m=0;\x1b\\");
    t.replies.clear();
    t.advance(b"\x1b]72;t=r:x=1:y=1\x1b\\");
    assert!(t.replies.ends_with(b";EINVAL\x1b\\"));
    assert!(
        t.host_protocols
            .dnd
            .state
            .as_ref()
            .expect("valid fixture")
            .items
            .is_none()
    );
}
#[test]
fn oversized_malformed_packets_do_not_swallow_following_output() {
    let mut t = term(b"\x1b]72;t=a;\x1b\\");
    let mut packet = b"\x1b]72;t=a;".to_vec();
    packet.extend(std::iter::repeat_n(b'x', 4097));
    packet.extend_from_slice(b"\x1b\\ok");
    t.advance(&packet);
    assert!(t.replies.ends_with(b";EFBIG\x1b\\"));
    assert_eq!(
        t.screen()
            .row(0)
            .expect("valid fixture")
            .cells
            .get(0)
            .expect("valid fixture")
            .ch,
        'o' as u32
    );
    t.advance(b"\x1b]72;t=r:x=4294967296\x1b\\\x1b]72;t=o\x1b\\");
    assert!(t.replies.ends_with(b";EPERM\x1b\\"));
}

#[test]
fn unused_grid_and_support_query_have_no_drop_storage_or_warm_allocations() {
    let mut t = term(b"warm\r\x1b]72;t=q\x1b\\");
    t.replies.clear();
    let counts = allocation_counter::measure(|| {
        for _ in 0..100 {
            t.advance(b"ASCII\r");
        }
    });
    assert_eq!(counts.count_total, 0);
    assert!(t.host_protocols.dnd.state.is_none());
}

#[test]
fn aggregate_chunk_cap_and_duplicate_mimes_are_atomic() {
    // A complete 1-MiB registration is legal even across 4096-byte packets.
    let mut t = term(b"\x1b]72;t=a:m=1;text/plain\x1b\\");
    let mut continuation = b"\x1b]72;m=1;".to_vec();
    continuation.extend(std::iter::repeat_n(b' ', 4096));
    continuation.extend_from_slice(b"\x1b\\");
    for _ in 0..255 {
        t.advance(&continuation);
    }
    // The next packet crosses the aggregate bound, independently of packet size.
    t.advance(&continuation);
    assert!(t.replies.ends_with(b";EFBIG\x1b\\"));
    assert!(
        t.host_protocols
            .dnd
            .state
            .as_ref()
            .expect("partial owner")
            .partial
            .is_none()
    );
    t.advance(b"\x1b]72;t=a;\x1b\\");
    let duplicate = vec![items(b"")[0].clone(); 2];
    assert!(
        t.drop_input(
            1,
            Input::Drop {
                position: [0; 4],
                operations: 1,
                items: duplicate
            }
        )
        .is_err()
    );
    assert!(
        t.host_protocols
            .dnd
            .state
            .as_ref()
            .expect("owner")
            .items
            .is_none()
    );
    // One shared allocation supplies an oversized representation; no partial data survives.
    let oversized = items(&vec![0; crate::dnd::MAX_BYTES + 1]);
    assert!(
        t.drop_input(
            1,
            Input::Drop {
                position: [0; 4],
                operations: 1,
                items: oversized
            }
        )
        .is_err()
    );
    t.advance(b"\x1b]72;t=r:x=1\x1b\\");
    assert!(t.replies.ends_with(b";EPERM\x1b\\"));
}
