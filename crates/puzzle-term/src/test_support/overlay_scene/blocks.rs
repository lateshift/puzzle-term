//! A failed long command and successful short command share one document.
//! Injected elapsed time avoids sleeping while retaining real OSC parsing.
//! The two commands let hover identify an earlier output interval.
//! Pointer routing uses the presented row table, as native input does.
//! No fixture pre-extracts output or writes clipboard contents directly.
//! Command-block specimens use parsed OSC boundaries and the injected Session clock.
use crate::{
    Session,
    overlay::provider::{PointerEvent, PointerKind},
};
use web_time::Instant;
/// Two statuses and a duration badge use normal parser facts, not fabricated models.
pub(super) fn install(session: &mut Session) {
    session.install_model_overlays();
    let now = Instant::now();
    // Prompt editing is outside the measured execution interval.
    session.feed(b"\x1b[?25l\x1b]133;A\x07$ \x1b]133;B\x07slow command\r\n\x1b]133;C\x07first output\r\nsecond output\r\n", now - std::time::Duration::from_millis(1250));
    // A second successful command makes the failed command's gutter color distinct.
    session.feed(b"\x1b]133;D;1\x07\x1b]133;A\x07$ \x1b]133;B\x07fast command\r\n\x1b]133;C\x07latest output\r\n\x1b]133;D;0\x07", now);
}
/// Hover is routed only after a real frame has supplied its presented region table.
pub fn hover(session: &mut Session, now: Instant) {
    let geometry = session.overlay_geometry();
    // The first command's output begins on the second displayed row.
    let point = [
        geometry.grid.origin[0] + 10.0,
        geometry.grid.origin[1] + geometry.cell_size()[1] + 2.0,
    ];
    session.route_overlay_pointer(
        &PointerEvent {
            kind: PointerKind::Move,
            point,
            button: None,
            modifiers: 0,
        },
        now,
    );
}

/// Click the displayed fixture button, failing if no presented target consumes it.
/// This helper is diagnostic input only; extraction still uses normal copy policy.
pub fn copy(session: &mut Session, now: Instant) -> Result<(), String> {
    // Use the same acknowledged geometry as the preceding hover helper. No
    // font metrics or current parser cursor are consulted on this input path.
    let geometry = session.overlay_geometry();
    // Sample the button's right-hand interior, clear of the scrollbar strip.
    // Hover must already have been presented; a future build cannot accept this click.
    let point = [
        geometry.grid.origin[0] + geometry.grid.size[0] - 24.0,
        geometry.grid.origin[1] + geometry.cell_size()[1] + 2.0,
    ];
    // The model requests a typed Copy for the button's own OSC identity.
    // This helper has no clipboard capability and cannot manufacture a success.
    // Policy vetoes therefore reach the same blocked feedback as a real click.
    let result = session.route_overlay_pointer(
        &PointerEvent {
            kind: PointerKind::Press,
            point,
            button: Some(0),
            modifiers: 0,
        },
        now,
    );
    // Consumption proves that the sample hit an interactive presented control,
    // rather than the passive row observer which deliberately returns No.
    if !result.handled {
        return Err("presented command-copy button did not consume its click".into());
    }
    // Pair the press with a release before subsequent hover/clock operations.
    // Otherwise the registry would correctly keep native-style pointer capture.
    session.route_overlay_pointer(
        &PointerEvent {
            kind: PointerKind::Release,
            point,
            button: Some(0),
            modifiers: 0,
        },
        now,
    );
    Ok(())
}
