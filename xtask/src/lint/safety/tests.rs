//! R8 contracts and R9 display-link dependencies cannot cross platform boundaries.
use super::*;

#[test]
fn foreign_dependency_requires_the_correct_owner_and_target() {
    let target =
        "[target.'cfg(target_os = \"macos\")'.dependencies]\nobjc2-core-video.workspace = true";
    assert!(manifest(Path::new("crates/puzzle-sys/Cargo.toml"), target).is_ok());
    assert!(manifest(Path::new("crates/puzzle-term/Cargo.toml"), target).is_err());
    assert!(
        manifest(
            Path::new("crates/puzzle-sys/Cargo.toml"),
            "[dependencies]\nobjc2-core-video.workspace = true"
        )
        .is_err()
    );
}

#[test]
fn every_foreign_block_has_its_own_adjacent_contract() {
    let path = Path::new("crates/puzzle-sys/src/display_link/macos.rs");
    assert!(
        source(
            path,
            "// SAFETY: pointer is live.\n// It remains retained.\nlet x = unsafe { read() };\n"
        )
        .is_ok()
    );
    assert!(
        source(
            path,
            "// SAFETY: first pointer.\nlet x = unsafe { read() };\nlet y = unsafe { read() };\n"
        )
        .is_err()
    );
    assert!(
        source(
            path,
            "// SAFETY: stale paragraph.\n\nlet x = unsafe { read() };\n"
        )
        .is_err()
    );
    assert!(source(path, "let x = unsafe\n{ read() };\n").is_err());
    assert!(!exception(Path::new("crates/puzzle-term/src/session.rs")));
}

#[test]
fn layer_frameworks_require_sys_and_macos() {
    for name in [
        "objc2-quartz-core",
        "objc2-core-graphics",
        "objc2",
        "block2",
        "objc2-metal",
        "objc2-io-surface",
        "objc2-core-foundation",
    ] {
        let target =
            format!("[target.'cfg(target_os = \"macos\")'.dependencies]\n{name}.workspace = true");
        assert!(manifest(Path::new("crates/puzzle-sys/Cargo.toml"), &target).is_ok());
        assert!(manifest(Path::new("crates/puzzle-desktop/Cargo.toml"), &target).is_err());
        assert!(
            manifest(
                Path::new("crates/puzzle-sys/Cargo.toml"),
                &format!("[dependencies]\n{name}.workspace = true")
            )
            .is_err()
        );
    }
}

#[test]
fn metal_hal_is_only_a_macos_sys_dependency() {
    let path = Path::new("crates/puzzle-sys/Cargo.toml");
    assert!(
        manifest(
            path,
            "[target.'cfg(target_os = \"macos\")'.dependencies]\nwgpu.workspace = true"
        )
        .is_ok()
    );
    assert!(manifest(path, "[dependencies]\nwgpu.workspace = true").is_err());
}
