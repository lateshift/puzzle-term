//! Winit's implementation of the toolkit-free native surface seam (D-43).
//!
//! Every OS observation is copied on the main thread. The render worker sees
//! backing extent, DPR, refresh metadata and posted host notifications only.
use puzzle_embed::{Extent, SurfaceInfo, Visibility};
use winit::{dpi::PhysicalSize, window::Window};

/// Converts physical host pixels without applying DPR a second time.
pub(super) fn extent(size: PhysicalSize<u32>) -> Extent {
    Extent {
        width: size.width,
        height: size.height,
    }
}

/// Copies the display's highest rate, matching D-33's ProMotion convention.
pub(super) fn info(window: &Window) -> SurfaceInfo {
    // Monitor objects never cross the actor seam; extract their plain observations here.
    let monitor = window.current_monitor();
    // Highest advertised mode preserves the existing ProMotion prediction policy (D-33).
    let millihertz = monitor.as_ref().map_or(60_000, |monitor| {
        // A display without video modes may still expose its current refresh rate.
        monitor
            .video_modes()
            .map(|mode| mode.refresh_rate_millihertz())
            .max()
            .or_else(|| monitor.refresh_rate_millihertz())
            .unwrap_or(60_000)
    });
    #[cfg(target_os = "macos")]
    let display = monitor.as_ref().map_or(0, |monitor| {
        // This public winit accessor supplies the CG ID on the main thread.
        // No AppKit/monitor object crosses into the native callback or renderer.
        use winit::platform::macos::MonitorHandleExtMacOS;
        u64::from(monitor.native_id())
    });
    #[cfg(not(target_os = "macos"))]
    let display = 0;
    SurfaceInfo {
        // Other platforms replace zero with their host-tracked display incarnation.
        display,
        // Backing pixels already include the scale factor; do not multiply twice.
        extent: extent(window.inner_size()),
        // Font metrics receive the separate logical-to-device scale.
        scale: window.scale_factor(),
        millihertz,
    }
}

/// Copies actual focus/display witnesses while retaining unknown visibility.
pub(super) fn visibility(window: &Window) -> Visibility {
    // Capture a coherent native display witness for the diagnostic/recovery owner.
    let monitor = window.current_monitor();
    Visibility {
        // Focus controls clock calibration eligibility; Session receives focus separately.
        focused: window.has_focus(),
        // Unknown visibility remains unknown rather than manufacturing a hidden window.
        visible: window.is_visible(),
        // Only the Occluded event can authoritatively supply this observation.
        occluded: None,
        // Diagnostics compare the actual backing size with the display resolution.
        monitor_extent: monitor.as_ref().map(|m| extent(m.size())),
        // Names are owned strings so toolkit handles cannot reach the render thread.
        monitor_name: monitor.as_ref().and_then(|m| m.name()),
        // Retain the active rate beside the highest-mode rate used by the clock.
        monitor_rate: monitor.and_then(|m| m.refresh_rate_millihertz()),
        #[cfg(feature = "lock-metrics")]
        fullscreen: window.fullscreen().is_some(),
    }
}
