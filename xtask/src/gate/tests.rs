//! Independent file descriptions must exclude one another until the guard drops.
use super::*;

/// A retained file excludes another native/browser gate and releases on drop.
#[test]
fn gate_releases_without_deleting_its_file() -> Result<(), String> {
    let path = std::env::temp_dir().join(format!("puzzle-gate-test-{}", std::process::id()));
    let guard = lock_file(&path)?;
    let next = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|error| error.to_string())?;
    assert!(matches!(
        next.try_lock(),
        Err(std::fs::TryLockError::WouldBlock)
    ));
    drop(guard);
    next.try_lock().map_err(|error| error.to_string())?;
    drop(next);
    fs::remove_file(path).map_err(|error| error.to_string())?;
    Ok(())
}

/// Child helpers invoked by acceptance must not reacquire their parent's guard.
#[test]
fn nested_helpers_do_not_lock() -> Result<(), String> {
    for args in [vec!["harness"], vec!["web", "build"], vec!["lint"]] {
        assert!(acquire(&args.into_iter().map(str::to_owned).collect::<Vec<_>>())?.is_none());
    }
    Ok(())
}

/// A fork/exec window may retain a descriptor after the parent ends its gate.
#[test]
fn explicit_unlock_retires_the_gate_with_a_surviving_descriptor() -> Result<(), String> {
    let path =
        std::env::temp_dir().join(format!("puzzle-gate-inherited-test-{}", std::process::id()));
    let guard = lock_file(&path)?;
    let inherited = guard.file.try_clone().map_err(|error| error.to_string())?;
    let next = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|error| error.to_string())?;
    assert!(matches!(
        next.try_lock(),
        Err(std::fs::TryLockError::WouldBlock)
    ));
    // Simulate the parent's completion while the child's duplicate is still open.
    drop(guard);
    next.try_lock().map_err(|error| error.to_string())?;
    drop(next);
    drop(inherited);
    fs::remove_file(path).map_err(|error| error.to_string())?;
    Ok(())
}
