//! Creation validates options once and mounts an independently owned Session.
//!
//! GPU and pack work happen in the platform; the core receives a text context
//! only when validated bytes exist. No font file is compiled into the engine.
//!
//! A Session needs its initial keyboard and exit policy before any host can
//! send input. Defaults are resolved for the user's OS, then a session-layer
//! settings patch is validated through the same store as desktop config.
//! The web hold default is installed before that patch so an explicit host
//! choice can still replace it without another settings implementation.
//!
//! Text resources are acquired before input is mounted. Mounting installs
//! observers which report appearance and visibility immediately, and those
//! callbacks must find a usable Session even while DOM creation is underway.
//! They request later presentation rather than recursively creating frames.
//!
//! The initial font pack contains exact em-space texels and integer-cell
//! metrics. Creating a text context from it never extracts a font outline.
//! The later font fetch is platform work, and its successful takeover
//! invalidates per-session row caches through the context generation.
//!
//! Input mount failures return to create instead of publishing a partially
//! usable terminal. The local owners then remove already registered DOM
//! listeners and release their strong resource references. Registry entries
//! are weak, so a failed create cannot pin a page device indefinitely.
//!
//! Font parsing is asynchronous with respect to creation. Until tables are
//! available, the existing pack remains the source for covered startup text.
//! A failed fetch is a diagnostic; it does not blank that usable pack.
//!
//! Each terminal awaits the shared full-font promise independently and then
//! invalidates its own row caches. Font download state belongs to the text
//! owner; terminals do not duplicate parsed faces or store font data globally.
//! The fixed creation options are input to the existing settings validator,
//! while later settings mutation remains the separate M10 API surface.
use super::*;
use puzzle_term::settings::{Layer, SettingsStore};
use wasm_bindgen::{JsCast, JsValue};

/// Builds the M7 rendered API subset; later M10 APIs are not exported here.
pub(crate) async fn create(
    parent: web_sys::HtmlElement,
    options: JsValue,
) -> Result<Owner, JsValue> {
    let store = options_store(&options)?;
    let grid = arguments::grid(&options)?;
    let settings = store.effective().clone();
    let dpr = window()?.device_pixel_ratio().clamp(0.25, 8.0) as f32;
    // Start the adapter request before awaiting any pack network response.
    let gpu = resources::gpu();
    let key = settings::text_key(&settings, f64::from(dpr));
    let text = resources::text(
        gpu,
        key,
        settings.font_size,
        dpr,
        settings.font_fallback.clone(),
        settings.render_glyph_memory_mb,
    )
    .await?;
    resources::wait_gpu(&text.gpu).await?;
    let owner = Runtime::new(initial_config(&settings, grid));
    {
        let mut runtime = owner.borrow_mut();
        runtime.session.set_keybindings(store.keybindings());
        let context = text.context.borrow();
        let context = context
            .as_ref()
            .ok_or_else(|| value::error("FontLoadFailed", "pack context missing"))?;
        // Runtime keeps render caches; Session receives geometry without owning fonts.
        // This also handles a full-font takeover whose dimensions match the old pack.
        runtime
            .set_text_context(context)
            .map_err(|e| value::error("FontLoadFailed", &e))?;
        // Render policies need measured geometry before the first publication.
        runtime.session.apply_settings(&settings, Instant::now());
        runtime.text = Some(text.clone());
        runtime.settings = settings;
        runtime.store = store;
        runtime.dpr = f64::from(dpr);
        runtime.parent = Some(parent.clone());
        runtime.fixed_grid = grid;
    }
    text.follow(&owner);
    media::css(&owner)?;
    let canvas = window()?
        .document()
        .ok_or_else(|| value::error("InvalidArgument", "document unavailable"))?
        .create_element("canvas")?
        .dyn_into::<web_sys::HtmlCanvasElement>()?;
    let surface = text
        .gpu
        .instance
        .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
        .map_err(|e| value::error("WebGpuUnavailable", &e.to_string()))?;
    owner.borrow_mut().surface = Some(surface);
    let input = crate::dom::DomInput::mount(&parent, canvas, host::Host(Rc::downgrade(&owner)))?;
    input.refresh_layout().await?;
    owner.borrow_mut().dom = Some(input);
    draw::install(&owner)?;
    draw::fit(&owner)?;
    draw::schedule(&owner)?;
    load_fonts(&owner, text);
    Ok(owner)
}

/// Resolves OS defaults before validating the host's session-layer patch.
fn options_store(options: &JsValue) -> Result<SettingsStore, JsValue> {
    let mut store = SettingsStore::default();
    store.set_binding_platform(crate::dom::platform(), true);
    store.apply_key(
        "session.onExit",
        puzzle_term::bus::Value::String("hold".into()),
        Layer::Session,
    );
    // Undefined options retain OS defaults; wrong payloads are handled by the schema.
    if !options.is_null() && !options.is_undefined() {
        let settings = value::get(options, "settings")?;
        if !settings.is_undefined() {
            let patch = value::plain(&settings, 0, &mut 4096)?;
            let report = store.apply_patch(&patch, Layer::Session);
            // Preserved browser-reserved/future bindings need an explicit availability warning.
            for issue in report.unavailable {
                tracing::warn!(event="settings.binding_unavailable", key=%issue.path, reason=%issue.message);
            }
            for issue in report.rejected {
                tracing::warn!(event="settings.rejected", key=%issue.path, reason=%issue.message);
            }
        }
    }
    Ok(store)
}

/// Full OpenType tables take over in the background, preserving pack origins.
pub(super) fn load_fonts(owner: &Owner, text: Rc<resources::Text>) {
    let weak = Rc::downgrade(owner);
    wasm_bindgen_futures::spawn_local(async move {
        let result = wasm_bindgen_futures::JsFuture::from(resources::load_fonts(&text)).await;
        if let Some(owner) = weak.upgrade().filter(|o| !o.borrow().disposed) {
            if let Err(error) = result {
                tracing::warn!(event = "font.load_failed", ?error);
            }
            // A zoom/DPR change may already have selected a different context.
            // Completion of an older font request must not revert that choice.
            if owner
                .borrow()
                .text
                .as_ref()
                .is_some_and(|current| Rc::ptr_eq(current, &text))
                && let Some(context) = text.context.borrow().as_ref()
                && let Err(error) = owner.borrow_mut().set_text_context(context)
            {
                tracing::warn!(event = "web.metrics_failed", %error);
            }
            if let Err(error) = draw::schedule(&owner) {
                tracing::warn!(event = "web.frame_schedule_failed", ?error);
            }
        }
    });
}

/// Fixed creation dimensions belong to the initial VT, before any font-fit resize.
fn initial_config(
    settings: &puzzle_term::settings::Settings,
    grid: Option<[u16; 2]>,
) -> puzzle_term::SessionConfig {
    let mut config = settings.session_config();
    if let Some([columns, rows]) = grid {
        // Shrinking a default 80×24 VT at startup would retain blank history rows.
        // Allocate the requested grid once so firstLineId names actual written output.
        config.columns = usize::from(columns);
        config.rows = usize::from(rows);
    }
    config
}
