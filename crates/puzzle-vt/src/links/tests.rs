//! OSC 8 link ceiling and URI policy checks (design §6.11, §16).
#![allow(
    clippy::expect_used,
    reason = "tests require a link insertion to succeed"
)]

use super::LinkTable;

/// A recycled slot must not make an old cell open a different link.
#[test]
fn stale_generation_does_not_resolve() {
    let mut links = LinkTable::default();
    let old = links
        .insert("https://example.org/old", None)
        .expect("old link");
    for index in 0..4096 {
        links.insert(&format!("https://example.org/{index}"), None);
    }
    assert_eq!(links.resolve(old), None);
}

/// Script URLs remain visible as text but cannot be launched by policy.
#[test]
fn scheme_policy_rejects_script_and_web_file() {
    assert!(!LinkTable::openable("javascript:alert(1)", true));
    assert!(!LinkTable::openable("file:///tmp/example", false));
    assert!(LinkTable::openable("https://example.org", false));
}
