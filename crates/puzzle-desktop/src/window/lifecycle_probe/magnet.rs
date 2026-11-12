//! Exact Magnet geometry replay, confined to the opt-in native diagnostic.
//!
//! The observer supplies NSScreen.visibleFrame converted to desktop top-left
//! logical coordinates. Commands use the user's horizontal 24×12 grid, without
//! padding. Native frame requests remain position → size → position, allowing
//! AppKit to deliver the same abrupt, non-live-resize callbacks as Accessibility.
//!
//! Native coordinates and rendered pixels have different lifetimes here:
//! - A command describes an outer window frame in logical desktop points.
//! - The observed titlebar/border inset converts that into winit's inner size.
//! - Real Resized/Moved callbacks, rather than this command, update the renderer.
//! - Acquisition may still return an old-sized drawable; the renderer clips it.
//! Keeping these observations separate makes a coalesced callback visible without
//! pretending that every quickly requested intermediate size was ever presented.
//! A relative display move scales fractions of the source work area; a same-size
//! move preserves dimensions. Both variants remain explicit fixture commands.
//! Restore retains the original outer frame across every subsequent crossing.
//! Drag interpolation changes only position and lets macOS choose its screen
//! while the frame overlaps both panels. Forwarded native ticks, rather than a
//! guessed delay, consume positions. Their display/index stamps expose the switch.
//! The queue is finite and exists only with lock-metrics plus an authored replay;
//! ordinary windows have no drag driver, extra timer or geometry override.
use super::super::ui::InputWindow;
use serde_json::Value;
use winit::dpi::{LogicalPosition, LogicalSize};

/// Native outer frame in logical desktop points: x, y, width, height.
type Rect = [f64; 4];
/// Original frame and refresh-stepped drag belong to this diagnostic window.
#[derive(Default)]
pub(in crate::window) struct Replay {
    /// Restore always refers to the frame before the first Magnet command.
    original: Option<Rect>,
    /// One position is consumed per actual refresh callback forwarded to the UI.
    drag: std::collections::VecDeque<[f64; 2]>,
}

/// Resolves a grid command against an observed work area; never alters font/DPR.
fn resolve(command: &str, current: Rect, visible: Rect, original: Rect) -> Result<Rect, String> {
    // Menu bar and Dock exclusions are already represented in this runtime rectangle.
    let [x, y, w, h] = visible;
    Ok(match command {
        // Half-screen commands divide the work area, not the physical panel resolution.
        "left" => [x, y, w / 2.0, h],
        "right" => [x + w / 2.0, y, w / 2.0, h],
        // The horizontal 24×12 grid gives a two-column inset and twenty-column width.
        "boss" => [x + w / 12.0, y, w * 5.0 / 6.0, h],
        "maximize" => visible,
        // Center preserves dimensions, even when the source is larger than its target.
        "center" => [
            x + (w - current[2]) / 2.0,
            y + (h - current[3]) / 2.0,
            current[2],
            current[3],
        ],
        // Repeated commands never overwrite the first frame saved by the replay owner.
        "restore" => original,
        // Moving at the same size centers it on the other screen, without scaling.
        "next-size" | "previous-size" => [
            x + (w - current[2]) / 2.0,
            y + (h - current[3]) / 2.0,
            current[2],
            current[3],
        ],
        _ => return Err("unknown Magnet command".into()),
    })
}

/// Preserves the outer frame's fractions when moving between unequal work areas.
fn relative(current: Rect, source: Rect, target: Rect) -> Rect {
    // Source and target were validated before dividing by either work-area extent.
    [
        target[0] + (current[0] - source[0]) * target[2] / source[2],
        target[1] + (current[1] - source[1]) * target[3] / source[3],
        current[2] * target[2] / source[2],
        current[3] * target[3] / source[3],
    ]
}

/// Reads a finite observer-authored rectangle; malformed replay data is rejected.
fn rect(value: &Value) -> Result<Rect, String> {
    // Coordinates may be negative in a multi-display desktop; extents may not.
    let values = value
        .as_array()
        .filter(|v| v.len() == 4)
        .ok_or("invalid visible frame")?;
    // Reject NaNs/infinities before calling native geometry APIs.
    let mut result = [0.0; 4];
    for (out, v) in result.iter_mut().zip(values) {
        *out = v
            .as_f64()
            .filter(|n| n.is_finite())
            .ok_or("nonfinite visible frame")?;
    }
    if result[2] <= 0.0 || result[3] <= 0.0 {
        return Err("empty visible frame".into());
    }
    Ok(result)
}

/// Captures actual native outer geometry, including the titlebar for AX parity.
fn current(live: &InputWindow) -> Result<Rect, String> {
    // Winit reports physical geometry, whereas NSScreen.visibleFrame uses points.
    let scale = live.window.scale_factor();
    let p = live
        .window
        .outer_position()
        .map_err(|e| e.to_string())?
        .to_logical::<f64>(scale);
    // Outer size includes decoration; Accessibility frame commands use that same size.
    let s = live.window.outer_size().to_logical::<f64>(scale);
    Ok([p.x, p.y, s.width, s.height])
}

/// Applies one decoded command with real decoration insets, never inner/outer guessing.
pub(super) fn apply(live: &mut InputWindow, step: Value) -> Result<(), String> {
    let current = current(live)?;
    // Capture before any command requests native geometry, not after its first callback.
    let original = *live.magnet.original.get_or_insert(current);
    // Each command carries a witnessed target work area, including cross-display jumps.
    let visible = rect(&step["visible"])?;
    let command = step["command"].as_str().ok_or("missing Magnet command")?;
    let frame = if command.ends_with("-relative") {
        // Preserve fractions of the source visible area, including its menu-bar offset.
        let source = rect(&step["source"])?;
        relative(current, source, visible)
    } else {
        resolve(command, current, visible, original)?
    };
    request(live, command, frame);
    Ok(())
}

/// Requests the outer frame as three successive native operations, as Magnet does.
fn request(live: &InputWindow, command: &str, frame: Rect) {
    let scale = live.window.scale_factor();
    let outer = live.window.outer_size().to_logical::<f64>(scale);
    let inner = live.window.inner_size().to_logical::<f64>(scale);
    // Winit accepts inner size; subtract the actual titlebar/border decorations.
    let size = LogicalSize::new(
        (frame[2] - (outer.width - inner.width)).max(1.0),
        (frame[3] - (outer.height - inner.height)).max(1.0),
    );
    // Applying the final position again compensates AppKit size anchoring, as Magnet does.
    let position = LogicalPosition::new(frame[0], frame[1]);
    // Retain the three-call duration so slow native setters cannot hide in callback gaps.
    let started = std::time::Instant::now();
    live.window.set_outer_position(position);
    let _ = live.window.request_inner_size(size);
    live.window.set_outer_position(position);
    tracing::trace!(target:"puzzle_desktop::resize",event="desktop.magnet_command",
        command, ?frame, elapsed_us=started.elapsed().as_secs_f64()*1e6);
    live.record_event(
        "magnet",
        serde_json::json!({"command":command,"frame":frame}),
    );
}

/// Queues a small-step straddling drag; actual display ticks advance it, not timers.
pub(super) fn drag(live: &mut InputWindow, step: Value) -> Result<(), String> {
    let current = current(live)?;
    // The destination is centered at the existing size, so the intermediate path straddles.
    let target = rect(&step["visible"])?;
    let count = step["steps"]
        .as_u64()
        .filter(|n| *n > 0 && *n <= 10000)
        .ok_or("invalid drag steps")?;
    let end = [
        target[0] + (target[2] - current[2]) / 2.0,
        target[1] + (target[3] - current[3]) / 2.0,
    ];
    // A new authored drag cannot silently overwrite an incomplete previous one.
    if !live.magnet.drag.is_empty() {
        return Err("previous drag incomplete".into());
    }
    // The finite fixture chooses spatial resolution; it never changes display pacing.
    // Interpolation includes the endpoint and does not snap to a screen boundary.
    for i in 1..=count {
        let t = i as f64 / count as f64;
        live.magnet.drag.push_back([
            current[0] + (end[0] - current[0]) * t,
            current[1] + (end[1] - current[1]) * t,
        ]);
    }
    Ok(())
}

/// Exactly one position per delivered source tick, including the screen-switch tick.
pub(in crate::window) fn tick(live: &mut InputWindow, display_id: u64, index: u64) {
    // No size request or live-resize notification is synthesized during this mouse-like path.
    // A source switch supplies subsequent ticks from the newly observed native display.
    if let Some([x, y]) = live.magnet.drag.pop_front() {
        live.window.set_outer_position(LogicalPosition::new(x, y));
        tracing::trace!(target:"puzzle_desktop::resize",event="desktop.drag_step",display_id,index,x,y,
            remaining=live.magnet.drag.len());
    }
}

#[cfg(test)]
mod tests;
