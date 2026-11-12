//! Bridge output and API writes share the parser but not the ACK ledger.
#![allow(
    clippy::unwrap_used,
    reason = "ledger assertions fail on unexpected attribution errors"
)]
use super::*;

/// A promise barrier cannot complete before a split reply-bearing prefix parses.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn barriers_and_ack_follow_parsed_prefixes() {
    let mut parsed = Parsed::default();
    let first = parsed.accept(80_000, Some(1));
    let api = parsed.accept(10, None);
    parsed.accept(70_000, Some(1));
    assert!(parsed.advance(32_000).unwrap().is_empty());
    assert!(parsed.consumed < first);
    assert_eq!(parsed.advance(48_000).unwrap(), vec![(1, 80_000)]);
    assert_eq!(parsed.consumed, first);
    assert!(parsed.advance(10).unwrap().is_empty());
    assert_eq!(parsed.consumed, api);
    assert_eq!(parsed.advance(70_000).unwrap(), vec![(1, 70_000)]);
    assert!(parsed.advance(1).is_err());
}
