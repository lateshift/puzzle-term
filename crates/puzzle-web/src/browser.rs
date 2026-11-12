//! WebGPU canvas setup owned by the browser shell (design §10.1, R9).
//!
//! The browser crate is the only workspace member that reads DOM objects.
//! It obtains the canvas, creates a WebGPU-only instance and surface, then
//! hands those handles to the same renderer used by the native harness.
//! This keeps platform handles out of `puzzle-render` while proving that its
//! clear pass can present through a browser swap chain.
//!
//! A wasm start function cannot await GPU initialization. It installs
//! diagnostics synchronously, then starts the adapter request on the browser
//! task queue. A contrasting page background, canvas state attribute and
//! visible error element make first-frame success distinguishable from a
//! WebGPU initialization failure during browser review.
//!
//! The M3 review font is a separate Trunk asset. Fetching it before drawing
//! keeps 296 KiB of font data out of the wasm code while retaining the
//! exact vendored face used by native references. A fetch failure follows
//! the same visible error path as a WebGPU setup failure.
//!
//! The M3 fixture is a fixed scene rather than a live terminal viewport.
//! Its original 248×248 image describes CSS-sized geometry at DPR one.
//! The review page must draw its affine instances into a backing store
//! enlarged by the browser's device-pixel ratio. If CSS independently
//! scales the old bitmap, a sharp shader result becomes blurred by the
//! browser compositor before it reaches a physical display pixel.
//! This distinction matters at fractional Windows scaling as well as on
//! Retina panels: the rounded backing extent can have a fractional CSS
//! counterpart. The same DPR therefore controls the instance transform,
//! canvas attributes and inline CSS dimensions for one initial frame.
//! The full container observer and monitor-change path remains in M7.
use crate::canvas_size::CanvasSize;
use puzzle_render::Renderer;
use puzzle_term::{
    pack_scene::PackScene,
    slug_scene::{FontData, SlugScene, SlugSceneKind},
    text_scene::FontPack,
};
use std::str::FromStr;
use tracing_subscriber::{filter::Targets, prelude::*};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::HtmlCanvasElement;
/// M4 pack-first font fetch and hinted full-font specimen takeover.
mod text;

/// Fixed browser review scenes; their source bytes stay platform-owned.
enum BrowserScene {
    /// M3's full-font geometry comparison.
    Slug(SlugScene),
    /// M4's pack-only ASCII first frame.
    Pack(PackScene),
}

/// Exposes the shared affine and target dimensions for DPR sizing.
impl BrowserScene {
    /// Returns logical Slug dimensions or already rounded pack dimensions.
    fn dimensions(&self) -> (u32, u32) {
        match self {
            Self::Slug(scene) => (scene.width, scene.height),
            Self::Pack(scene) => (scene.width, scene.height),
        }
    }

    /// Scales only affine placement; the packed em curves remain unchanged.
    fn scale(&mut self, factor: f32, width: u32, height: u32) {
        let instances = match self {
            Self::Slug(scene) => {
                scene.width = width;
                scene.height = height;
                &mut scene.instances
            }
            // A pack scene has already rounded its cells in device pixels.
            // A second scale would shift full-font takeover at fractional DPR.
            Self::Pack(_) => return,
        };
        for instance in instances {
            for component in &mut instance.m {
                *component *= factor;
            }
            for component in &mut instance.t {
                *component *= factor;
            }
        }
    }
}

/// Reads an optional URL filter without treating a malformed query as fatal.
fn log_targets() -> Targets {
    // Keep the first-frame info event visible under the web's warn default.
    let default = Targets::new()
        .with_default(tracing::Level::WARN)
        .with_target("puzzle_web", tracing::Level::INFO);
    let query = web_sys::window()
        .and_then(|window| window.location().search().ok())
        .and_then(|search| web_sys::UrlSearchParams::new_with_str(&search).ok())
        .and_then(|params| params.get("log"));
    let Some(spec) = query else {
        return default;
    };
    // A bad URL option must not prevent WebGPU startup; report it under the
    // safe default after installing the subscriber.
    Targets::from_str(&spec).unwrap_or(default)
}
/// Installs browser diagnostics and starts the first clear frame.
/// The panic hook supplies browser stack messages during development.
#[wasm_bindgen(start)]
pub(crate) fn start() {
    console_error_panic_hook::set_once();
    // Console and performance timeline consume the same structured events.
    let console = tracing_subscriber::fmt::layer()
        .without_time()
        .with_writer(tracing_web::MakeWebConsoleWriter::new())
        .with_filter(log_targets());
    tracing_subscriber::registry()
        .with(console)
        .with(tracing_web::performance_layer())
        .init();
    // Historical specimen URLs remain opt-in; normal demo creation owns the page GPU.
    let review = web_sys::window()
        .and_then(|w| w.location().search().ok())
        .is_some_and(|query| query.contains("scene="));
    if !review {
        return;
    }
    spawn_local(async {
        if let Err(error) = clear_canvas().await {
            tracing::error!(event = "web.gpu_setup_failed", %error, "browser WebGPU setup failed");
            show_error(&error);
        }
    });
}
/// Marks the canvas as failed and exposes the setup error in the page.
///
/// The error element is outside the canvas, so it stays readable even if
/// WebGPU never produces a frame. The canvas may be absent if the page was
/// embedded incorrectly; in that case the error text can still be shown.
fn show_error(error: &str) {
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    if let Some(canvas) = document.get_element_by_id("puzzle-canvas")
        && let Err(dom_error) = canvas.set_attribute("data-puzzle-state", "error")
    {
        tracing::error!(
            event = "web.canvas_state_failed",
            ?dom_error,
            "failed to mark canvas error state"
        );
    }
    if let Some(message) = document.get_element_by_id("puzzle-error") {
        message.set_text_content(Some(error));
        if let Err(dom_error) = message.remove_attribute("hidden") {
            tracing::error!(
                event = "web.error_display_failed",
                ?dom_error,
                "failed to reveal browser error text"
            );
        }
    }
}
/// Selects the approved real-font review fixture from the URL query.
async fn review_scene(window: &web_sys::Window) -> Result<Option<BrowserScene>, String> {
    // The fixture is built before surface configuration so its target size
    // determines the canvas backing dimensions without CSS interpolation.
    // It uses the approved bundled font, avoiding a font fetch race when a
    // reviewer opens the URL and checks the first presented frame.
    let search = window.location().search().unwrap_or_default();
    let params = web_sys::UrlSearchParams::new_with_str(&search)
        .map_err(|error| format!("invalid scene query: {error:?}"))?;
    match params.get("scene").as_deref() {
        Some("slug") => {
            let font = FontData::new(fetch_asset(window, "JetBrainsMono-Variable.ttf").await?, 0);
            SlugScene::new(SlugSceneKind::GlyphLarge, Some(&font))
                .map(BrowserScene::Slug)
                .map(Some)
        }
        Some("text") => {
            // This path fetches no TTF before presentation. The ready
            // pack contains the entire initial ASCII sample and texels.
            let bytes = fetch_asset(window, "jetbrains-regular.pzfp").await?;
            let pack = FontPack::decode(&bytes)?;
            PackScene::new(
                pack,
                "Puzzle Terminal -> == !=",
                14.0,
                review_dpr(window)? as f32,
            )
            .map(BrowserScene::Pack)
            .map(Some)
        }
        _ => Ok(None),
    }
}

/// Fetches a Trunk-copied asset without embedding it in the wasm binary.
async fn fetch_asset(window: &web_sys::Window, path: &str) -> Result<Vec<u8>, String> {
    // Fetch resolves the asset beside the wasm from the page's own origin;
    // no font bytes are linked into the platform-neutral core or wasm.
    let response = JsFuture::from(window.fetch_with_str(path))
        .await
        .map_err(|error| format!("font fetch failed: {error:?}"))?;
    let response: web_sys::Response = response
        .dyn_into()
        .map_err(|_| "font fetch returned no Response")?;
    if !response.ok() {
        return Err(format!("font fetch returned HTTP {}", response.status()));
    }
    let buffer = response
        .array_buffer()
        .map_err(|error| format!("font body unavailable: {error:?}"))?;
    let buffer = JsFuture::from(buffer)
        .await
        .map_err(|error| format!("font body failed: {error:?}"))?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}

/// Presents a Slug fixture or the existing M0 clear frame.
fn present_scene(
    renderer: &Renderer,
    surface: &wgpu::Surface<'_>,
    scene: Option<&mut BrowserScene>,
    width: u32,
    height: u32,
) -> Result<(), String> {
    // The two paths deliberately share the same surface and readiness
    // reporting. The review URL must reveal shader setup failures rather
    // than falling back to the M0 clear screen unnoticed.
    match scene {
        Some(BrowserScene::Slug(scene)) => renderer
            .render_slug_surface(surface, &mut scene.atlas, &scene.instances, width, height)
            .map_err(|error| error.to_string()),
        Some(BrowserScene::Pack(scene)) => renderer
            .render_packed_slug_surface(
                surface,
                &scene.pack.curves,
                &scene.pack.bands,
                &scene.instances,
                width,
                height,
            )
            .map_err(|error| error.to_string()),
        None => renderer
            .clear_surface(surface)
            .map_err(|error| error.to_string()),
    }
}

/// Uses the browser DPR unless a review URL supplies a numeric override.
///
/// An explicit override lets the reviewer exercise fractional backing
/// sizes on one machine. A malformed override is reported through the
/// page's visible error state, so an invalid sharpness test cannot quietly
/// fall back to the machine's own DPR.
fn review_dpr(window: &web_sys::Window) -> Result<f64, String> {
    // URLSearchParams handles ordering and escaping for combined scene,
    // log-filter and DPR options. A substring test could mistake another
    // parameter's value for the requested ratio.
    let search = window
        .location()
        .search()
        .map_err(|error| format!("cannot read URL query: {error:?}"))?;
    let params = web_sys::UrlSearchParams::new_with_str(&search)
        .map_err(|error| format!("cannot parse URL query: {error:?}"))?;
    let Some(value) = params.get("dpr") else {
        // Browser zoom and OS display scaling are reflected in this ratio
        // at startup; a resize observer is unnecessary for the fixed probe.
        return Ok(window.device_pixel_ratio());
    };
    value
        .parse::<f64>()
        .map_err(|_| format!("invalid dpr override: {value}"))
}

/// Matches scene coordinates, canvas backing and CSS size to one DPR.
///
/// The packed curve texture remains size-independent em-space data. Its
/// affine instance is the sole source of device-pixel scale for the Slug
/// vertex shader. The CSS dimensions then reverse that DPR scaling so the
/// physical presentation maps each rendered pixel to one display pixel.
fn size_canvas(
    window: &web_sys::Window,
    canvas: &HtmlCanvasElement,
    scene: Option<&mut BrowserScene>,
) -> Result<CanvasSize, String> {
    let (logical_width, logical_height) = scene
        .as_ref()
        .map_or((800, 480), |scene| scene.dimensions());
    let dpr = review_dpr(window)?;
    let size = if matches!(scene.as_ref(), Some(BrowserScene::Pack(_))) {
        // The pack scene's source dimensions already contain integer
        // device cells. CSS is derived from those exact backing pixels.
        CanvasSize::from_backing(logical_width, logical_height, dpr)?
    } else {
        CanvasSize::new(logical_width, logical_height, dpr)?
    };
    if let Some(scene) = scene {
        // Atlas curves stay in em space. Scale only their affine map so one
        // CSS-sized glyph occupies the correct number of device pixels.
        // Multiplying all four matrix entries also preserves rotations and
        // shears if the fixed review URL later chooses another M3 fixture.
        // Translation uses the same ratio so the glyph margin stays equal
        // in CSS pixels at every backing resolution.
        scene.scale(size.dpr as f32, size.backing_width, size.backing_height);
    }
    // Width and height attributes determine WebGPU's attachment extent.
    // They are set before surface creation; CSS affects only presentation.
    canvas.set_width(size.backing_width);
    canvas.set_height(size.backing_height);
    // The fractional CSS value comes from the rounded backing size. A
    // fixed 800 px rule or aspect ratio would resample the 248 px fixture.
    // Inline dimensions take precedence over the page's display styling.
    canvas
        .set_attribute(
            "style",
            &format!("width: {}px; height: {}px", size.css_width, size.css_height),
        )
        .map_err(|error| format!("cannot size canvas CSS: {error:?}"))?;
    // Keep all three quantities in one event so a reviewer can verify
    // backing = round(CSS × DPR) from the browser console.
    tracing::info!(
        dpr = size.dpr,
        css_width = size.css_width,
        css_height = size.css_height,
        backing_width = size.backing_width,
        backing_height = size.backing_height,
        "puzzle-web: canvas pixel size"
    );
    Ok(size)
}

/// Creates the platform surface and presents the selected first frame.
///
/// This fixed-scene DPR snapshot keeps pixels sharp at first presentation.
/// Live ResizeObserver and DPR-change handling belong to the later web milestone.
///
/// Scene creation precedes sizing because the native golden scene supplies
/// its own crop dimensions. Sizing precedes surface configuration because
/// WebGPU must acquire a texture at the final device-pixel extent.
async fn clear_canvas() -> Result<(), String> {
    let window = web_sys::window().ok_or("browser window is unavailable")?;
    let document = window.document().ok_or("browser document is unavailable")?;
    let element = document
        // A missing host canvas is an integration error, not a GPU error.
        .get_element_by_id("puzzle-canvas")
        .ok_or("canvas is missing")?;
    let canvas: HtmlCanvasElement = element.dyn_into().map_err(|_| "element is not a canvas")?;
    // Other URLs retain the M0 clear-color smoke screen.
    let mut scene = review_scene(&window).await?;
    let size = size_canvas(&window, &canvas, scene.as_mut())?;
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    // Request browser WebGPU only; the wasm build has no WebGL fallback.
    descriptor.backends = wgpu::Backends::BROWSER_WEBGPU;
    let instance = wgpu::Instance::new(descriptor);
    let surface = instance
        // wgpu's Canvas target wraps the DOM element without a native window.
        .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
        .map_err(|error| error.to_string())?;
    let renderer = Renderer::new(&instance, Some(&surface))
        .await
        .map_err(|error| error.to_string())?;
    // Configuration uses the selected scene dimensions before acquisition;
    // otherwise the swapchain could silently crop the review glyph.
    renderer
        .configure(&surface, size.backing_width, size.backing_height)
        // Surface configuration must happen before the first acquisition.
        .map_err(|error| error.to_string())?;
    // Present before marking ready; WebGPU failure remains visible in the page.
    present_scene(
        &renderer,
        &surface,
        scene.as_mut(),
        size.backing_width,
        size.backing_height,
    )?;
    // Set ready only after present succeeds; loading and failed states remain visible otherwise.
    canvas
        .set_attribute("data-puzzle-state", "ready")
        .map_err(|error| format!("could not mark canvas ready: {error:?}"))?;
    tracing::info!("puzzle-web: first frame presented");
    if let Some(BrowserScene::Pack(pack)) = scene.as_ref() {
        // The pack frame is already visible. The child module owns the
        // later font fetch and keeps that frame if takeover fails.
        text::after_first(&window, &canvas, &renderer, &surface, &pack.pack, &size).await;
    }
    Ok(())
}
