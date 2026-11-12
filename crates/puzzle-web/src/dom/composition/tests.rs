//! Composition orders seen by the input adapter, independent of an installed IME.
use super::*;
use wasm_bindgen_test::wasm_bindgen_test;

/// Chromium's trailing input and Safari's pre-end final input type once.
#[wasm_bindgen_test]
fn composition_commit_orders() {
    for before_end in [true, false] {
        let mut state = Composition::default();
        assert!(matches!(state.start(), Command::Preedit(_)));
        for text in ["に", "日本"] {
            assert!(
                state
                    .input(text.into(), true)
                    .iter()
                    .all(|c| matches!(c, Command::Preedit(_)))
            );
        }
        if before_end {
            assert!(
                state
                    .input("日本".into(), false)
                    .iter()
                    .all(|c| matches!(c, Command::Preedit(_)))
            );
        }
        let commit = state.end("日本".into());
        assert!(matches!(&commit[0], Command::Preedit(p) if p.text.is_empty()));
        assert!(matches!(&commit[1], Command::Text(text) if text == "日本"));
        if !before_end {
            assert!(state.input("日本".into(), false).is_empty());
        }
        state.new_key();
        assert!(matches!(
            &state.input("日本".into(), false)[0],
            Command::Text(_)
        ));
    }
}

/// Cancellation and non-BMP caret offsets must not fabricate input or split UTF-8.
#[wasm_bindgen_test]
fn composition_cancel_and_unicode_caret() {
    let mut state = Composition::default();
    state.start();
    assert_eq!(state.end(String::new()).len(), 1);
    assert_eq!(utf8_offset("a😀日", 3), Some(5));
    assert_eq!(utf8_offset("a😀日", 2), None);
    assert_eq!(utf8_offset("日", 9), None);
    // Dead-key commits need no composition transaction to become text.
    assert!(matches!(&state.input("é".into(), false)[0], Command::Text(text) if text == "é"));
}
