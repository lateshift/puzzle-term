//! Pack-first browser text takeover after the initial WebGPU present (§8.8).
//!
//! A prebuilt unhinted pack can draw the first frame without blocking on a
//! TTF fetch. This task then fetches the matching approved variable font.
//! On a 1× display, D-27 prepares light-hinted outlines from those bytes
//! and presents the same text through the neutral layout and Slug seam.
//! On DPR 1.5 and above, the size-independent pack ink remains the correct
//! unhinted backend result, so the shell keeps its already uploaded frame.
//!
//! This fixed demo scene is not the M6 Session's persistent font owner.
//! It proves ordering, source-hash validation and the low-DPR hinted path
//! through browser WebGPU without embedding font bytes in neutral code.
//! The DOM records `data-font-state` separately from the canvas's ready
//! state: a late TTF error does not make the already visible pack frame
//! disappear. A reviewer can distinguish the initial pack present from
//! the later hinted redraw through the two structured info messages.
use super::fetch_asset;
use crate::canvas_size::CanvasSize;
use puzzle_render::Renderer;
use puzzle_term::session::presentation::SettleWindow;
use puzzle_term::text_scene::{FontData, FontPack, FontSet, FontStyle, TextLine, TextScene};
use std::time::Duration;
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::JsFuture;
use web_sys::HtmlCanvasElement;

/// Waits for the browser's paint callback before taking a changed snapshot.
async fn next_animation_frame(window: &web_sys::Window) -> Result<(), String> {
    // The browser supplies refresh ticks; the same neutral burst rule
    // decides whether this tick is late enough to show both font updates.
    let started = web_time::Instant::now();
    let mut settle = SettleWindow::new(Duration::from_millis(8));
    settle.request(Duration::ZERO, Duration::from_millis(16));
    loop {
        animation_tick(window).await?;
        if settle.may_snapshot(started.elapsed()) {
            return Ok(());
        }
    }
}

/// Awaits one browser paint callback without drawing from a fetch callback.
async fn animation_tick(window: &web_sys::Window) -> Result<(), String> {
    let window = window.clone();
    let promise = js_sys::Promise::new(&mut move |resolve, reject| {
        // Resolving inside rAF resumes the future in the microtask turn
        // before that paint. The renderer therefore never draws an
        // intermediate font-takeover frame from a fetch callback.
        let callback = Closure::once(move |_: f64| {
            let _ = resolve.call0(&JsValue::UNDEFINED);
        });
        match window.request_animation_frame(callback.as_ref().unchecked_ref()) {
            Ok(_) => callback.forget(),
            Err(error) => {
                let _ = reject.call1(&JsValue::UNDEFINED, &error);
            }
        }
    });
    JsFuture::from(promise)
        .await
        .map(|_| ())
        .map_err(|error| format!("requestAnimationFrame failed: {error:?}"))
}

/// Updates the optional full-font state without hiding the pack first frame.
pub(super) async fn after_first(
    window: &web_sys::Window,
    canvas: &HtmlCanvasElement,
    renderer: &Renderer,
    surface: &wgpu::Surface<'_>,
    pack: &FontPack,
    size: &CanvasSize,
) {
    // A TTF fetch or parse failure is recoverable: the verified pack ink
    // stays presented and the state/trace explain why takeover stopped.
    // This status change happens after first presentation, so network
    // latency cannot delay the canvas's `data-puzzle-state=ready` signal.
    if let Err(error) = canvas.set_attribute("data-font-state", "loading") {
        tracing::warn!(?error, "could not mark font loading state");
    }
    match takeover(window, renderer, surface, pack, size).await {
        Ok(()) => {
            // At high DPR no second draw is needed; "ready" means the
            // matching font was validated for later shaping requests.
            if let Err(error) = canvas.set_attribute("data-font-state", "ready") {
                tracing::warn!(?error, "could not mark font ready state");
            }
            tracing::info!("puzzle-web: full font ready after pack frame");
        }
        Err(error) => {
            // `warn` preserves the original failure report while keeping
            // the fallback pack frame usable for startup text.
            if let Err(dom_error) = canvas.set_attribute("data-font-state", "error") {
                tracing::warn!(?dom_error, "could not mark font fallback state");
            }
            tracing::warn!(event = "font.parse_failed", %error, "pack frame remains visible");
        }
    }
}

/// Fetches a matching full font after pack presentation and updates 1× ink.
async fn takeover(
    window: &web_sys::Window,
    renderer: &Renderer,
    surface: &wgpu::Surface<'_>,
    pack: &FontPack,
    size: &CanvasSize,
) -> Result<(), String> {
    // The pack's source hash includes the variable weight. A mismatched
    // TTF must not silently replace text with unrelated metrics or glyphs.
    // Fetching only after the pack present is the visible P7 ordering
    // guarantee exercised by this fixed browser scene.
    let bytes = fetch_asset(window, "JetBrainsMono-Variable.ttf").await?;
    let font = FontData::new(bytes, 0);
    if !pack.matches_source(&font) {
        return Err("full font does not match prebuilt pack".into());
    }
    // At 2× the pack's unhinted curves already serve any device size.
    // Keeping them proves common glyphs need no second preparation.
    if size.dpr >= 1.5 {
        return Ok(());
    }
    let mut fonts = FontSet::new(font, None, None)?;
    // The review line uses the same text and top-left device-pixel cell
    // origin as the pack, so only hinted ink changes on the redraw.
    let line = TextLine {
        text: "Puzzle Terminal -> == !=",
        origin_px: [16, 16],
        size: 14.0,
        dpr: size.dpr as f32,
        style: FontStyle::Regular,
        ligatures: true,
        transformed: false,
    };
    // Layout uses the same integer cell metrics as the pack. Only glyph
    // contours change when the light hinter fits stems at this ppem.
    let mut scene = TextScene::new(&mut fonts, &[line], size.backing_width, size.backing_height)?;
    next_animation_frame(window).await?;
    renderer
        .render_slug_surface(
            surface,
            &mut scene.atlas,
            &scene.instances,
            size.backing_width,
            size.backing_height,
        )
        .map_err(|error| error.to_string())
}
