//! The sanctioned weak page registry contains exactly GPU and text (§4.6).
//!
//! Initialization belongs to the resource itself, so simultaneous creates
//! await one request rather than racing two devices. The registry never owns
//! resources strongly; dropping the last terminal releases the shared state.
//!
//! A WebGPU device and its queue are inseparable: texture uploads on one
//! queue cannot make another device's textures valid. The registry therefore
//! returns their renderer owner as one resource rather than caching separate
//! adapter, device and queue objects with independent lifetimes.
//!
//! Initialization is published before yielding to JavaScript. A second
//! create sees the weak resource identity and awaits its ready promise.
//! That promise is a field of the resource, not a third registry category.
//! After it settles, only terminals and their text owners keep strong refs.
//!
//! Text contexts additionally own the GPU atlas textures. A glyph inserted
//! by one Session produces an upload span in its first frame. Sharing the
//! painter lets another Session draw that address without attempting to
//! replay only its own dirty spans into an empty texture.
//!
//! Context keys distinguish the glyph inputs that can change geometry.
//! A different DPR or hinting policy cannot reuse a ppem-specific outline
//! prepared for another display. Per-session row caches remain in Session;
//! only fonts, shaping caches, atlas mirrors and textures live here.
//!
//! A lost device is an obsolete resource incarnation, even if a terminal
//! still owns it. Its WebGPU lost promise marks it unusable for new creates.
//! Removal compares identities so a delayed old-device callback cannot
//! erase a newer device that another create has already installed.
//!
//! Weak text entries are pruned on lookup. A text context also retains its
//! GPU owner, so dropping the last terminal for that context releases both
//! its CPU atlas and its texture resources when no other context needs them.
//! The registry alone cannot keep an otherwise disposed page alive.
use super::{value::error, window};
use js_sys::Promise;
use puzzle_render::{FrameRenderer, Renderer};
use puzzle_term::frame_builder::context::TextContext;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::{Rc, Weak},
};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::{JsFuture, future_to_promise, spawn_local};

/// The page's adapter, device and queue form one shared incarnation.
pub(super) struct Gpu {
    /// Surfaces are created by this platform owner, not by core crates (R9).
    pub(super) instance: wgpu::Instance,
    /// Becomes available once ready resolves; remains shared by every canvas.
    pub(super) renderer: RefCell<Option<Renderer>>,
    /// Concurrent creates await the resource's own initialization.
    ready: RefCell<Option<Promise>>,
    /// Device loss prevents another terminal from reusing a dead device.
    lost: Cell<bool>,
}

/// End the device incarnation when its last terminal or text owner releases it.
impl Drop for Gpu {
    /// wgpu's WebGPU Device drop is a no-op, leaving native cleanup to browser GC.
    /// Explicit destruction ends outstanding device work before JS finalizers run;
    /// shared terminals remain unaffected because only the last Rc invokes Drop.
    fn drop(&mut self) {
        if let Some(renderer) = self.renderer.get_mut().as_ref() {
            renderer.device().destroy();
        }
    }
}

/// Text geometry and its GPU atlas textures have the same shared lifetime.
pub(super) struct Text {
    /// Browser readbacks reuse one lazy CPU-oriented canvas for this text context.
    readback: RefCell<readback::Surface>,
    /// Weak sessions receive font completion without a strong ownership cycle.
    pub(super) sessions: RefCell<Vec<Weak<RefCell<super::Runtime>>>>,
    /// Source drivers refer to this resource-owned browser completion listener by name.
    fallback: RefCell<Option<fallback::Wake>>,
    /// Scalar index measurements belong to the same resource lifetime as discovery.
    pub(super) fallback_stats: RefCell<Vec<std::sync::Arc<super::fallback::idle::Stats>>>,
    /// One CPU atlas serves sessions with identical text settings.
    pub(super) context: RefCell<Option<TextContext>>,
    /// Uploads drained by one session remain visible to every shared canvas.
    pub(super) painter: RefCell<Option<FrameRenderer>>,
    /// A context cannot upload textures to a different device incarnation.
    pub(super) gpu: Rc<Gpu>,
    /// Browser family policy follows full-font takeover and settings-key changes.
    families: Vec<String>,
    /// Initialization stays with the resource, never a third registry category.
    ready: RefCell<Option<Promise>>,
    /// Full font downloads and parsing are shared across simultaneous creates.
    fonts: RefCell<Option<Promise>>,
}

/// Registry entries are discoverability only, not global terminal ownership.
#[derive(Default)]
struct Registry {
    /// There is at most one live device per page.
    gpu: Weak<Gpu>,
    /// Font options, DPR and hinting select an independent CPU/GPU atlas.
    text: HashMap<String, Weak<Text>>,
}

thread_local! {
    /// Approved page-only exception; no sessions, callbacks or transports live here.
    static REGISTRY: RefCell<Registry> = RefCell::new(Registry::default());
}

/// Gets the page device, inserting its weak identity before any await.
pub(super) fn gpu() -> Rc<Gpu> {
    let gpu = REGISTRY.with(|registry| {
        // Registry lookup does not create ownership; callers retain the returned strong resource.
        let mut registry = registry.borrow_mut();
        if let Some(gpu) = registry.gpu.upgrade().filter(|gpu| !gpu.lost.get()) {
            return gpu;
        }
        let gpu = Rc::new(Gpu {
            instance: wgpu::Instance::default(),
            renderer: RefCell::new(None),
            ready: RefCell::new(None),
            lost: Cell::new(false),
        });
        registry.gpu = Rc::downgrade(&gpu);
        gpu
    });
    // Publish initialization before the first await so concurrent creates share its promise.
    if gpu.ready.borrow().is_none() {
        let weak = Rc::downgrade(&gpu);
        let promise = future_to_promise(async move {
            let gpu = weak
                .upgrade()
                .ok_or_else(|| error("Disposed", "GPU owner released"))?;
            // Adapter/device creation is asynchronous browser work, parallel with pack fetch.
            let renderer = Renderer::new(&gpu.instance, None)
                .await
                .map_err(|e| error("WebGpuUnavailable", &e.to_string()))?;
            // Device loss invalidates lookup even while existing terminals hold old resources.
            watch_loss(&gpu, &renderer)?;
            *gpu.renderer.borrow_mut() = Some(renderer);
            Ok(JsValue::UNDEFINED)
        });
        *gpu.ready.borrow_mut() = Some(promise);
    }
    gpu
}

/// Waits for the shared request while font fetching proceeds independently (P7).
pub(super) async fn wait_gpu(gpu: &Rc<Gpu>) -> Result<(), JsValue> {
    let ready = gpu
        .ready
        .borrow()
        .clone()
        .ok_or_else(|| error("WebGpuUnavailable", "GPU initialization missing"))?;
    // Concurrent callers observe the same success or rejection without another network request.
    JsFuture::from(ready).await?;
    Ok(())
}

/// WebGPU's lost promise allows Weak capture without unsafe Send wrappers.
fn watch_loss(gpu: &Rc<Gpu>, renderer: &Renderer) -> Result<(), JsValue> {
    // WebGPU exposes loss through its native promise; no unsafe callback Send wrapper is needed.
    let device = renderer
        .device()
        .as_webgpu()
        .ok_or_else(|| error("WebGpuUnavailable", "device is not WebGPU"))?;
    let promise = super::value::get(device.as_ref(), "lost")?.dyn_into::<Promise>()?;
    let weak = Rc::downgrade(gpu);
    spawn_local(async move {
        let info = JsFuture::from(promise).await;
        // GPUDeviceLostInfo carries the browser's diagnosis; Reflect uses the
        // existing JS boundary without enabling another web-sys feature.
        let field = |name| {
            info.as_ref()
                .ok()
                .and_then(|info| super::value::get(info, name).ok())
                .and_then(|v| v.as_string())
                .unwrap_or_default()
        };
        let reason = field("reason");
        let message = field("message");
        // Once all owners are gone, the callback must not recreate page state.
        if let Some(gpu) = weak.upgrade() {
            gpu.lost.set(true);
            REGISTRY.with(|registry| {
                // Registry lookup does not create ownership; callers retain the returned strong resource.
                let mut registry = registry.borrow_mut();
                if registry
                    .gpu
                    .upgrade()
                    .is_some_and(|current| Rc::ptr_eq(&current, &gpu))
                {
                    registry.gpu = Weak::new();
                }
            });
            tracing::warn!(
                event = "web.device_lost",
                %reason,
                %message,
                "next create will request a fresh device"
            );
        }
    });
    Ok(())
}

/// Fetches exact bytes; fonts and packs are served beside the wasm, never embedded.
pub(super) async fn fetch(path: &str) -> Result<Vec<u8>, JsValue> {
    let response = JsFuture::from(window()?.fetch_with_str(path))
        .await?
        .dyn_into::<web_sys::Response>()?;
    // HTTP errors are font failures, not malformed-font parse attempts on an error document.
    if !response.ok() {
        return Err(error(
            "FontLoadFailed",
            &format!("fetch {path}: HTTP {}", response.status()),
        ));
    }
    // Copy the fetched byte buffer into the platform-owned FontData boundary.
    let buffer = JsFuture::from(response.array_buffer()?).await?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}

/// Gets one text context and texture set for a complete font-settings key.
pub(super) async fn text(
    gpu: Rc<Gpu>,
    key: String,
    size: f32,
    dpr: f32,
    families: Vec<String>,
    glyph_memory_mb: u32,
) -> Result<Rc<Text>, JsValue> {
    let text = REGISTRY.with(|registry| {
        // Registry lookup does not create ownership; callers retain the returned strong resource.
        let mut registry = registry.borrow_mut();
        // Dead keys cannot accumulate as permanent cache entries across repeated zooms.
        registry.text.retain(|_, weak| weak.strong_count() != 0);
        // A device replacement makes otherwise identical font settings a new GPU incarnation.
        if let Some(text) = registry
            .text
            .get(&key)
            .and_then(Weak::upgrade)
            .filter(|text| Rc::ptr_eq(&text.gpu, &gpu))
        {
            return text;
        }
        let text = Rc::new(Text {
            readback: RefCell::new(Default::default()),
            sessions: RefCell::new(Vec::new()),
            fallback: RefCell::new(None),
            fallback_stats: RefCell::new(Vec::new()),
            context: RefCell::new(None),
            painter: RefCell::new(None),
            gpu,
            ready: RefCell::new(None),
            fonts: RefCell::new(None),
            // Retain policy until asynchronous full-font takeover can install sources.
            families,
        });
        registry.text.insert(key, Rc::downgrade(&text));
        text
    });
    if text.fallback.borrow().is_none() {
        Text::install_fallback_wake(&text)?;
    }
    // A shared startup promise prevents two terminals from decoding the same pack concurrently.
    if text.ready.borrow().is_none() {
        let weak = Rc::downgrade(&text);
        let ready = future_to_promise(async move {
            let mut context = initial_text(size, dpr).await?;
            // Budget participates in the registry key; other terminals keep their owner.
            context
                .set_glyph_memory_mb(glyph_memory_mb)
                .map_err(|e| error("InvalidArgument", &e))?;
            let text = weak
                .upgrade()
                .ok_or_else(|| error("Disposed", "text owner released"))?;
            // Corrupt-pack recovery may already have tables before this first frame.
            if context.has_font_tables() {
                fallback::install_sources(&mut context, &text)?;
            }
            // The resource becomes usable atomically after pack validation and metrics succeed.
            *text.context.borrow_mut() = Some(context);
            Ok(JsValue::UNDEFINED)
        });
        *text.ready.borrow_mut() = Some(ready);
    }
    let ready = text
        .ready
        .borrow()
        .clone()
        .ok_or_else(|| error("FontLoadFailed", "text initialization missing"))?;
    // Concurrent callers observe the same success or rejection without another network request.
    JsFuture::from(ready).await?;
    Ok(text)
}

/// A corrupt or unavailable startup pack falls back to the approved external fonts (§8.8).
/// Packs are derived caches: a stale deployment must still be able to load its fonts.
/// The normal path imports ready atlas planes without waiting for outline extraction.
/// Only a failed pack switches startup to runtime preparation from full font tables.
/// That slower path uses the same bundled family, so fallback does not change metrics.
/// Neither path publishes a context until validation and construction have succeeded.
/// A partially decoded plane therefore cannot become visible to another terminal.
/// The returned fallback already owns its tables; load_fonts recognizes that state
/// and avoids fetching and parsing the family again after the first frame.
async fn initial_text(size: f32, dpr: f32) -> Result<TextContext, JsValue> {
    let packed: Result<TextContext, JsValue> = async {
        let bytes = fetch("jetbrains-regular.pzfp").await?;
        // Validate every plane/address before importing any glyph handles.
        let pack = puzzle_term::text::pack::FontPack::decode(&bytes)
            .map_err(|e| error("FontLoadFailed", &e))?;
        let mut context =
            TextContext::from_pack(pack, size, dpr).map_err(|e| error("FontLoadFailed", &e))?;
        packs::styles(&mut context).await?;
        Ok(context)
    }
    .await;
    match packed {
        Ok(context) => Ok(context),
        Err(cause) => {
            // Bad cache data delays startup, but must not prevent a usable terminal.
            tracing::warn!(event = "font.pack_fallback", ?cause);
            let regular = fetch("JetBrainsMono-Variable.ttf").await?;
            let italic = fetch("JetBrainsMono-Italic-Variable.ttf").await?;
            let symbols = fetch("SymbolsNerdFontMono-Regular.ttf").await?;
            // I/O remains in the platform; the neutral font system receives owned bytes only.
            let fonts = puzzle_term::text::font_set::FontSet::new(
                puzzle_term::text::FontData::new(regular, 0),
                Some(puzzle_term::text::FontData::new(italic, 0)),
                Some(puzzle_term::text::FontData::new(symbols, 0)),
            )
            .map_err(|e| error("FontLoadFailed", &e))?;
            TextContext::new(fonts, size, dpr).map_err(|e| error("FontLoadFailed", &e))
        }
    }
}

/// Fetches the complete bundled family once for this shared context.
pub(super) fn load_fonts(text: &Rc<Text>) -> Promise {
    // The fallback already loaded tables; it must not fetch the family a second time.
    if text
        .context
        .borrow()
        .as_ref()
        .is_some_and(TextContext::has_font_tables)
    {
        return Promise::resolve(&JsValue::UNDEFINED);
    }
    // A second terminal joins an in-flight family download rather than loading duplicate faces.
    if let Some(promise) = text.fonts.borrow().clone() {
        return promise;
    }
    let weak = Rc::downgrade(text);
    let promise = future_to_promise(async move {
        // External font assets remain outside the wasm engine and platform-neutral crates.
        let regular = fetch("JetBrainsMono-Variable.ttf").await?;
        let italic = fetch("JetBrainsMono-Italic-Variable.ttf").await?;
        let symbols = fetch("SymbolsNerdFontMono-Regular.ttf").await?;
        let text = weak
            .upgrade()
            .ok_or_else(|| error("Disposed", "text owner released"))?;
        // Full-font takeover preserves cell geometry and invalidates per-session row handles.
        let mut context = text.context.borrow_mut();
        let context = context
            .as_mut()
            .ok_or_else(|| error("FontLoadFailed", "pack missing"))?;
        // Source-hash checking rejects a font that would reinterpret the startup pack glyph IDs.
        context
            .load_fonts(
                puzzle_term::text::FontData::new(regular, 0),
                Some(puzzle_term::text::FontData::new(italic, 0)),
                Some(puzzle_term::text::FontData::new(symbols, 0)),
            )
            .map_err(|e| error("FontLoadFailed", &e))?;
        // Both cluster policies enter together before sessions rebuild their rows.
        fallback::install_sources(context, &text)?;
        Ok(JsValue::UNDEFINED)
    });
    // A rejected family fetch is shared too: another pane must not silently retry every frame.
    *text.fonts.borrow_mut() = Some(promise.clone());
    promise
}

/// Completion listeners belong to a text resource, outside the weak page registry.
mod fallback;

/// All four approved startup styles retain pack ink through font takeover.
mod packs;
/// Readback Canvas lifetime follows existing weak text-resource ownership.
pub(super) mod readback;
