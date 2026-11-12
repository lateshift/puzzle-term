//! Real served assets protect cluster fallback placement and upload ownership at both DPRs.
use super::*;
use puzzle_term::frame::Upload;
/// Public writes must submit each fallback cluster in its D-31 allocation.
#[wasm_bindgen_test(async)]
async fn fallback_cells_color_masks_and_hidden_uploads() {
    let (terminal, host) = super::presentation::empty_terminal().await;
    let result = fallback_frames(&terminal).await;
    terminal.dispose().unwrap();
    host.remove();
    assert!(result.is_ok(), "{}", result.unwrap_err());
}
/// A hidden prepared frame must retain both bitmap formats until an actual GPU write.
async fn fallback_frames(terminal: &PuzzleTerminal) -> Result<(), String> {
    let resource = terminal.owner.borrow().text.clone().unwrap();
    JsFuture::from(super::super::resources::load_fonts(&resource))
        .await
        .unwrap();
    let lines = [
        "A漢字B",
        "AかなB",
        "A한글B",
        "A🦀B",
        "A👨‍👩‍👧‍👦B",
        "A🇩🇪B",
        "A👍🏽B",
        "A☺️B",
        "A✌️B",
        "A𝄞B",
    ];
    let output = format!(
        "\x1b[?25l\x1b[2J\x1b[H\x1b[38;2;0;255;0m{}",
        lines.join("\r\n")
    );
    terminal.owner.borrow_mut().visible = false;
    JsFuture::from(terminal.write(output.into()).unwrap())
        .await
        .unwrap();
    settle(terminal).await?;
    hidden_formats(terminal, &resource)?;
    super::super::draw::present(&terminal.owner).unwrap();
    terminal.owner.borrow_mut().visible = true;
    // A further public write requests the image; no synthetic snapshot hides missing uploads.
    JsFuture::from(terminal.write("\x1b[24;1H!".into()).unwrap())
        .await
        .unwrap();
    let pixels = super::presentation::presented_pixels(terminal).await;
    guard(terminal, &pixels)?;
    // CPU mirrors restore all pipelines to a fresh GPU painter without source work.
    restore(terminal, &pixels).await
}
/// Checks both retained raster formats while presentation is suppressed by visibility.
fn hidden_formats(
    terminal: &PuzzleTerminal,
    resource: &Rc<super::super::resources::Text>,
) -> Result<(), String> {
    {
        let mut runtime = terminal.owner.borrow_mut();
        let mut context = resource.context.borrow_mut();
        runtime.session.submit(
            puzzle_term::messages::Command::GlyphRasterized,
            puzzle_term::Origin::Platform,
        );
        runtime.session.pump(Instant::now());
        let frame = runtime
            .build_frame(Instant::now(), context.as_mut().unwrap())?
            .ok_or("no fallback frame")?;
        if !frame
            .uploads
            .iter()
            .any(|u| matches!(u, Upload::Mask { .. }))
            || !frame
                .uploads
                .iter()
                .any(|u| matches!(u, Upload::Color { .. }))
        {
            return Err("guard did not prepare both raster kinds".into());
        }
        runtime.visible = false;
    }
    Ok(())
}
/// Every ink pixel belongs to a declared cell; CJK uses green mask tint, emoji retain color.
fn guard(terminal: &PuzzleTerminal, pixels: &[u8]) -> Result<(), String> {
    let runtime = terminal.owner.borrow();
    let text = runtime.text.as_ref().unwrap();
    let metrics = text.context.borrow().as_ref().unwrap().metrics();
    let padding = (runtime.settings.render_padding * runtime.dpr as f32).round() as u32;
    let width = runtime.dom.as_ref().unwrap().canvas().width();
    let height = runtime.dom.as_ref().unwrap().canvas().height();
    let mut counts = [[0_usize; 80]; 24];
    let mut colored = [0_usize; 24];
    for y in 0..height {
        for x in 0..width {
            let at = ((y * width + x) * 4) as usize;
            let pixel = &pixels[at..at + 3];
            if pixel
                .iter()
                .zip(&pixels[..3])
                .all(|(a, b)| a.abs_diff(*b) <= 8)
            {
                continue;
            }
            let (col, row) = (
                (x.saturating_sub(padding) / metrics.width_px) as usize,
                (y.saturating_sub(padding) / metrics.height_px) as usize,
            );
            let cells = if row < 3 {
                6
            } else if row < 9 {
                4
            } else if row == 9 {
                3
            } else if row == 23 {
                1
            } else {
                0
            };
            if x < padding || y < padding || row >= 24 || col >= cells {
                return Err(format!(
                    "fallback ink outside allocated cells at {x},{y} ({col},{row})"
                ));
            }
            counts[row][col] += 1;
            if col > 0 && col + 1 < cells && (pixel[0] > pixel[1] || pixel[2] > pixel[1]) {
                colored[row] += 1;
            }
        }
    }
    validate_counts(&counts, &colored)
}

/// Assertions are independent of pixel traversal and report the exact blank cell.
fn validate_counts(counts: &[[usize; 80]; 24], colored: &[usize; 24]) -> Result<(), String> {
    for row in 0..10 {
        let cells = if row < 3 {
            6
        } else if row < 9 {
            4
        } else {
            3
        };
        for (col, count) in counts[row].iter().enumerate().take(cells) {
            if *count < 3 {
                return Err(format!("no fallback ink at {col},{row}"));
            }
        }
        if row < 3 && colored[row] != 0 {
            return Err(format!("CJK mask ignored text tint at row {row}"));
        }
    }
    if colored[3..9].contains(&0) {
        return Err(format!("emoji tint replaced authored colors: {colored:?}"));
    }
    Ok(())
}
/// A new painter receives retained CPU pages, producing identical submitted pixels.
async fn restore(terminal: &PuzzleTerminal, expected: &[u8]) -> Result<(), String> {
    let text = terminal.owner.borrow().text.clone().unwrap();
    let count = text.context.borrow().as_ref().unwrap().prepared_glyphs();
    {
        let renderer = text.gpu.renderer.borrow();
        let renderer = renderer.as_ref().unwrap();
        let format = renderer
            .surface_format(terminal.owner.borrow().surface.as_ref().unwrap(), 800, 480)
            .unwrap();
        *text.painter.borrow_mut() = Some(puzzle_render::FrameRenderer::new(renderer, format));
        text.context.borrow_mut().as_mut().unwrap().reset_atlas();
        let context = text.context.borrow();
        terminal
            .owner
            .borrow_mut()
            .set_text_context(context.as_ref().unwrap())
            .unwrap();
    }
    JsFuture::from(terminal.write("\x1b[24;2H".into()).unwrap())
        .await
        .unwrap();
    let actual = super::presentation::presented_pixels(terminal).await;
    if actual != expected {
        return Err("CPU raster mirror restoration changed pixels".into());
    }
    if count != text.context.borrow().as_ref().unwrap().prepared_glyphs() {
        return Err("device restore repeated glyph preparation".into());
    }
    Ok(())
}

/// The public bitmap-only effect exception requests a fresh larger browser source.
mod effects;

/// Evidence awaits accepted jobs while the real hidden-frame upload policy remains active.
async fn settle(terminal: &PuzzleTerminal) -> Result<(), String> {
    let start = Instant::now();
    loop {
        let pending = {
            let mut runtime = terminal.owner.borrow_mut();
            let resource = runtime.text.clone().unwrap();
            let mut context = resource.context.borrow_mut();
            let context = context.as_mut().unwrap();
            // The same internal completion command requests a fresh visible-row snapshot.
            runtime.session.submit(
                puzzle_term::messages::Command::GlyphRasterized,
                puzzle_term::Origin::Platform,
            );
            runtime.session.pump(Instant::now());
            runtime
                .build_frame(Instant::now(), context)?
                .ok_or("fallback probe frame absent")?;
            context.sources_pending()
        };
        if !pending {
            return Ok(());
        }
        if start.elapsed().as_secs() >= 30 {
            return Err("fallback completion exceeded 30 seconds".into());
        }
        // Macrotasks allow the actual browser discovery service to run between probe frames.
        super::socket::delay(4).await;
    }
}
