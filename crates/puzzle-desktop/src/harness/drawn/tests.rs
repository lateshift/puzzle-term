//! Recognizer confidence and immutable failure capture use real GPU attachments.
use super::*;
use crate::{block_on::block_on, gpu_test};

/// A previously ambiguous status glyph must compete against independent templates.
#[test]
fn status_recognition_and_failed_pixels_survive_retry() -> Result<(), String> {
    let _gpu = gpu_test::lock();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let renderer =
        block_on(puzzle_render::Renderer::new(&instance, None)).map_err(|e| e.to_string())?;
    let output =
        std::env::temp_dir().join(format!("puzzle-status-recognition-{}", std::process::id()));
    let mut state = Scenario::new(renderer, output);
    for dpr in [1.0, 2.0] {
        super::super::preview::terminal(&mut state, Some("100x30"))?;
        state.set_dpr(Some(&dpr.to_string()))?;
        // This is the actual failed K5c status prefix and physical row. Fill
        // earlier rows too so glyph preparation and row reuse are nontrivial.
        let term = state.terminal.as_mut().ok_or("test terminal missing")?;
        term.advance(b"\x1b[?25l");
        for row in 0..30 {
            term.advance(format!("\x1b[{};1Hflags=1 last=ctrl counter=10", row + 1).as_bytes());
        }
        super::super::preview::render(&mut state)?;
        prefix::expect_prefix(&mut state, "expect-drawn 28 \"flags=1 last=ctrl counter\"")?;
        expect_grid(&mut state)?;
        cold_match(&mut state)?;
        // A rejected prefix must save both images even when they are identical;
        // a later passing command must not overwrite that failing observation.
        let failed = prefix::expect_prefix(&mut state, "expect-drawn 28 \"WRONG\"")
            .err()
            .ok_or("wrong prefix unexpectedly passed")?;
        let path = super::super::failures::capture(&state, &failed, "forced wrong prefix")?;
        let before = std::fs::read(path.join("warm.png")).map_err(|e| e.to_string())?;
        assert_eq!(
            before,
            std::fs::read(path.join("cold.png")).map_err(|e| e.to_string())?
        );
        prefix::expect_prefix(&mut state, "expect-drawn 28 \"flags=1 last=ctrl counter\"")?;
        let later = super::super::failures::capture(&state, "second failure", "retry")?;
        assert_ne!(path, later);
        assert_eq!(
            before,
            std::fs::read(path.join("warm.png")).map_err(|e| e.to_string())?
        );
        assert!(
            std::fs::read_to_string(path.join("failure.txt"))
                .map_err(|e| e.to_string())?
                .contains("WRONG")
        );
    }
    Ok(())
}

/// Cold equality compares presentation inputs too, rather than default preferences.
#[test]
fn cold_reference_keeps_cursor_and_theme_settings() -> Result<(), String> {
    let _gpu = gpu_test::lock();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let renderer =
        block_on(puzzle_render::Renderer::new(&instance, None)).map_err(|e| e.to_string())?;
    let mut state = Scenario::new(
        renderer,
        std::env::temp_dir().join("puzzle-cold-preferences"),
    );
    super::super::preview::terminal(&mut state, Some("20x3"))?;
    super::super::preview::cursor_settings(&mut state, "cursor-settings bar off false #ff0000")?;
    super::super::theme::set(&mut state, "set theme.dark catppuccin-mocha")?;
    super::super::preview::feed(&mut state, "feed \"ctrl counter\"")?;
    super::super::preview::render(&mut state)?;
    cold_match(&mut state)
}

/// Isolated candidate placement must not misclassify the same thin glyph elsewhere.
#[test]
fn thin_status_glyph_matches_at_every_cell() -> Result<(), String> {
    let _gpu = gpu_test::lock();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let renderer =
        block_on(puzzle_render::Renderer::new(&instance, None)).map_err(|e| e.to_string())?;
    let mut state = Scenario::new(
        renderer,
        std::env::temp_dir().join("puzzle-candidate-positions"),
    );
    for dpr in [1.0, 2.0] {
        super::super::preview::terminal(&mut state, Some("100x30"))?;
        state.set_dpr(Some(&dpr.to_string()))?;
        let term = state.terminal.as_mut().ok_or("test terminal missing")?;
        term.advance(b"\x1b[?25l");
        for row in 0..30 {
            term.advance(format!("\x1b[{};1H{}", row + 1, "c".repeat(100)).as_bytes());
        }
        super::super::preview::render(&mut state)?;
        if let Err(error) = expect_grid(&mut state) {
            let path = super::super::failures::capture(&state, &error, "thin c at all cells")?;
            return Err(format!("DPR {dpr}: {error}; {}", path.display()));
        }
    }
    Ok(())
}

/// A stale cell must keep the exact competing masks that detected its old ink.
#[test]
fn failed_grid_retains_original_candidate_masks() -> Result<(), String> {
    let _gpu = gpu_test::lock();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let renderer =
        block_on(puzzle_render::Renderer::new(&instance, None)).map_err(|e| e.to_string())?;
    let mut state = Scenario::new(
        renderer,
        std::env::temp_dir().join("puzzle-failed-cell-masks"),
    );
    super::super::preview::terminal(&mut state, Some("20x3"))?;
    super::super::preview::feed(&mut state, "feed \"\\e[?25lc\"")?;
    super::super::preview::render(&mut state)?;
    // Deliberately change stored VT without drawing again. Independent candidates
    // must still read the old c, rather than accepting the new d as contextual ink.
    state
        .terminal
        .as_mut()
        .ok_or("terminal absent")?
        .advance(b"\x1b[1;1Hd");
    let error = expect_grid(&mut state).err().ok_or("stale pixels passed")?;
    let path = super::super::failures::capture(&state, &error, "forced stale cell")?;
    let bytes = std::fs::read(path.join("cell-0-0.json")).map_err(|e| e.to_string())?;
    let evidence: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let candidates = evidence["candidates"]
        .as_array()
        .ok_or("candidate masks absent")?;
    let c = candidates
        .iter()
        .find(|value| value["character"] == "c")
        .ok_or("c missing")?;
    let d = candidates
        .iter()
        .find(|value| value["character"] == "d")
        .ok_or("d missing")?;
    assert_eq!(c["error"], 0.0);
    assert!(d["error"].as_f64().ok_or("d score absent")? > 2.0);
    assert_eq!(c["coverage"], evidence["observed_coverage"]);
    // The failing bundle is frozen before a subsequent corrected render or pass.
    super::super::preview::render(&mut state)?;
    expect_grid(&mut state)?;
    assert_eq!(
        bytes,
        std::fs::read(path.join("cell-0-0.json")).map_err(|e| e.to_string())?
    );
    Ok(())
}
