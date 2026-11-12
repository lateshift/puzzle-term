//! Resource lifetime witnesses request real WebGPU devices in Chrome.
//!
//! Font fetches are replaced only inside these tests: the test server does
//! not serve repository assets. Production always fetches external files.
use super::*;

/// Owns a temporary fetch replacement and restores the page on teardown.
pub(super) struct Fonts {
    /// Preserve the browser's actual networking method for later tests.
    original: JsValue,
    /// The installed function must outlive asynchronous resource creation.
    _callback: Closure<dyn FnMut(JsValue) -> Promise>,
}

/// Replacement lifetime is confined to one browser witness.
impl Fonts {
    /// Supplies approved bundled fixture bytes without introducing a test server.
    pub(super) fn new() -> Self {
        Self::with_corrupt_pack(false)
    }

    /// Corrupt bytes exercise the loader's specified fallback without changing shipped assets.
    fn with_corrupt_pack(corrupt: bool) -> Self {
        let window = window().unwrap();
        let original = value::get(window.as_ref(), "fetch").unwrap();
        let callback = Closure::wrap(Box::new(move |path: JsValue| {
            if corrupt && path.as_string().unwrap() == "jetbrains-regular.pzfp" {
                return Promise::resolve(&JsValue::from(
                    web_sys::Response::new_with_opt_str(Some("broken pack")).unwrap(),
                ));
            }
            let bytes: &[u8] = match path.as_string().unwrap().as_str() {
                "jetbrains-regular.pzfp" => {
                    include_bytes!("../../../../../assets/fontpacks/jetbrains-regular.pzfp")
                }
                "jetbrains-bold.pzfp" => {
                    include_bytes!("../../../../../assets/fontpacks/jetbrains-bold.pzfp")
                }
                "jetbrains-italic.pzfp" => {
                    include_bytes!("../../../../../assets/fontpacks/jetbrains-italic.pzfp")
                }
                "jetbrains-bold-italic.pzfp" => {
                    include_bytes!("../../../../../assets/fontpacks/jetbrains-bold-italic.pzfp")
                }
                "JetBrainsMono-Variable.ttf" => {
                    include_bytes!("../../../../../assets/fonts/JetBrainsMono-Variable.ttf")
                }
                "JetBrainsMono-Italic-Variable.ttf" => {
                    include_bytes!("../../../../../assets/fonts/JetBrainsMono-Italic-Variable.ttf")
                }
                "SymbolsNerdFontMono-Regular.ttf" => {
                    include_bytes!("../../../../../assets/fonts/SymbolsNerdFontMono-Regular.ttf")
                }
                _ => panic!("unexpected asset request"),
            };
            let mut bytes = bytes.to_vec();
            Promise::resolve(&JsValue::from(
                web_sys::Response::new_with_opt_u8_array(Some(&mut bytes)).unwrap(),
            ))
        }) as Box<dyn FnMut(JsValue) -> Promise>);
        js_sys::Reflect::set(window.as_ref(), &"fetch".into(), callback.as_ref()).unwrap();
        Self {
            original,
            _callback: callback,
        }
    }
}

/// Test fetches cannot escape into other browser witnesses.
impl Drop for Fonts {
    /// All requested font promises settle before restoring the native function.
    fn drop(&mut self) {
        js_sys::Reflect::set(window().unwrap().as_ref(), &"fetch".into(), &self.original).unwrap();
    }
}

/// Creates through the public surface, not a fabricated registry resource.
pub(super) async fn create(size: u32) -> (PuzzleTerminal, web_sys::HtmlElement) {
    let document = window().unwrap().document().unwrap();
    let host = document
        .create_element("div")
        .unwrap()
        .dyn_into::<web_sys::HtmlElement>()
        .unwrap();
    host.style().set_property("width", "800px").unwrap();
    host.style().set_property("height", "480px").unwrap();
    document.body().unwrap().append_child(&host).unwrap();
    let font = js_sys::Object::new();
    js_sys::Reflect::set(&font, &"size".into(), &size.into()).unwrap();
    let settings = js_sys::Object::new();
    js_sys::Reflect::set(&settings, &"font".into(), &font).unwrap();
    let options = js_sys::Object::new();
    js_sys::Reflect::set(&options, &"settings".into(), &settings).unwrap();
    let terminal = PuzzleTerminal::create(host.clone(), Some(options.into()))
        .await
        .unwrap();
    let text = terminal.owner.borrow().text.clone().unwrap();
    JsFuture::from(super::super::resources::load_fonts(&text))
        .await
        .unwrap();
    (terminal, host)
}

/// Same page and complete settings key must not duplicate fonts or GPU state.
#[wasm_bindgen_test(async)]
async fn two_terminals_share_device_and_context() {
    let _fonts = Fonts::new();
    let supported = PuzzleTerminal::is_supported().await.unwrap();
    assert_eq!(value::get(&supported, "webgpu").unwrap(), JsValue::TRUE);
    let (a, ha) = create(14).await;
    let (b, hb) = create(14).await;
    let ta = a.owner.borrow().text.clone().unwrap();
    let tb = b.owner.borrow().text.clone().unwrap();
    assert!(Rc::ptr_eq(&ta, &tb));
    assert!(Rc::ptr_eq(&ta.gpu, &tb.gpu));
    // Native loss must remain pending while another terminal still owns the device.
    let device = ta
        .gpu
        .renderer
        .borrow()
        .as_ref()
        .unwrap()
        .device()
        .as_webgpu()
        .unwrap()
        .clone();
    let lost = value::get(device.as_ref(), "lost")
        .unwrap()
        .dyn_into::<Promise>()
        .unwrap();
    let observed = Rc::new(std::cell::Cell::new(false));
    let flag = observed.clone();
    let callback =
        Closure::wrap(Box::new(move |_: JsValue| flag.set(true)) as Box<dyn FnMut(JsValue)>);
    let _ = lost.then(&callback);
    drop(ta);
    drop(tb);
    a.resize(100.into(), 30.into()).unwrap();
    assert_eq!((a.cols().unwrap(), a.rows().unwrap()), (100, 30));
    assert_ne!(a.cols().unwrap(), b.cols().unwrap());
    a.fit().unwrap();
    a.dispose().unwrap();
    JsFuture::from(Promise::resolve(&JsValue::UNDEFINED))
        .await
        .unwrap();
    assert!(
        !observed.get(),
        "disposing one pane destroyed the shared device"
    );
    b.dispose().unwrap();
    JsFuture::from(lost).await.unwrap();
    ha.remove();
    hb.remove();
}

/// Hinted ppem is part of identity, while the device stays page scoped.
#[wasm_bindgen_test(async)]
async fn different_font_sizes_have_separate_contexts() {
    let _fonts = Fonts::new();
    let (a, ha) = create(14).await;
    let (b, hb) = create(18).await;
    let ta = a.owner.borrow().text.clone().unwrap();
    let tb = b.owner.borrow().text.clone().unwrap();
    assert!(!Rc::ptr_eq(&ta, &tb));
    assert!(Rc::ptr_eq(&ta.gpu, &tb.gpu));
    a.dispose().unwrap();
    b.dispose().unwrap();
    ha.remove();
    hb.remove();
}

/// The registry must not pin resources after the last terminal is disposed.
#[wasm_bindgen_test(async)]
async fn last_dispose_releases_resources() {
    let _fonts = Fonts::new();
    let (a, host) = create(14).await;
    let text = a.owner.borrow().text.clone().unwrap();
    let weak_text = Rc::downgrade(&text);
    let weak_gpu = Rc::downgrade(&text.gpu);
    // Rust weak expiry alone is insufficient: wgpu's WebGPU Drop does not destroy
    // the JS device. Last disposal must end its native incarnation explicitly.
    let device = text
        .gpu
        .renderer
        .borrow()
        .as_ref()
        .unwrap()
        .device()
        .as_webgpu()
        .unwrap()
        .clone();
    let lost = value::get(device.as_ref(), "lost")
        .unwrap()
        .dyn_into::<Promise>()
        .unwrap();
    drop(text);
    a.dispose().unwrap();
    // Initialization promises can retain an owner until their completed
    // microtask drops. Await a microtask before observing weak ownership.
    JsFuture::from(Promise::resolve(&JsValue::UNDEFINED))
        .await
        .unwrap();
    assert!(weak_text.upgrade().is_none());
    assert!(weak_gpu.upgrade().is_none());
    JsFuture::from(lost).await.unwrap();
    host.remove();
}

/// Destroying the actual WebGPU device resolves its loss promise.
#[wasm_bindgen_test(async)]
async fn device_loss_creates_fresh_device() {
    // Real served fonts and packs must populate the fresh GPU incarnation.
    let (a, ha) = create(14).await;
    let old = a.owner.borrow().text.clone().unwrap();
    let device = old
        .gpu
        .renderer
        .borrow()
        .as_ref()
        .unwrap()
        .device()
        .as_webgpu()
        .unwrap()
        .clone();
    let lost = value::get(device.as_ref(), "lost")
        .unwrap()
        .dyn_into::<Promise>()
        .unwrap();
    value::get(device.as_ref(), "destroy")
        .unwrap()
        .dyn_into::<Function>()
        .unwrap()
        .call0(device.as_ref())
        .unwrap();
    JsFuture::from(lost).await.unwrap();
    JsFuture::from(Promise::resolve(&JsValue::UNDEFINED))
        .await
        .unwrap();
    let (b, hb) = create(14).await;
    let new = b.owner.borrow().text.clone().unwrap();
    assert!(!Rc::ptr_eq(&old.gpu, &new.gpu));
    // A device identity alone cannot prove its newly allocated atlas has ink.
    b.focus().unwrap();
    let alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    JsFuture::from(
        b.write(format!("\x1b[?25h\x1b[2 q{alphabet}").into())
            .unwrap(),
    )
    .await
    .unwrap();
    let pixels = super::presentation::presented_pixels(&b).await;
    let ink = super::presentation::assert_cells(&b, &pixels, &[alphabet], 52, 0);
    a.dispose().unwrap();
    b.dispose().unwrap();
    ha.remove();
    hb.remove();
    assert!(ink.is_ok(), "{}", ink.unwrap_err());
}

/// The production rAF path submits real ink at exact device-pixel canvas dimensions.
#[wasm_bindgen_test(async)]
async fn rendered_canvas_backing_and_first_frame() {
    let _fonts = Fonts::new();
    let (terminal, host) = create(14).await;
    JsFuture::from(terminal.write("\x1b[?25lWebGPU ->".into()).unwrap())
        .await
        .unwrap();
    let canvas = terminal
        .owner
        .borrow()
        .dom
        .as_ref()
        .unwrap()
        .canvas()
        .clone();
    let pixels = submitted_pixels(&terminal, &canvas).await;
    let dpr = terminal.owner.borrow().dpr;
    assert_eq!(canvas.width(), (800.0 * dpr).round() as u32);
    assert_eq!(canvas.height(), (480.0 * dpr).round() as u32);
    let background = &pixels[..4];
    assert_eq!(background[3], 255);
    let ink = pixels
        .chunks_exact(4)
        .filter(|px| *px != background)
        .count();
    assert!(
        ink > 100 && ink < pixels.len() / 16,
        "blank or smeared submitted frame: {ink}"
    );
    terminal.dispose().unwrap();
    host.remove();
}

/// Read the submitted texture before browser composition discards WebGPU's drawing buffer.
async fn submitted_pixels(
    terminal: &PuzzleTerminal,
    canvas: &web_sys::HtmlCanvasElement,
) -> Vec<u8> {
    // drawImage observes the submitted WebGPU canvas without adding a renderer path.
    let document = window().unwrap().document().unwrap();
    let copy = document
        .create_element("canvas")
        .unwrap()
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .unwrap();
    copy.set_width(canvas.width());
    copy.set_height(canvas.height());
    let ctx = copy
        .get_context("2d")
        .unwrap()
        .unwrap()
        .dyn_into::<web_sys::CanvasRenderingContext2d>()
        .unwrap();
    ctx.draw_image_with_html_canvas_element(&canvas, 0.0, 0.0)
        .unwrap();
    let start = Instant::now();
    loop {
        // WebGPU releases its current texture after browser composition. Read in
        // the same refresh turn as submission, before that discard can happen.
        JsFuture::from(Promise::new(&mut |resolve, _| {
            window().unwrap().request_animation_frame(&resolve).unwrap();
        }))
        .await
        .unwrap();
        copy.set_width(canvas.width());
        copy.set_height(canvas.height());
        ctx.draw_image_with_html_canvas_element(&canvas, 0.0, 0.0)
            .unwrap();
        let pixels = ctx
            .get_image_data(0.0, 0.0, f64::from(copy.width()), f64::from(copy.height()))
            .unwrap()
            .data()
            .0;
        if pixels[3] == 255 {
            break pixels;
        }
        assert!(
            start.elapsed().as_secs() < 5,
            "no opaque submitted GPU canvas"
        );
        // Retry with actual damage; each presentation must paint the text as well as clear.
        JsFuture::from(terminal.write("\rWebGPU ->".into()).unwrap())
            .await
            .unwrap();
    }
}

/// Invalid prebuilt data cannot prevent startup with the same bundled font family.
#[wasm_bindgen_test(async)]
async fn corrupt_pack_falls_back_to_external_fonts() {
    let _fonts = Fonts::with_corrupt_pack(true);
    let (terminal, host) = create(17).await;
    let text = terminal.owner.borrow().text.clone().unwrap();
    assert!(text.context.borrow().as_ref().unwrap().has_font_tables());
    assert!(terminal.cols().unwrap() > 0);
    terminal.dispose().unwrap();
    host.remove();
}

/// A budget reload selects another owner without shrinking a sibling's atlas.
#[wasm_bindgen_test(async)]
async fn budget_reload_keeps_other_shared_owner() {
    let _fonts = Fonts::new();
    let (a, ha) = create(14).await;
    let (b, hb) = create(14).await;
    let old = a.owner.borrow().text.clone().unwrap();
    assert!(Rc::ptr_eq(&old, b.owner.borrow().text.as_ref().unwrap()));
    {
        let mut runtime = a.owner.borrow_mut();
        runtime.store.apply_key(
            "render.glyphMemoryMb",
            puzzle_term::bus::Value::Number(28.0),
            puzzle_term::settings::Layer::Session,
        );
        super::super::settings::apply(&mut runtime);
    }
    super::super::settings::select_text(&a.owner);
    let start = Instant::now();
    loop {
        JsFuture::from(Promise::new(&mut |resolve, _| {
            window().unwrap().request_animation_frame(&resolve).unwrap();
        }))
        .await
        .unwrap();
        let current = a.owner.borrow().text.clone().unwrap();
        if !Rc::ptr_eq(&old, &current) {
            assert!(Rc::ptr_eq(&old.gpu, &current.gpu));
            assert_eq!(
                current
                    .context
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .glyph_budget_bytes(),
                28 * 1024 * 1024
            );
            break;
        }
        assert!(
            start.elapsed().as_secs() < 10,
            "budget context not selected"
        );
    }
    assert!(Rc::ptr_eq(&old, b.owner.borrow().text.as_ref().unwrap()));
    assert_eq!(
        old.context.borrow().as_ref().unwrap().glyph_budget_bytes(),
        48 * 1024 * 1024
    );
    a.dispose().unwrap();
    b.dispose().unwrap();
    ha.remove();
    hb.remove();
}
