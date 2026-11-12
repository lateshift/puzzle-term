//! Draws real PTY output through the retained harness frame path (§13.4).
//!
//! The shell's Session is the input-mode authority and receives every OS
//! read. The harness mirrors those same captured reads into a pure Terminal
//! for GPU assertions. Comparing both visible grids before drawing prevents
//! the mirror from hiding a Session parse defect. Only the newly captured
//! suffix is fed each time, so FrameBuilder can retain row caches across
//! many keys and `expect-cold-match` can detect lost damage.
//! This mirror is a test witness; the live desktop still renders directly
//! from a sealed Session publication, without readback or a duplicate VT.

use super::{Scenario, ShellRun};
use puzzle_term::{SessionConfig, vt::Terminal};

/// Discards the preceding child's GPU rows and byte mirror before spawn.
pub(super) fn reset(state: &mut Scenario) {
    // A second flags scenario opens a new alt screen; retaining the first
    // child's row caches would make the same line IDs appear unchanged.
    state.terminal = None;
    state.image = None;
    state.frame_builder = None;
    state.text_context = None;
    state.frame_renderer = None;
    state.shell_render_offset = 0;
    state.prior_drawn_rows = None;
    state.drawn_cache = None;
}

/// Builds one offscreen frame after the scenario's explicit idle boundary.
pub(crate) fn render(state: &mut Scenario) -> Result<(), String> {
    // Recognize the previous image before advancing VT. A scroll command
    // can then prove the first drawn row actually moved, not just its grid.
    let prior = if state.image.is_some() && state.terminal.is_some() {
        Some(super::super::drawn::recognize::cached(state)?.rows.clone())
    } else {
        None
    };
    let (suffix, live_text) = {
        let Some(ShellRun::Active(probe)) = state.shell.as_ref() else {
            return Err("render-shell needs active shell".into());
        };
        // The diagnostic raw buffer has a 1 MiB cap. A truncated suffix
        // cannot prove pixel parity with the actual Session state.
        if probe.raw.len() >= 1_048_576 {
            return Err("render-shell raw PTY witness reached its cap".into());
        }
        let bytes = probe
            .raw
            .get(state.shell_render_offset..)
            .ok_or("render-shell offset exceeds PTY capture")?
            .to_vec();
        let view = probe.session.buffer();
        let lines = view.line_ids().ok_or("render-shell lacks visible rows")?;
        (bytes, view.text(lines, Default::default()))
    };
    let terminal = state.terminal.get_or_insert_with(|| {
        // The shell fixture starts at the same 100×30 PTY size. M7's
        // resize scenarios will replace this fixed harness geometry.
        // Match the live shell's default retention too: long scrollback must
        // not make this independent mirror silently discard different rows.
        let config = SessionConfig::default();
        Terminal::new(100, 30, config.scrollback_lines, config.scrollback_bytes)
    });
    terminal.advance(&suffix);
    state.shell_render_offset += suffix.len();
    let view = terminal.buffer();
    let lines = view.line_ids().ok_or("mirrored terminal has no lines")?;
    if view.text(lines, Default::default()) != live_text {
        return Err("render-shell mirror grid differs from Session grid".into());
    }
    // Preview's production FrameBuilder and retained GPU executor now see
    // the same VT state that the input adapter and child produced.
    super::super::preview::render(state)?;
    state.prior_drawn_rows = prior;
    Ok(())
}
