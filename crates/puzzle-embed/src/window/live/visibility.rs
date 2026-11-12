//! Visibility evidence for idle refresh samples (§10.4, reviewer 2026-10-04).
//!
//! An existing nonzero surface can still be behind another app or fully occluded.
//! Winit supplies occlusion events and an actual OS focus query (isKeyWindow on
//! macOS). These observations deliberately do not change calibration eligibility:
//! a visible unfocused terminal must still learn phase beside an active editor.
//! Establish an occlusion cause from traces before changing the phase rules.
//!
//! Window creation and surface configuration have separate ages: a monitor move
//! can reconfigure an older window, whereas startup can produce both young ages.
//! Keep those clocks beside each calibration so early free acquires can be
//! distinguished from a later attempt in the same visibility/display state.
//! These diagnostic observations run at event boundaries, outside timed frames.
//! Missing OS evidence stays null/unknown; it is never inferred from focus.
use super::LiveWindow;

impl LiveWindow {
    /// Retains every winit occlusion edge and its actual key/display witnesses.
    pub fn occluded(&mut self, occluded: bool) {
        // Keep the last actual event, including false. Before the first event,
        // None means unknown rather than a manufactured visible-window claim.
        self.occluded = Some(occluded);
        self.log_visibility("winit_occluded");
    }

    /// Logs independently observed OS state rather than the terminal cursor's focus.
    pub fn log_visibility(&self, source: &'static str) {
        let monitor = self.window.current_monitor();
        tracing::debug!(event = "desktop.window_visibility", source,
            since_window_ms = self.window_created.elapsed().as_secs_f64() * 1000.0,
            since_configure_ms = self.surface_configured.elapsed().as_secs_f64() * 1000.0,
            key = self.window.has_focus(), visible = ?self.window.is_visible(),
            occluded = ?self.occluded,
            surface_size = ?self.frame.target_size,
            monitor_resolution = ?monitor.as_ref().map(|m| m.size()),
            monitor_name = ?monitor.as_ref().and_then(|m| m.name()),
            monitor_millihertz = ?monitor.and_then(|m| m.refresh_rate_millihertz()));
    }

    /// Dev-only frontmost-process observation, outside acquire/present timing.
    #[cfg(feature = "lock-metrics")]
    pub fn visibility_evidence(&self, source: &'static str) -> serde_json::Value {
        let monitor = self.window.current_monitor();
        // Winit key-window state and frontmost process are independent: another
        // process may be active while this terminal remains fully visible.
        let frontmost = frontmost();
        let result = serde_json::json!({
            "key":self.window.has_focus(), "visible":self.window.is_visible(),
            "occluded":self.occluded, "frontmost":frontmost,
            "process_id":std::process::id(),
            "since_window_ms":self.window_created.elapsed().as_secs_f64()*1000.0,
            "since_configure_ms":self.surface_configured.elapsed().as_secs_f64()*1000.0,
            "surface_size_device_px":self.frame.target_size,
            "monitor_resolution_device_px":monitor.as_ref().map(|m| [m.size().width,m.size().height]),
            "monitor_name":monitor.as_ref().and_then(|m| m.name()),
            "monitor_millihertz":monitor.and_then(|m| m.refresh_rate_millihertz()),
        });
        // The query precedes or follows an entire timed loop. Never shell out
        // between presents: that delay could accidentally supply FIFO pacing.
        tracing::debug!(event = "desktop.visibility_evidence", source, evidence = %result);
        result
    }
}

/// Reads the active application without adding AppKit dependencies or handwritten unsafe.
#[cfg(all(feature = "lock-metrics", target_os = "macos"))]
fn frontmost() -> serde_json::Value {
    // This read-only script queries an application identity, never window
    // placement or activation. A denied query must not alter pacing behavior.
    let observed = std::process::Command::new("/usr/bin/osascript")
        .args([
            "-e",
            "tell application \"System Events\"",
            "-e",
            "set p to first application process whose frontmost is true",
            "-e",
            "return {name of p, unix id of p}",
            "-e",
            "end tell",
        ])
        .output();
    match observed {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout);
            let pair = text.trim().rsplit_once(", ");
            serde_json::json!({"name":pair.map(|(name, _)|name),
                "pid":pair.and_then(|(_, pid)|pid.parse::<u32>().ok())})
        }
        // Permission/unavailable observations remain explicit unknown evidence.
        // They cannot manufacture a visible-window or pacing confirmation.
        result => serde_json::json!({"unknown":format!("{result:?}")}),
    }
}

/// Other desktop platforms retain the winit witnesses without invoking macOS tools.
#[cfg(all(feature = "lock-metrics", not(target_os = "macos")))]
fn frontmost() -> serde_json::Value {
    serde_json::json!({"unknown":"frontmost process query is macOS-only"})
}
