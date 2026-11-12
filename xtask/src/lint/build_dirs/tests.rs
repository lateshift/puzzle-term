//! Artifact placement must accept workspace target and reject even empty crate targets.
use super::check;
use std::{fs, path::Path};

/// Reproduces the stray screenshot directory without touching actual crate artifacts.
#[test]
fn rejects_crate_targets_but_allows_workspace_target() -> Result<(), String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../target/lint-build-dirs")
        .join(std::process::id().to_string());
    let crates = root.join("crates");
    fs::create_dir_all(crates.join("example/src")).map_err(|error| error.to_string())?;
    fs::create_dir_all(root.join("target")).map_err(|error| error.to_string())?;
    let result = (|| {
        check(&crates)?;
        fs::create_dir_all(crates.join("example/target")).map_err(|error| error.to_string())?;
        let Err(error) = check(&crates) else {
            return Err("an empty crate-local target must fail lint".into());
        };
        assert!(error.contains("example/target"), "{error}");
        assert!(error.contains("workspace target/"), "{error}");
        Ok(())
    })();
    // Cleanup is unconditional so a failed witness leaves no fixture that a later run could reuse.
    fs::remove_dir_all(root).map_err(|error| error.to_string())?;
    result
}
