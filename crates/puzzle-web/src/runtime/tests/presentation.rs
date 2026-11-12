//! Public write-to-pixels regression for the browser's real asset/startup path.
//!
//! Whole-canvas ink can come from a cursor or a handful of working glyphs.
//! Each ASCII cell must contain its own ink, before and after full-font
//! takeover and scrolling. Reading the submitted canvas avoids fabricating
//! a new snapshot, offscreen frame or damage that could hide stale presentation.
use super::*;

/// Prepared glyphs must survive a hidden-tab turn which presents no image.
#[wasm_bindgen_test(async)]
async fn hidden_prepared_frame_keeps_every_glyph_for_next_presentation() {
    hidden_prepared_reload(false).await;
}

/// A smaller context must not consume stale spans or discard skipped ink.
#[wasm_bindgen_test(async)]
async fn budget_reload_preserves_hidden_prepared_ink() {
    hidden_prepared_reload(true).await;
}

/// Exercise the same hidden-image handoff with and without a policy change.
async fn hidden_prepared_reload(reload: bool) {
    let (terminal, host) = empty_terminal().await;
    let resource = terminal.owner.borrow().text.clone().unwrap();
    JsFuture::from(super::super::resources::load_fonts(&resource))
        .await
        .unwrap();
    // All four styled ASCII faces now remain in the pack (§8.8). Greek
    // exercises newly prepared full-font ink rather than already uploaded data.
    let alphabet = "ΑΒΓΔΕΖΗΘΙΚΛΜΝΞΟΠΡΣΤΥΦΧΨΩαβγδεζηθικλμνξοπρστυφχψω";
    terminal.owner.borrow_mut().visible = false;
    JsFuture::from(
        terminal
            .write(format!("\x1b[1m{alphabet}\x1b[0m").into())
            .unwrap(),
    )
    .await
    .unwrap();
    drain_hidden_preparation(&terminal).await;
    {
        let mut runtime = terminal.owner.borrow_mut();
        let mut context = resource.context.borrow_mut();
        let frame = runtime
            .build_frame(Instant::now(), context.as_mut().unwrap())
            .unwrap()
            .unwrap();
        assert!(
            !frame.uploads.is_empty(),
            "guard did not prepare new glyphs"
        );
        // A surface becomes unavailable after preparation. The actual draw
        // callback's hidden branch must not acknowledge this pending batch.
        runtime.visible = false;
    }
    super::super::draw::present(&terminal.owner).unwrap();
    if reload {
        reload_hidden_budget(&terminal, &resource).await;
    }
    terminal.owner.borrow_mut().visible = true;
    JsFuture::from(terminal.write("!".into()).unwrap())
        .await
        .unwrap();
    let result = assert_cells(
        &terminal,
        &presented_pixels(&terminal).await,
        &[&format!("{alphabet}!")],
        alphabet.chars().count() as u32 + 1,
        0,
    );
    terminal.dispose().unwrap();
    host.remove();
    assert!(result.is_ok(), "{}", result.unwrap_err());
}

/// Await a new policy owner while the previous glyph spans remain unpresented.
async fn reload_hidden_budget(
    terminal: &PuzzleTerminal,
    resource: &Rc<super::super::resources::Text>,
) {
    {
        let mut runtime = terminal.owner.borrow_mut();
        runtime.store.apply_key(
            "render.glyphMemoryMb",
            puzzle_term::bus::Value::Number(f64::from(
                puzzle_term::frame_builder::context::budget::MIN_MIB,
            )),
            puzzle_term::settings::Layer::Session,
        );
        super::super::settings::apply(&mut runtime);
    }
    super::super::settings::select_text(&terminal.owner);
    let start = Instant::now();
    loop {
        JsFuture::from(Promise::new(&mut |resolve, _| {
            window().unwrap().request_animation_frame(&resolve).unwrap();
        }))
        .await
        .unwrap();
        let current = terminal.owner.borrow().text.clone().unwrap();
        if !Rc::ptr_eq(resource, &current) {
            assert!(terminal.owner.borrow().render.frame.uploads.is_empty());
            assert_eq!(
                current
                    .context
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .glyph_budget_bytes(),
                puzzle_term::frame_builder::context::budget::MIN_BYTES,
            );
            break;
        }
        assert!(
            start.elapsed().as_secs() < 10,
            "budget context not selected"
        );
    }
    // The replacement owns fresh font tables and completion lanes; wait while
    // still hidden so the final assertion checks real ink, not admission boxes.
    let current = terminal.owner.borrow().text.clone().unwrap();
    JsFuture::from(super::super::resources::load_fonts(&current))
        .await
        .unwrap();
    drain_hidden_preparation(terminal).await;
}

/// Transfers cold font ink while the production visibility guard prevents presentation.
async fn drain_hidden_preparation(terminal: &PuzzleTerminal) {
    let resource = terminal.owner.borrow().text.clone().unwrap();
    // Enqueue cold work without presenting; completion events cannot draw a hidden page.
    {
        let mut runtime = terminal.owner.borrow_mut();
        let mut context = resource.context.borrow_mut();
        runtime
            .build_frame(Instant::now(), context.as_mut().unwrap())
            .unwrap()
            .unwrap();
    }
    let start = Instant::now();
    loop {
        JsFuture::from(Promise::new(&mut |resolve, _| {
            window().unwrap().request_animation_frame(&resolve).unwrap();
        }))
        .await
        .unwrap();
        let pending = {
            let mut context = resource.context.borrow_mut();
            let context = context.as_mut().unwrap();
            // Transfer ready ownership under the existing refresh-derived publication
            // allowance; neither glyph preparation nor actual rendering happens here.
            context.process_source_completions();
            context.sources_pending()
        };
        if !pending {
            break;
        }
        assert!(
            start.elapsed().as_secs() < 5,
            "hidden font jobs did not complete"
        );
    }
}

/// Regular, styled and colored cells must survive public writes after the initial frame.
#[wasm_bindgen_test(async)]
async fn public_write_renders_every_cell_and_cursor_with_served_assets() {
    let (terminal, host) = empty_terminal().await;
    // Clean up before an assertion traps wasm: later tests must not inherit listeners.
    let result = written_frames(&terminal).await;
    terminal.dispose().unwrap();
    host.remove();
    assert!(result.is_ok(), "{}", result.unwrap_err());
}

/// Creates a public terminal with served assets and observes its empty first image.
pub(super) async fn empty_terminal() -> (PuzzleTerminal, web_sys::HtmlElement) {
    // No Fonts replacement: HTTP serves the same pack/font bytes as the demo.
    let document = window().unwrap().document().unwrap();
    let host = document
        .create_element("div")
        .unwrap()
        .dyn_into::<web_sys::HtmlElement>()
        .unwrap();
    host.style().set_property("width", "800px").unwrap();
    host.style().set_property("height", "480px").unwrap();
    document.body().unwrap().append_child(&host).unwrap();
    let options = js_sys::JSON::parse(r#"{"cols":80,"rows":24,"settings":{"font":{"ligatures":false},"cursor":{"blink":false},"theme":{"mode":"dark"}}}"#).unwrap();
    let terminal = PuzzleTerminal::create(host.clone(), Some(options))
        .await
        .unwrap();
    terminal.focus().unwrap();
    let _ = presented_pixels(&terminal).await;
    (terminal, host)
}

/// Tests output, normal asynchronous takeover, and public writeln scrolling.
async fn written_frames(terminal: &PuzzleTerminal) -> Result<(), String> {
    let text = "ABCDabcd0123";
    let output = format!(
        "{text}\r\n\x1b[1m{text}\x1b[0m\r\n\x1b[3m{text}\x1b[0m\r\n\x1b[31m{text}\x1b[0m\r\n\x1b[32m{text}\x1b[0m\r\n\x1b[2;34m{text}\x1b[0m"
    );
    JsFuture::from(terminal.write(output.clone().into()).unwrap())
        .await
        .unwrap();
    assert_cells(
        terminal,
        &presented_pixels(terminal).await,
        &[text; 6],
        12,
        5,
    )?;
    // Await the normal background takeover without reinstalling a builder or forcing damage.
    let resource = terminal.owner.borrow().text.clone().unwrap();
    JsFuture::from(super::super::resources::load_fonts(&resource))
        .await
        .unwrap();
    JsFuture::from(
        terminal
            .write(format!("\x1b[2J\x1b[H{output}").into())
            .unwrap(),
    )
    .await
    .unwrap();
    assert_cells(
        terminal,
        &presented_pixels(terminal).await,
        &[text; 6],
        12,
        5,
    )?;
    JsFuture::from(terminal.write("\x1b[2J\x1b[H".into()).unwrap())
        .await
        .unwrap();
    for _ in 0..40 {
        JsFuture::from(terminal.writeln(text.into()).unwrap())
            .await
            .unwrap();
    }
    assert_cells(
        terminal,
        &presented_pixels(terminal).await,
        &[text; 23],
        0,
        23,
    )
}

/// Capture an actual rAF submission in its own refresh, before drawing-buffer discard.
pub(super) async fn presented_pixels(terminal: &PuzzleTerminal) -> Vec<u8> {
    let copy = window()
        .unwrap()
        .document()
        .unwrap()
        .create_element("canvas")
        .unwrap()
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .unwrap();
    let context = copy
        .get_context("2d")
        .unwrap()
        .unwrap()
        .dyn_into::<web_sys::CanvasRenderingContext2d>()
        .unwrap();
    // Install a read-only submission observer before the already requested rAF.
    // It copies this canvas inside paint, without creating damage or another frame.
    terminal.owner.borrow_mut().measurements.capture = Some((copy.clone(), context.clone()));
    let start = Instant::now();
    loop {
        JsFuture::from(Promise::new(&mut |resolve, _| {
            window().unwrap().request_animation_frame(&resolve).unwrap();
        }))
        .await
        .unwrap();
        if copy.width() > 0 && copy.height() > 0 {
            let pixels = context
                .get_image_data(0.0, 0.0, f64::from(copy.width()), f64::from(copy.height()))
                .unwrap()
                .data()
                .0;
            let pending = {
                let r = terminal.owner.borrow();
                r.damage || r.resize.is_some()
            };
            if pixels[3] == 255 && !pending {
                terminal.owner.borrow_mut().measurements.capture = None;
                return pixels;
            }
        }
        assert!(
            start.elapsed().as_secs() < 5,
            "no submitted canvas after public write"
        );
    }
}

/// Geometry comes from the fitted metrics; ink and cursor position come exclusively from pixels.
pub(super) fn assert_cells(
    terminal: &PuzzleTerminal,
    pixels: &[u8],
    rows: &[&str],
    cursor_col: u32,
    cursor_row: u32,
) -> Result<(), String> {
    let runtime = terminal.owner.borrow();
    let resource = runtime.text.as_ref().unwrap();
    let context = resource.context.borrow();
    let metrics = context.as_ref().unwrap().metrics();
    let padding = (runtime.settings.render_padding * runtime.dpr as f32).round() as u32;
    let width = runtime.dom.as_ref().unwrap().canvas().width();
    let background = &pixels[..4];
    let ink = |col: u32, row: u32| {
        (0..metrics.height_px)
            .flat_map(|y| (0..metrics.width_px).map(move |x| (x, y)))
            .filter(|(x, y)| {
                let at = (((padding + row * metrics.height_px + y) * width
                    + padding
                    + col * metrics.width_px
                    + x)
                    * 4) as usize;
                pixels[at..at + 3]
                    .iter()
                    .zip(background)
                    .any(|(a, b)| a.abs_diff(*b) > 8)
            })
            .count()
    };
    let missing: Vec<_> = rows
        .iter()
        .enumerate()
        .flat_map(|(row, text)| {
            text.chars()
                .enumerate()
                .map(move |(col, ch)| (row, col, ch))
        })
        .filter(|(row, col, _)| ink(*col as u32, *row as u32) < 3)
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "public write left cells without glyph ink: {missing:?}; expected cursor ink {}",
            ink(cursor_col, cursor_row)
        ));
    }
    // A steady focused block fills the empty next cell, unlike stray text pixels.
    if ink(cursor_col, cursor_row) <= (metrics.width_px * metrics.height_px / 2) as usize {
        return Err(format!(
            "cursor missing at expected ({cursor_col}, {cursor_row}); old origin ink {}",
            ink(0, 0)
        ));
    }
    Ok(())
}
