//! Opt-in native lifecycle replay for D-53 measurements, not frame scheduling.
//!
//! A caller-authored JSON fixture posts resize, move, mark and close operations
//! to the existing UI inbox. Winit applies them on its owner, so real OS geometry
//! callbacks traverse the ordinary surface seam. No display mode is changed.
//! The finite producer has no reference to a Window, Session or render driver.
//! An idle ordinary terminal never creates this producer or reads a fixture.
//! Times and extents are measurement inputs, not runtime pacing constants.
//! Resize requests deliberately do not pretend to be observed resize events:
//! the OS may coalesce them, and the private recording captures actual callbacks.
//! Monitor selection requires its observed rate (or unknown-rate maximum), since
//! ProMotion also advertises a 60-Hz mode and must not be mistaken for the LG.
//! Marks wait for caller-authored gaps after moves to observe actual placement.
//! Scroll inputs use the shared wheel message and the normal model animation.
//! Their clocks are collected before dispatch, independently of callback arrival.
//! The driver records which actual refresh it reserved; future scanout is separate.
//! No data from this fixture can change measured lead, settle windows or latency.
//! A close returns to the ordinary host shutdown order; this module joins no
//! pipeline threads and cannot bypass their explicit session shutdown controls.
//! The finite worker owns only an event-loop proxy, so its lifetime cannot keep
//! a closed native surface subscribed to a retained per-display link.
/// User grid commands and actual-refresh drag replays.
pub(in crate::window) mod magnet;
use super::{WakeEvent, ui::InputWindow};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use winit::{
    dpi::{LogicalPosition, PhysicalSize},
    event_loop::EventLoopProxy,
};

/// Native host operations, with no terminal or GPU ownership attached.
#[derive(Debug)]
pub(super) enum Action {
    /// Resize the backing surface through the same callback as a live drag.
    Resize([u32; 2]),
    /// Place the window on the display with this advertised maximum rate.
    Move(u32),
    /// Seal an actual geometry/time observation in the private recording.
    Mark(String),
    /// A finite wheel fixture enters the ordinary owner-side scroll policy.
    Scroll(String),
    /// Send explicitly authored text to this window's login shell, never global input.
    Text(String),
    /// Authored physical chord with its pre-UI-post clock, never global OS input.
    Key(String, Option<Instant>),
    /// Close while the independently authored child may still be producing output.
    Close,
    /// Runtime visible-frame command, independent of AppKit live-resize state.
    Magnet(Value),
    /// Start a position-only drag consumed one step per real display tick.
    Drag(Value),
}

/// Loads one explicitly requested finite fixture before any timer thread exists.
pub(super) fn start(proxy: EventLoopProxy<WakeEvent>) -> Result<(), String> {
    let Some(path) = std::env::var_os("PUZZLE_LIFECYCLE_SCRIPT") else {
        // Default builds and ordinary windows have no additional wake source.
        return Ok(());
    };
    let value: Value = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let steps = value
        .as_array()
        .ok_or("lifecycle fixture must be an array")?;
    let mut events = Vec::new();
    // Decode on startup: invalid data never enters the asynchronous UI inbox.
    for step in steps {
        // Reject malformed fixtures rather than silently omitting an intended test.
        let millis = step["ms"].as_u64().ok_or("missing lifecycle time")?;
        let action = decode(step)?;
        events.push((Duration::from_millis(millis), action));
    }
    if events.windows(2).any(|pair| pair[0].0 > pair[1].0) {
        return Err("lifecycle times must be ordered".into());
    }
    std::thread::Builder::new()
        // A named producer is distinguishable from the measured pipeline in samples.
        .name("puzzle-lifecycle-probe".into())
        .spawn(move || {
            let origin = Instant::now();
            for (offset, mut action) in events {
                // Absolute fixture times do not accumulate UI or rendering delays.
                // A malformed centuries-long fixture must not panic its worker.
                let Some(deadline) = origin.checked_add(offset) else {
                    tracing::warn!(event = "desktop.lifecycle_probe_failed", "time overflow");
                    break;
                };
                std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
                // Stamp on this timer thread before posting, so native event-loop
                // residence is visible even while the main thread handles geometry.
                if let Action::Key(_, arrival) = &mut action {
                    *arrival = Some(Instant::now());
                    // UTC pairs with the independent child receipt; elapsed stages
                    // use monotonic Instant and never subtract clocks across threads.
                    let issued_ns = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                        .ok().map(|value| value.as_nanos() as u64);
                    tracing::debug!(target:"puzzle_desktop::input",event="desktop.key_post",?issued_ns);
                }
                if proxy.send_event(WakeEvent::Lifecycle(action)).is_err() {
                    // Closing an owned window ends this finite producer normally.
                    break;
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Converts untrusted measurement data without defaults that could hide a wrong display.
fn decode(step: &Value) -> Result<Action, String> {
    // Checked conversion rejects truncation of extents and advertised refresh rates.
    let number = |key: &str| {
        // JSON numbers are unsigned; negative coordinates/extents are invalid data.
        step[key]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| format!("invalid lifecycle {key}"))
    };
    match step["action"].as_str() {
        // This closed catalogue adds no arbitrary code or callback on the UI owner.
        Some("resize") => Ok(Action::Resize([number("width")?, number("height")?])),
        // Movement preserves display modes: only window position is requested.
        Some("move") => Ok(Action::Move(number("millihertz")?)),
        Some("mark") => Ok(Action::Mark(
            step["label"].as_str().ok_or("missing label")?.into(),
        )),
        Some("scroll") => Ok(Action::Scroll(
            // Labels identify all samples, including failures; none select policy.
            step["label"].as_str().ok_or("missing label")?.into(),
        )),
        // Preserve the runtime visible frame; the UI validates it before native calls.
        Some("magnet") => Ok(Action::Magnet(step.clone())),
        // Drag steps remain a finite queue, advanced by observed refresh identities.
        Some("drag") => Ok(Action::Drag(step.clone())),
        Some("text") => Ok(Action::Text(
            step["text"].as_str().ok_or("missing text")?.into(),
        )),
        // This diagnostic chord never targets another application's focused view.
        Some("key") => Ok(Action::Key(
            step["chord"].as_str().ok_or("missing key chord")?.into(),
            None,
        )),
        Some("close") => Ok(Action::Close),
        _ => Err("unknown lifecycle action".into()),
    }
}

/// Applies geometry on the native thread and asks the host to close only on explicit exit.
pub(super) fn apply(live: &mut InputWindow, action: Action) -> bool {
    let _stage = crate::resize_timing::Stage::new("main_programmatic_geometry");
    match action {
        Action::Resize([width, height]) => {
            // Surface/grid/PTY coalescing remains the ordinary Resized callback's job.
            let _ = live
                .window
                .request_inner_size(PhysicalSize::new(width, height));
        }
        Action::Move(rate) => {
            // A display's supported lower modes do not identify its nominal rate.
            let monitor = live.window.available_monitors().find(|monitor| {
                monitor.refresh_rate_millihertz() == Some(rate)
                    || (monitor.refresh_rate_millihertz().is_none()
                        && monitor
                            .video_modes()
                            .map(|mode| mode.refresh_rate_millihertz())
                            .max()
                            == Some(rate))
            });
            if let Some(monitor) = monitor {
                // Position is in the destination's logical coordinate space, as
                // in the existing diagnostic placement; no native handle crosses threads.
                let p = monitor.position().to_logical::<f64>(monitor.scale_factor());
                live.window
                    .set_outer_position(LogicalPosition::new(p.x + 100.0, p.y + 100.0));
            } else {
                // A missing destination invalidates this run; fallback would hide it.
                tracing::error!(
                    event = "desktop.lifecycle_probe_failed",
                    rate,
                    "display unavailable"
                );
                return true;
            }
        }
        Action::Mark(label) => {
            // Observe after the OS's asynchronous move, not its requested position.
            let info = live.surface_info();
            // Only plain copied facts cross into the JSON recording and log sink.
            let data = json!({"label":label, "display":info.display,
                "millihertz":info.millihertz, "scale":info.scale, "extent":[info.extent.width, info.extent.height]});
            live.record_event("lifecycle", data.clone());
            // File observers receive the same geometry as the private event recording.
            tracing::debug!(event="desktop.lifecycle_mark", %data);
        }
        Action::Magnet(step) => {
            if let Err(error) = magnet::apply(live, step) {
                tracing::error!(event="desktop.lifecycle_probe_failed",%error);
                return true;
            }
        }
        Action::Drag(step) => {
            if let Err(error) = magnet::drag(live, step) {
                tracing::error!(event="desktop.lifecycle_probe_failed",%error);
                return true;
            }
        }
        // Input targets this fixture's own session, never the globally focused app.
        // Keeping the login shell intact exercises its configuration and normal job control.
        Action::Text(text) => live.sent(live.session.text(text)),
        Action::Key(chord, arrival) => return key(live, &chord, arrival),
        Action::Close => return true,
        // Scroll dispatch does not ask the renderer to skip its normal frame gates.
        Action::Scroll(label) => scroll(live, label),
    }
    false
}

/// Timestamps one authored first input before sending it through the existing wheel lane.
fn scroll(live: &InputWindow, label: String) {
    use puzzle_term::{viewport::ScrollInput, vt::input::*};
    // The trace timestamp and private recording precede dispatch; no reply is awaited.
    tracing::trace!(target: "puzzle_desktop::restart",
        event = "desktop.restart_scroll_input", %label);
    live.record_event("scroll-input", json!({"label":label}));
    // Cell zero is a neutral pointer location; the wheel carries a local override.
    let pointer = MouseEvent {
        // Shift chooses local history scrolling even if the child enabled tracking.
        kind: MouseKind::Wheel,
        button: 64,
        column: 0,
        row: 0,
        pixel_x: 0,
        pixel_y: 0,
        modifiers: Modifiers(Modifiers::SHIFT),
        button_held: false,
        click_count: 0,
    };
    // A negative notch moves into the long neutral history prepared by the child.
    // Use the same local-override wheel command as the existing scrolling witness.
    // Diagnostic wheel positions follow the same view-local contract as the live host.
    live.sent(live.session.wheel(
        ScrollInput::Rows(-16.0),
        puzzle_embed::SurfacePointer {
            position: [0.0, 0.0],
            event: pointer,
        },
    ));
}

/// Dispatch one authored chord through the same adapter as a native callback.
fn key(live: &mut InputWindow, chord: &str, arrival: Option<Instant>) -> bool {
    // Each constituent event retains the same pre-post clock. Release/modifier
    // callbacks cannot manufacture writer samples when they encode no bytes.
    match (crate::keyboard::sequences::for_chord(chord), arrival) {
        (Ok((events, _)), Some(arrival)) => {
            for event in events {
                live.input_arrived(event, arrival);
            }
            false
        }
        _ => {
            tracing::error!(
                event = "desktop.lifecycle_probe_failed",
                "invalid timed key fixture"
            );
            true
        }
    }
}
