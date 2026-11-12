//! Mode-table checks for DECRQM's supported/reset distinction (§6.5).

use super::Modes;

/// xterm ctlseqs reports advertised modes as 1 or 2 and unknown as 0.
#[test]
fn all_advertised_modes_are_queryable() {
    let modes = Modes::default();
    for number in [4, 20] {
        assert!(modes.get(false, number).is_some());
    }
    for number in [
        1, 3, 5, 6, 7, 12, 25, 45, 47, 66, 69, 1047, 1048, 1049, 9, 1000, 1002, 1003, 1004, 1005,
        1006, 1015, 1016, 1007, 1036, 1039, 2004, 2026, 2027, 2031, 2048,
    ] {
        assert!(modes.get(true, number).is_some(), "missing ?{number}");
    }
    assert_eq!(modes.get(true, 9000), None);
}

/// §7.4 enables alternate scrolling before an application overrides mode 1007.
#[test]
fn alternate_scroll_starts_enabled() {
    assert_eq!(Modes::default().get(true, 1007), Some(true));
}
