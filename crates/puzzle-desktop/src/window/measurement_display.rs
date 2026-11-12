//! Opt-in monitor placement for reviewer-requested 60/120-Hz comparisons (M8.8).
//! Winit's physical position/size and millihertz identify the actual window's display.
//! No display-specific pacing exists: LG/tiled and built-in screens use one Pacer.
//! Normal windows retain OS placement. A missing requested display fails explicitly
//! rather than recording on a default screen and calling it a 120-Hz measurement.
//!
//! The user authorized placing the owned diagnostic window on the opened built-in
//! display. This path never changes the display mode or moves a normal terminal.
//! ProMotion can advertise 120 Hz while adapting its actual cadence; placement
//! therefore supplies hardware metadata, with the separate FIFO preflight checking
//! the measured period before collecting 120-Hz scroll evidence.
//! Every recording queries current_monitor again after this asynchronous move.
use winit::{event_loop::ActiveEventLoop, window::Window};

/// Places a diagnostic before AppKit creates it, so maximize uses the chosen display.
/// Moving a created window then immediately maximizing can maximize its old screen.
/// The later visibility witness must still confirm the requested display and geometry.
pub(super) fn attributes(
    event_loop: &ActiveEventLoop,
    attributes: winit::window::WindowAttributes,
) -> Result<winit::window::WindowAttributes, String> {
    // Ordinary shells leave position selection to the OS, including restored windows.
    if std::env::var_os("PUZZLE_UNICODE_PROBE").is_none() {
        return Ok(attributes);
    }
    // Millihertz avoids rounding a monitor's advertised rate before selection.
    // Unicode/flood probes default to the LG; the built-in run opts into 120 Hz.
    let requested = std::env::var("PUZZLE_SCROLL_PROBE_MILLIHZ")
        .unwrap_or_else(|_| "60000".into())
        .parse::<u32>()
        .map_err(|e| e.to_string())?;
    // ProMotion's current adaptive rate can differ from its maximum supported mode.
    // Selection accepts the advertised mode without changing the display's settings.
    let monitor = event_loop
        .available_monitors()
        .find(|monitor| {
            monitor.refresh_rate_millihertz() == Some(requested)
                || monitor
                    .video_modes()
                    .any(|mode| mode.refresh_rate_millihertz() == requested)
        })
        .ok_or_else(|| format!("no available {requested}-mHz display"))?;
    // Missing hardware is an error: a fallback would invalidate the comparison.
    // Initial coordinates select the monitor before set_maximized computes its work area.
    // Convert with the destination scale, not the startup window's backing scale.
    let position = monitor.position().to_logical::<f64>(monitor.scale_factor());
    Ok(attributes.with_position(winit::dpi::LogicalPosition::new(
        position.x + 100.0,
        position.y + 100.0,
    )))
}

/// Logs available hardware and chooses the explicitly requested nominal refresh rate.
pub(super) fn place(event_loop: &ActiveEventLoop, window: &Window) -> Result<(), String> {
    // Scroll and Unicode probes share placement but retain their own geometry.
    let pressure = std::env::var_os("PUZZLE_UNICODE_PRESSURE").is_some();
    let unicode = std::env::var_os("PUZZLE_UNICODE_PROBE").is_some();
    if !unicode && std::env::var("PUZZLE_SCROLL_PROBE").as_deref() != Ok("1") {
        return Ok(());
    }
    let requested = std::env::var("PUZZLE_SCROLL_PROBE_MILLIHZ")
        .ok()
        .map(|value| value.parse::<u32>().map_err(|error| error.to_string()))
        .transpose()?
        .or(unicode.then_some(60_000));
    // Retain hardware metadata in the log even when the requested display is absent.
    let mut selected = None;
    for monitor in event_loop.available_monitors() {
        let rate = monitor.refresh_rate_millihertz();
        let maximum = monitor
            .video_modes()
            .map(|mode| mode.refresh_rate_millihertz())
            .max();
        tracing::debug!(event = "desktop.measurement_display", name = ?monitor.name(),
            resolution = ?monitor.size(), position = ?monitor.position(),
            millihertz = ?rate, maximum_millihertz = ?maximum);
        // Select from the hardware's advertised mode, not a guessed monitor name.
        if requested.is_some_and(|wanted| rate == Some(wanted) || maximum == Some(wanted)) {
            selected = Some(monitor);
        }
    }
    let Some(requested) = requested else {
        return Ok(());
    };
    let monitor = selected.ok_or_else(|| format!("no available {requested}-mHz display"))?;
    if unicode && std::env::var_os("PUZZLE_UNICODE_MAXIMIZED").is_some() {
        // Match the user's maximized window, preserving the macOS titlebar and tabs.
        let position = monitor.position().to_logical::<f64>(monitor.scale_factor());
        window.set_outer_position(winit::dpi::LogicalPosition::new(
            position.x + 100.0,
            position.y + 100.0,
        ));
        // AppKit applies the move asynchronously. Maximizing here can undo it;
        // the Moved callback below maximizes only after the destination is observed.
        maximize_arrived(window);
        return Ok(());
    }
    if pressure {
        // The frozen diagnostic needs enough device cells for 5,120 wide syllables.
        // This changes only the owned probe window, preserving ordinary user geometry.
        let _ = window.request_inner_size(winit::dpi::PhysicalSize::new(2200, 1800));
    }
    let position = monitor.position().to_logical::<f64>(monitor.scale_factor());
    // Move the created window: AppKit can constrain an initial frame to the
    // startup screen. Logical desktop coordinates also avoid using the current
    // window's backing scale to interpret another monitor's physical origin.
    window.set_outer_position(winit::dpi::LogicalPosition::new(
        position.x + 100.0,
        position.y + 100.0,
    ));
    tracing::debug!(event = "desktop.measurement_placement", ?requested,
        position = ?window.outer_position(), monitor = ?window.current_monitor().map(|m| m.name()));
    Ok(())
}

/// Maximizes an owned Unicode probe only after AppKit reports its destination.
pub(super) fn maximize_arrived(window: &Window) {
    // Normal terminals and fixed-size samples never enter this measurement path.
    if std::env::var_os("PUZZLE_UNICODE_PROBE").is_none()
        || std::env::var_os("PUZZLE_UNICODE_MAXIMIZED").is_none()
        || !window.is_resizable()
    {
        return;
    }
    let requested = std::env::var("PUZZLE_SCROLL_PROBE_MILLIHZ")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(60_000);
    // Monitor identity is queried on the native thread, before OS zoom chooses a work area.
    // Accepting the maximum mode accommodates ProMotion's adaptive current rate.
    if window.current_monitor().is_some_and(|monitor| {
        monitor.refresh_rate_millihertz() == Some(requested)
            || monitor
                .video_modes()
                .any(|mode| mode.refresh_rate_millihertz() == requested)
    }) {
        // Native zoom follows the actual window screen; non-resizable zoom uses mainScreen.
        window.set_maximized(true);
        // Freeze geometry after zoom so accidental drags cannot alter the cohort.
        window.set_resizable(false);
    }
}

/// Seals the actual published grid beside physical host and font observations.
/// Called only by an explicitly enabled probe, outside its measured frame work.
/// Snapshot dimensions, rather than a font-size estimate, witness the tested grid.
pub(super) fn witness(live: &super::live::LiveWindow) -> serde_json::Value {
    // The main thread already copied these native facts into the host view.
    let info = live.window.info;
    // No winit or AppKit object is queried from the render actor.
    let monitor = live.window.current_monitor();
    // Integral device-cell dimensions are the renderer’s authoritative spacing.
    let metrics = live.text_context.metrics();
    // Logical extents are derived only for evidence; the grid witness comes
    // directly from the sealed publication rather than this division.
    serde_json::json!({
        "grid_columns_rows":[live.snapshot.columns(),live.snapshot.grid_rows()],
        "captured_rows":live.snapshot.rows(),
        "font_size_logical":live.settings.run_font_size(),
        "font_family":live.settings.store.effective().font_family,
        "dpr":live.text_context.dpr(), "host_scale":info.scale,
        "surface_device_px":[info.extent.width,info.extent.height],
        "surface_logical_points":[f64::from(info.extent.width)/info.scale,
            f64::from(info.extent.height)/info.scale],
        "published_device_px":live.frame.target_size,
        "cell_device_px":[metrics.width_px,metrics.height_px],
        "display_id":info.display,"millihertz":info.millihertz,
        "monitor_name":monitor.as_ref().and_then(|m| m.name()),
        "monitor_device_px":monitor.map(|m| [m.size().width,m.size().height]),
        "presentation":"IOSurface on macOS"
    })
}
