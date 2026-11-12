//! A nested executable or Cargo library must never enter the client package.
#[test]
fn rejects_nested_bridge_artifacts_but_accepts_client_assets()
-> Result<(), Box<dyn std::error::Error>> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let root = std::env::temp_dir().join(format!("puzzle-package-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(root.join("nested"))?;
    std::fs::write(root.join("puzzle_web.wasm"), b"client")?;
    assert!(super::inspect(&root).is_ok());
    for artifact in ["puzzle-pty-bridge.exe", "libpuzzle_pty_bridge-hash.rlib"] {
        let path = root.join("nested").join(artifact);
        std::fs::write(&path, b"native development tool")?;
        assert!(super::inspect(&root).is_err_and(|error| error.contains(artifact)));
        std::fs::remove_file(path)?;
    }
    let fixture = root.join("nested/test_glyphs-glyf_colr_1.ttf");
    std::fs::write(&fixture, b"approved test-only font")?;
    assert!(super::inspect(&root).is_err_and(|e| e.contains("test font fixture")));
    std::fs::remove_dir_all(root)?;
    Ok(())
}
