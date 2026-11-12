//! Public protocol/storage witnesses use deterministic decoder completions.
//! Real codec bytes are tested by the native worker; core tests pin ordered
//! actions, atomic failure, placement identities and bounded source lifetime.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests assert protocol outcomes and fixture ownership"
)]
use super::*;
use crate::vt::{Terminal, terminal::ActiveScreen, tracked::PositionState};
use std::sync::Arc;

/// Complete a one-pixel direct transmission without introducing a native dependency.
pub(crate) fn transmit(store: &mut Store, terminal: &mut Terminal, command: &[u8]) {
    store.command(command, terminal);
    let request = store.take_request().expect("decode request");
    store.complete(
        DecodeResult {
            ticket: request.ticket,
            image: Ok(DecodedImage {
                size: [1, 1],
                rgba: Arc::from([255, 0, 0, 255]),
            }),
        },
        terminal,
    );
}

/// Transmit/put/query/replace, chunking and quiet replies preserve source identities.
#[test]
fn actions_chunking_quiet_and_atomic_failure() {
    let mut terminal = Terminal::new(10, 4, 4, 4096);
    let mut store = Store::new(4096, &terminal);
    store.command(b"a=T,i=1,p=4,f=32,s=1,v=1,C=1,m=1;", &mut terminal);
    assert!(!store.decode_pending());
    terminal.advance(b"ab");
    store.command(b"m=0;/wAA/w==", &mut terminal);
    let request = store.take_request().unwrap();
    assert_eq!(request.payload, [255, 0, 0, 255]);
    store.complete(
        DecodeResult {
            ticket: request.ticket,
            image: Ok(DecodedImage {
                size: [1, 1],
                rgba: Arc::from(request.payload),
            }),
        },
        &mut terminal,
    );
    assert_eq!(terminal.cursor.column, 2);
    assert_eq!(
        terminal.tracked.get(store.placements()[0].anchor),
        PositionState::Present {
            line: terminal.screen().row(0).unwrap().id,
            column: 2
        }
    );
    assert_eq!(terminal.replies, b"\x1b_Gi=1,p=4;OK\x1b\\");
    terminal.replies.clear();
    // Query validates without replacing the image or any of its placements.
    transmit(&mut store, &mut terminal, b"a=q,i=1,s=1,v=1;/wAA/w==");
    assert_eq!(store.placements().len(), 1);
    let generation = store.images()[0].generation;
    store.command(b"i=1,s=1,v=1;!!!!", &mut terminal);
    assert_eq!(store.images()[0].generation, generation);
    store.command(b"i=1,s=1,v=1;/wAA/w==", &mut terminal);
    let request = store.take_request().unwrap();
    store.complete(
        DecodeResult {
            ticket: request.ticket,
            image: Err(ImageError::Invalid),
        },
        &mut terminal,
    );
    assert_eq!(store.images()[0].generation, generation);
    terminal.replies.clear();
    transmit(&mut store, &mut terminal, b"i=2,q=1,s=1,v=1;/wAA/w==");
    assert!(terminal.replies.is_empty());
    store.command(b"a=p,i=99,q=2", &mut terminal);
    assert!(terminal.replies.is_empty());
    transmit(&mut store, &mut terminal, b"i=1,s=1,v=1;/wAA/w==");
    assert!(store.placements().is_empty());
    assert_eq!(store.decoded_bytes(), 8);
}

/// Both transmit-and-display and a later put advance by the full c/r rectangle.
#[test]
fn placement_cursor_policy_moves_both_axes_or_preserves_cursor() {
    for action in *b"Tp" {
        for policy in ["", "C=0,", "C=1,"] {
            let mut terminal = Terminal::new(12, 8, 4, 4096);
            let mut store = Store::new(4096, &terminal);
            terminal.advance(b"\x1b[2;3H");
            let origin = terminal.cursor_line().unwrap();
            if action == b'T' {
                transmit(
                    &mut store,
                    &mut terminal,
                    format!("a=T,i=1,{policy}c=3,r=2,s=1,v=1;/wAA/w==").as_bytes(),
                );
            } else {
                transmit(&mut store, &mut terminal, b"i=1,s=1,v=1;/wAA/w==");
                store.command(format!("a=p,i=1,{policy}c=3,r=2").as_bytes(), &mut terminal);
            }
            let expected = if policy == "C=1," { (1, 2) } else { (3, 5) };
            assert_eq!((terminal.cursor.row, terminal.cursor.column), expected);
            let placement = &store.placements()[0];
            assert_eq!(placement.cells, [3, 2]);
            assert_eq!(
                terminal.tracked.get(placement.anchor),
                PositionState::Present {
                    line: origin,
                    column: 2
                }
            );
        }
    }
}

/// Every selector retires only matching instances, including siblings of one image.
#[test]
fn every_delete_selector_preserves_unselected_placements() {
    for selector in *b"aincpqrxyz" {
        for free in [false, true] {
            let mut terminal = Terminal::new(10, 4, 4, 4096);
            let mut store = Store::new(4096, &terminal);
            transmit(
                &mut store,
                &mut terminal,
                b"a=T,I=7,p=1,C=1,s=1,v=1,z=2;/wAA/w==",
            );
            terminal.advance(b"\x1b[3;4H");
            store.command(b"a=p,i=1,p=2,C=1,z=3", &mut terminal);
            terminal.advance(b"\x1b[H");
            let key = if free {
                selector.to_ascii_uppercase()
            } else {
                selector
            } as char;
            let identity = if selector == b'n' { "I=7" } else { "i=1" };
            let extra = match selector {
                b'r' => "x=1,y=1",
                b'a' => "x=1,y=1",
                _ => "x=1,y=1,p=1,z=2",
            };
            store.command(
                format!("a=d,d={key},{identity},{extra}").as_bytes(),
                &mut terminal,
            );
            let all = matches!(selector, b'a' | b'r');
            assert_eq!(store.placements().len(), usize::from(!all), "{key}");
            assert_eq!(store.images().len(), usize::from(!(all && free)), "{key}");
        }
    }
}

/// Reflow and screen switches retain anchors; ED/RIS and history eviction retire them.
#[test]
fn placement_screen_reflow_clear_and_history_lifetime() {
    let mut terminal = Terminal::new(8, 2, 2, 4096);
    let mut store = Store::new(4096, &terminal);
    terminal.advance(b"abcdefghij");
    transmit(&mut store, &mut terminal, b"a=T,i=1,C=1,s=1,v=1;/wAA/w==");
    terminal.advance(b"\x1b[?1049h");
    store.reconcile(&mut terminal);
    transmit(&mut store, &mut terminal, b"a=T,i=2,C=1,s=1,v=1;/wAA/w==");
    terminal.resize(4, 2);
    store.reconcile(&mut terminal);
    assert_eq!(store.placements().len(), 2);
    assert_eq!(store.placements()[0].screen, ActiveScreen::Primary);
    terminal.advance(b"\x1b[2J");
    store.reconcile(&mut terminal);
    assert_eq!(store.placements().len(), 1);
    terminal.advance(b"\x1b[?1049l\n\n\n\n\n");
    store.reconcile(&mut terminal);
    assert!(store.placements().is_empty());
    terminal.advance(b"\x1bc");
    store.reconcile(&mut terminal);
    assert!(store.images().is_empty());
}

/// Nonvisible LRU sources go first; too-large completions cannot evict valid pixels.
#[test]
fn decoded_budget_eviction_and_zero_disable() {
    let mut terminal = Terminal::new(10, 4, 4, 4096);
    let mut store = Store::new(4096, &terminal);
    transmit(&mut store, &mut terminal, b"a=T,i=1,C=1,s=1,v=1;/wAA/w==");
    transmit(&mut store, &mut terminal, b"i=2,s=1,v=1;/wAA/w==");
    store.set_limit(4, &mut terminal);
    assert_eq!(store.images()[0].id, 1);
    assert_eq!(store.decoded_bytes(), 4);
    store.command(b"i=3,s=2,v=1;/wAA/w==", &mut terminal);
    assert!(!store.decode_pending());
    assert_eq!(store.images()[0].id, 1);
    store.set_limit(0, &mut terminal);
    assert!(store.images().is_empty());
    store.command(b"i=3,s=1,v=1;/wAA/w==", &mut terminal);
    assert!(!store.decode_pending());
}

proptest::proptest! {
    /// Invalid payloads cannot create partially decoded sources or dangling placements.
    #[test]
    fn malformed_transmissions_leave_no_partial_state(payload in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..5000)) {
        let mut terminal = Terminal::new(2, 1, 0, 0);
        let mut store = Store::new(1024, &terminal);
        let mut body = b"i=1,s=1,v=1;".to_vec();
        body.extend_from_slice(&payload);
        store.command(&body, &mut terminal);
        proptest::prop_assert!(store.images().is_empty());
        proptest::prop_assert!(store.placements().is_empty());
        if let Some(request) = store.take_request() { proptest::prop_assert!(request.payload.len() <= 1024); }
    }
}

/// Explicit scope rejections produce errors and respect both quiet modes.
#[test]
fn unsupported_features_and_continuation_errors_reply_once() {
    let mut terminal = Terminal::new(10, 4, 0, 0);
    let mut store = Store::new(4096, &terminal);
    for feature in ["t=f", "t=t", "t=s", "a=f", "a=a", "a=c", "P=1", "Q=2"] {
        store.command(format!("i=3,{feature};AAAA").as_bytes(), &mut terminal);
        assert_eq!(
            terminal.replies,
            b"\x1b_Gi=3;ENOTSUP:unsupported graphics feature\x1b\\"
        );
        terminal.replies.clear();
        store.command(format!("i=3,q=2,{feature};AAAA").as_bytes(), &mut terminal);
        assert!(terminal.replies.is_empty());
        assert!(!store.decode_pending());
    }
    store.command(b"i=7,s=1,v=1,m=1;", &mut terminal);
    store.command(b"m=0,q=2;!!!!", &mut terminal);
    assert!(terminal.replies.is_empty());
    store.command(b"i=7,s=1,v=1,m=1;", &mut terminal);
    store.command(b"m=0;!!!!", &mut terminal);
    assert_eq!(
        terminal.replies,
        b"\x1b_Gi=7;EINVAL:invalid graphics request\x1b\\"
    );
    assert!(store.images().is_empty());
}

/// Full placement metadata cannot prevent replacement, or allow partial source mutation.
#[test]
fn placement_metadata_replacement_stays_atomic_at_capacity() {
    let mut terminal = Terminal::new(10, 4, 0, 0);
    let mut store = Store::new(std::mem::size_of::<Placement>() * 2, &terminal);
    transmit(
        &mut store,
        &mut terminal,
        b"a=T,i=1,p=1,C=1,s=1,v=1;/wAA/w==",
    );
    store.command(b"a=p,i=1,p=2,C=1", &mut terminal);
    store.command(b"a=p,i=1,p=1,C=1", &mut terminal);
    assert_eq!(store.placements().len(), 2);
    let generation = store.images()[0].generation;
    terminal.replies.clear();
    store.command(b"a=p,i=1,p=3,C=1", &mut terminal);
    assert!(terminal.replies.starts_with(b"\x1b_Gi=1,p=3;ENOSPC"));
    assert_eq!(store.images()[0].generation, generation);
    transmit(
        &mut store,
        &mut terminal,
        b"a=T,i=1,p=4,C=1,s=1,v=1;/wAA/w==",
    );
    assert_eq!(store.placements().len(), 1);
    assert_eq!(store.placements()[0].id, 4);
}

/// Exact hardware footprints and visibility-aware admission remain atomic.
mod admission;
