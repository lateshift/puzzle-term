//! Closure evidence must expose old-display waits and preserve current receipts.
use super::*;

#[test]
fn crossing_wait_is_not_hidden_by_later_geometry_update() {
    // The OS call started on display 1, then ended after the host moved to 3.
    let log = "2026-10-10T13:00:00.000000Z event=\"desktop.render_geometry\" display: 1,\n\
        2026-10-10T13:00:00.000001Z event=\"desktop.host_moved\" display: 1,\n\
        2026-10-10T13:00:00.000002Z event=\"desktop.host_moved\" display: 3,\n\
        2026-10-10T13:00:00.010000Z stage=\"drawable_acquire\" wall_ms=12\n\
        2026-10-10T13:00:00.010001Z event=\"desktop.render_geometry\" display: 3,";
    assert!(crossing_trace(log).is_err_and(|error| error.contains("old-display acquisition")));
}

#[test]
#[ignore = "requires retained local M17 physical display recordings"]
fn retained_refresh_recordings_keep_all_bounds_and_sources() -> Result<(), String> {
    // Cargo normally starts in xtask/, whereas the gate runs at the workspace root.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("workspace root absent")?;
    std::env::set_current_dir(root).map_err(|error| error.to_string())?;
    let mut passed = true;
    run(&mut passed);
    assert!(
        passed,
        "a retained physical receipt failed its original contract"
    );
    Ok(())
}
