//! Shared OSC IDs span separate physical rows, exposing grouping mistakes.
//! The plain URL proves detection without application-authored metadata.
//! A hidden cursor keeps screenshots focused on underline and tooltip ink.
//! Pointer placement is sampled from presented cells at either DPR.
//! Time advances in the harness, so the tooltip scene needs no real sleep.
//! Link specimens originate in real OSC 8 output and presented hover routing.
use crate::{
    Session,
    overlay::provider::{PointerEvent, PointerKind},
};
use web_time::Instant;
/// Installs two disjoint grouped segments and a plain detected URL.
pub(super) fn install(session: &mut Session) {
    session.install_model_overlays();
    session.feed(b"\x1b[?25l\x1b]8;id=x;https://example.org/docs\x1b\\Grouped link\x1b]8;;\x1b\\\r\n\x1b]8;id=x;https://example.org/docs\x1b\\second segment\x1b]8;;\x1b\\\r\nhttps://example.org/plain",Instant::now());
}
/// The harness calls this only after rendering and acknowledging its region table.
pub fn hover(session: &mut Session, now: Instant) {
    // The first visible cell comes from actual presented grid geometry.
    let point = session.overlay_geometry().grid.origin.map(|v| v + 1.0);
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
