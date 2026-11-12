//! The advertised source must answer every D-32 XTGETTCAP name.

use super::value;

/// Boolean and string capabilities share the compiled source of truth.
#[test]
fn every_advertised_name_has_a_wire_value() {
    for name in [
        "Sync", "RGB", "Tc", "Smulx", "Setulc", "Ss", "Se", "Cs", "Cr", "Ms", "Smol", "Rmol", "E3",
        "BE", "BD", "PS", "PE", "hs", "tsl", "fsl", "XT",
    ] {
        assert!(value(name.as_bytes()).is_some(), "missing {name}");
    }
    assert_eq!(value(b"BE"), Some(b"\x1b[?2004h".to_vec()));
    assert_eq!(value(b"fsl"), Some(vec![7]));
    assert_eq!(value(b"unsupported"), None);
}
