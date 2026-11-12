//! Model overlay specimen feeds ordinary terminal output and public commands.
//!
//! Two OSC 133 command sequences carry success/failure statuses. A later bell,
//! literal search and selected range provide independent marker sources. Output
//! appended after scrolling back raises the M8 new-output state. The native
//! query mode uses the same Request/Text commands as host keyboard/IME input.
//! Reduced motion makes reference ink deterministic without sleeping; a public
//! hover pins the scrollbar during subsequent readbacks. No bitmap or direct
//! RenderFrame instance stands in for provider output in this fixture.
//!
//! Each source fact is authored independently in parser bytes, making absent
//! prompt status, bell or match wiring visible to the mechanical marker test.
//! The selected row uses a retained LineId obtained after all output is parsed;
//! it does not assume numeric IDs are contiguous or tied to screen row numbers.
//! Selection is submitted before the view moves, then survives later output.
//! The query intentionally has three matches on different document rows.
//!
//! Scrolling to Top changes following through ordinary API navigation. Appended
//! output then raises the existing Boolean without moving the viewed content.
//! Opening the native query and typing hit demonstrates that UI input reaches
//! the search engine while leaving the transport untouched. No query result set
//! or scrollbar marker is fabricated by the fixture.
//!
//! Hover reads the real track after the first frame establishes its geometry.
//! Detached routing runs before returning the builder, as in the desktop host.
//! Its model coordinates are divided by the actual builder DPR for logical input.
use crate::{
    Session,
    messages::Command,
    search::SearchCommand,
    selection::SelectionCommand,
    viewport::ScrollTarget,
    vt::selection::{SelectPoint, SelectionKind},
};
use puzzle_bus::Origin;
use web_time::Instant;

/// Populates every M8 marker source and registers the three public painters.
pub fn install(session: &mut Session) -> Result<(), String> {
    let now = Instant::now();
    session.install_model_overlays();
    session.set_reduced_motion(true);
    // This specimen pins M8's retained bell marker, not M9.5's transient flash.
    // Keep its existing pixel-zero background witness independent of flash timing.
    session.set_visual_bell(false);
    // Hide the terminal cursor so its independent blink cannot alter a reference.
    session.feed(b"\x1b[?25l", now);
    for row in 0..100 {
        // Prompt status belongs to its following Finished mark, as in a real shell.
        let line = match row {
            5 => "\x1b]133;A\x07$ true\x1b]133;B\x07\x1b]133;C\x07\x1b]133;D;0\x07".into(),
            25 => "\x1b]133;A\x07$ false\x1b]133;B\x07\x1b]133;C\x07\x1b]133;D;1\x07".into(),
            35 => "bell position\x07".into(),
            50 | 60 | 70 => format!("row {row:03} hit result"),
            _ => format!("row {row:03} retained output"),
        };
        session.feed(format!("{line}\r\n").as_bytes(), now);
    }
    // Select retained source text through the API, without creating a second range.
    let ids = session
        .buffer()
        .line_ids()
        .ok_or("model fixture rows absent")?;
    let selected = session
        .buffer()
        .lines(ids)
        .nth(40)
        .ok_or("selection row absent")?
        .id();
    session.submit(
        Command::Select(SelectionCommand::Range {
            kind: SelectionKind::Simple,
            start: SelectPoint {
                line: selected,
                column: 0,
            },
            end: SelectPoint {
                line: selected,
                column: 8,
            },
        }),
        Origin::Api,
    );
    session.submit(Command::ScrollTo(ScrollTarget::Top), Origin::Api);
    session.pump(now);
    // New output preserves the viewed rows and makes the click-to-follow pill visible.
    session.feed(b"new output after scrolling back\r\n", now);
    session.submit(Command::Search(SearchCommand::Request), Origin::User);
    session.pump(now);
    session.submit(Command::Text("hit".into()), Origin::User);
    session.pump(now);
    Ok(())
}

/// Pins scrollbar ink through the same logical pointer router used by both shells.
pub fn hover(session: &mut Session, now: Instant) {
    use crate::overlay::provider::{PointerEvent, PointerKind};
    let [x, y, w, height] = session.viewport().scrollbar.track();
    let dpr = session.overlay_geometry().dpr.max(f32::EPSILON);
    // The model stores device coordinates; providers and this router use logical pixels.
    session.route_overlay_pointer(
        &PointerEvent {
            kind: PointerKind::Move,
            point: [(x + w / 2.0) as f32 / dpr, (y + height - 1.0) as f32 / dpr],
            button: None,
            modifiers: 0,
        },
        now,
    );
}
