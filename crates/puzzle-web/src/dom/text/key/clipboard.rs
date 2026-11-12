//! One ClipboardItem supplies styled paste and its plain alternative (§14.9).
//!
//! Construction and write run inside the original key callback; only promise
//! observation is deferred. A denied write logs no private clipboard data.
//!
//! ClipboardItem accepts promises of Blobs. Each Blob has the matching MIME type,
//! and each promise is already resolved because serialization completed in core.
//! This keeps the browser operation within user activation without awaiting a
//! parser, frame or network resource. The web contains no native clipboard handle.
//! A plain editor reads text/plain, while a rich editor can read text/html.
//! Neither format overwrites the other in a separate browser write transaction.

use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

/// Builds a rich item with both MIME entries; neither payload contains guessed text.
pub(crate) fn item(html: &str, text: &str) -> Result<web_sys::ClipboardItem, JsValue> {
    let data = js_sys::Object::new();
    for (mime, content) in [("text/html", html), ("text/plain", text)] {
        // Explicit Blob MIME types prevent the browser from treating HTML as plain tags.
        let options = web_sys::BlobPropertyBag::new();
        options.set_type(mime);
        let blob = web_sys::Blob::new_with_str_sequence_and_options(
            &js_sys::Array::of1(&JsValue::from_str(content)),
            &options,
        )?;
        js_sys::Reflect::set(
            &data,
            &JsValue::from_str(mime),
            &js_sys::Promise::resolve(&blob),
        )?;
    }
    web_sys::ClipboardItem::new_with_record_from_str_to_blob_promise(&data)
}

/// Awaiting starts only after the activation-sensitive write has already been called.
pub(crate) fn observe(promise: js_sys::Promise) {
    wasm_bindgen_futures::spawn_local(async move {
        if JsFuture::from(promise).await.is_err() {
            tracing::warn!(
                event = "web.clipboard_write_failed",
                "Clipboard write rejected"
            );
        }
    });
}

/// Browser MIME tests do not read or modify the operating-system clipboard.
#[cfg(test)]
mod tests;
