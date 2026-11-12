//! Independent cursor-setting criteria from §14.2 and M6.4g.
//!
//! The precedence table covers all DECSCUSR codes without relying on a
//! single screenshot. Pixel assertions separately prove that chosen RGB
//! reaches the GPU. A retained builder checks that a settings update can
//! change the next frame while VT row content remains unchanged.
//!
//! DECRQSS reads the VT's last application request, while the frame uses
//! user settings. Keeping those checks separate catches an implementation
//! that overwrites terminal state merely to obtain the desired appearance.
//! The screenshot checks actual composited pixels: an instance-color unit
//! test alone would miss a lost cursor batch or incorrect shader ordering.

use super::{report, session_test};

/// Runs the two-frame scenario through a fresh GPU readback.
fn cursor_scenario() -> Result<(), String> {
    // Run the same native harness command used for earlier M6 references,
    // including font loading, WebGPU submission and offscreen readback.
    // Its two screenshots share one warmed builder, exposing stale colors
    // after a live setting change rather than rebuilding from scratch.
    crate::commands::capture(
        "cargo",
        &[
            "xtask",
            "harness",
            "assets/scenarios/terminal-frame/cursor-settings.pzs",
        ],
    )?;
    Ok(())
}

/// Reports each setting and VT boundary as a separate acceptance line.
pub(super) fn report_cursor(passed: &mut bool) {
    // All four configured shapes are crossed with seven DECSCUSR codes,
    // allowAppStyle, mode 12 and each blink policy. This is much broader
    // than the two color examples that follow in the screenshot scenario.
    report(
        "M6.4g cursor precedence table",
        session_test(
            "session::cursor_settings::tests::precedence_table_covers_shape_blink_and_app_permission",
        ),
        passed,
    );
    // The query contract is independent of the visual override. A renderer
    // that changes VT's stored code to zero could draw correctly but lie
    // to an application restoring its preferred cursor after a mode switch.
    report(
        "M6.4g DECRQSS stores ignored app style",
        session_test(
            "session::cursor_settings::tests::decrqss_returns_ignored_app_request_and_reset_uses_settings",
        ),
        passed,
    );
    // A cell with explicit red ink over blue background distinguishes
    // inverse from the theme's default light-on-dark color pair. The next
    // frame fixes green RGB without sending another grid edit.
    report(
        "M6.4g inverse and hex cursor pixels",
        cursor_scenario().and_then(|()| {
            session_test(
                "frame_builder::tests::cursor_inverse_and_literal_color_follow_osc_precedence",
            )
        }),
        passed,
    );
    // The style epoch must advance when settings change even though VT's
    // row version and stored DECSCUSR request do not. A warm-cache-only
    // failure would be missed by a cold harness screenshot.
    report(
        "M6.4g cursor setting changes next frame without grid damage",
        session_test("frame_builder::tests::cursor_setting_changes_next_frame_without_grid_damage"),
        passed,
    );
}
