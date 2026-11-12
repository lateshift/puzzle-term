//! Actual browser MIME contents for the rich clipboard item.
use super::*;
use wasm_bindgen::JsCast;
/// Rich copy supplies plain text, never HTML tags, under the text/plain type.
#[wasm_bindgen_test::wasm_bindgen_test(async)]
async fn html_clipboard_item_has_plain_alternative() {
    let item = item("<pre><span>one &amp; two</span></pre>", "one & two").unwrap();
    assert_eq!(item.types().length(), 2);
    for (mime, expected) in [
        ("text/html", "<pre><span>one &amp; two</span></pre>"),
        ("text/plain", "one & two"),
    ] {
        let blob = JsFuture::from(item.get_type(mime))
            .await
            .unwrap()
            .dyn_into::<web_sys::Blob>()
            .unwrap();
        assert_eq!(blob.type_(), mime);
        assert_eq!(
            JsFuture::from(blob.text())
                .await
                .unwrap()
                .as_string()
                .unwrap(),
            expected
        );
    }
}
