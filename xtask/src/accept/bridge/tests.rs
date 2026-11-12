//! U2 scheduling must not hide an integration witness from milestone closure.
#![allow(
    clippy::unwrap_used,
    reason = "malformed test inventories fail directly"
)]
use super::*;

/// Exercise the real CLI policy and complete socket inventory without spawning peers.
#[test]
fn task_skip_names_all_bridge_witnesses() {
    let args = ["M16".into(), "--skip-browser".into()];
    let (_, gate) = super::super::browser_gate::parse(&args).unwrap();
    let mut passed = true;
    closure(gate, &mut passed);
    assert!(passed);
}

/// A newly ignored socket test must also be registered in the closure inventory.
#[test]
fn every_ignored_bridge_test_has_a_closure_witness() {
    let inventory = include_str!("../bridge.rs");
    for source in [
        include_str!("../../../../crates/puzzle-pty-bridge/tests/input.rs"),
        include_str!("../../../../crates/puzzle-pty-bridge/tests/security.rs"),
        include_str!("../../../../crates/puzzle-pty-bridge/tests/transport.rs"),
        include_str!("../../../../crates/puzzle-pty-bridge/tests/flow.rs"),
    ] {
        for test in source.split("#[test]\n").skip(1) {
            assert!(test.starts_with("#[ignore = \"D-46/U2:"));
            let name = test.split("fn ").nth(1).unwrap().split('(').next().unwrap();
            assert!(inventory.contains(&format!("\"{name}\"")), "missing {name}");
        }
    }
}
