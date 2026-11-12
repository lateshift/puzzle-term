//! A browser-reserved binding remains valid but must produce its promised warning.
//! Public create crosses the option/schema boundary; a parser-only report cannot
//! catch the host discarding availability diagnostics before tracing sees them.
use super::*;
use puzzle_term::bus::logging::MemoryLayer;
use tracing_subscriber::prelude::*;

/// Cmd+W belongs to browser chrome; preserving its binding cannot hide unavailability.
#[wasm_bindgen_test(async)]
async fn public_create_logs_reserved_shortcut() {
    let _fonts = resources::Fonts::new();
    let memory = MemoryLayer::default();
    let subscriber = tracing_subscriber::registry().with(memory.clone());
    // Browser tests run one at a time; the scoped guard restores the prior subscriber.
    let _guard = tracing::subscriber::set_default(subscriber);
    let document = window().unwrap().document().unwrap();
    let host = document
        .create_element("div")
        .unwrap()
        .dyn_into::<web_sys::HtmlElement>()
        .unwrap();
    host.style().set_property("width", "800px").unwrap();
    host.style().set_property("height", "480px").unwrap();
    document.body().unwrap().append_child(&host).unwrap();
    let keyboard = js_sys::Object::new();
    // Warn independently of the machine's OS table: the shared schema reserves both forms.
    let bindings = js_sys::Array::of2(&"super+w=ignore".into(), &"ctrl+w=ignore".into());
    js_sys::Reflect::set(&keyboard, &"bindings".into(), &bindings).unwrap();
    let settings = js_sys::Object::new();
    js_sys::Reflect::set(&settings, &"keyboard".into(), &keyboard).unwrap();
    let options = js_sys::Object::new();
    js_sys::Reflect::set(&options, &"settings".into(), &settings).unwrap();
    let terminal = PuzzleTerminal::create(host.clone(), Some(options.into()))
        .await
        .unwrap();
    let text = terminal.owner.borrow().text.clone().unwrap();
    JsFuture::from(super::super::resources::load_fonts(&text))
        .await
        .unwrap();
    let warnings: Vec<_> = memory
        .records()
        .into_iter()
        .filter(|record| {
            record
                .fields
                .get("event")
                .is_some_and(|name| name.contains("settings.binding_unavailable"))
        })
        .collect();
    assert_eq!(
        warnings.len(),
        1,
        "exactly the active OS shortcut is reserved"
    );
    assert_eq!(warnings[0].level, tracing::Level::WARN);
    assert!(warnings[0].fields["key"].contains("keyboard.bindings["));
    assert!(warnings[0].fields["reason"].contains("browser-reserved shortcut"));
    terminal.dispose().unwrap();
    host.remove();
}
