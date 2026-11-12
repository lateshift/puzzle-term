//! Cursor reference specimens use ordinary Session sources (design §11.6).
//!
//! A steady bar cursor keeps the anchor visible without covering candidate ink.
//! The annotation is measured outside that cursor's narrow device-pixel footprint.
//! The uncommitted candidate remains distinct from terminal cells and PTY bytes.
//! No fixture writes instances, constructs its own painter or chooses a glyph.
//! Both ratios use identical logical input positions and font settings.
//! The harness alone selects device scale and performs the actual GPU readback.
//! Steady cursor policy removes timing variance without disabling the anchor.
//! Suggested and composed strings intentionally differ, exposing priority mistakes.
use crate::{
    Session,
    messages::{Command, PreeditState},
};
use puzzle_bus::Origin;
use web_time::Instant;

/// Parsed status and a static reduced-motion flash remain ordinary built-ins.
pub(super) fn status(session: &mut Session) {
    session.install_model_overlays();
    // Static flash ink makes the candidate independent of wall-clock fade phase.
    // Its finite endpoint and removal remain governed by the production model.
    session.set_reduced_motion(true);
    session.feed(
        b"\x1b[?25lProgress: 50 percent, error state\x1b]9;4;2;50\x1b\\\x07",
        Instant::now(),
    );
}

/// Installs the real built-ins and feeds their respective protocol/API sources.
pub(super) fn install(session: &mut Session, composing: bool) -> Result<(), String> {
    let now = Instant::now();
    session.install_model_overlays();
    session.set_cursor_settings(
        crate::session::cursor_settings::CursorSettings {
            style: crate::session::cursor_settings::CursorStyle::Bar,
            blink: crate::session::cursor_settings::CursorBlink::Off,
            ..Default::default()
        },
        now,
    );
    // Keep the cursor anchor visible: hiding the cursor also hides its annotations.
    session.feed(b"\x1b[3;3H$ ", now);
    session.set_ghost_text(Some("suggested command"));
    if composing {
        // A candidate replaces the suggestion; no commit reaches a transport.
        session.submit(
            Command::Preedit(PreeditState {
                text: "composing text".into(),
                cursor: Some(9),
            }),
            Origin::User,
        );
        session.pump(now);
    }
    Ok(())
}

/// Pins completed-frame facts for pixel references, using the ordinary host message.
/// Timing and counts vary between machines; the production statistics tests still
/// measure real frames. Equal timestamps preserve the explicitly supplied byte rate.
pub(super) fn hud_sample(session: &mut Session) -> Result<(), String> {
    use crate::{FrameResult, SessionMessage, messages::FrameStats};
    let now = Instant::now();
    // Six equal phases expose the HUD's sum, rather than one precomputed total.
    // Distinct pipeline counts make an accidental label permutation visible.
    // Binary-sized atlas and upload values also exercise the display units.
    let stats = FrameStats {
        cpu_ms: [0.1; 6],
        instances: [24, 128, 16],
        glyph_bytes: 8 * 1024 * 1024,
        upload_bytes: 4096,
        parse_bytes_per_second: 150_000_000.0,
    };
    // Seed the observer's rate boundary before providing its reproducible sample.
    // The first observation may replace the supplied rate using an older clock.
    // The second has a zero interval and therefore retains the host rate value.
    for _ in 0..2 {
        session.apply(
            SessionMessage::FrameResult(FrameResult::Statistics(stats)),
            now,
        )?;
    }
    Ok(())
}
