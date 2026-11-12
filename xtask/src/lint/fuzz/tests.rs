//! Finite invocation and isolated dependency decisions stay mechanically checked.
use super::*;
#[test]
fn fuzz_catalogue_rejects_unreviewed_versions_or_features() {
    let data = include_str!("../../../../fuzz/Cargo.toml");
    assert_eq!(manifest(Path::new("fuzz/Cargo.toml"), data), Ok(true));
    assert!(
        manifest(
            Path::new("fuzz/Cargo.toml"),
            &data.replace("=0.4.13", "=0.5.0")
        )
        .is_err()
    );
    assert!(
        manifest(
            Path::new("fuzz/Cargo.toml"),
            &data.replace("publish = false", "publish = true")
        )
        .is_err()
    );
    assert_eq!(
        manifest(Path::new("crates/puzzle-vt/Cargo.toml"), data),
        Ok(false)
    );
}
