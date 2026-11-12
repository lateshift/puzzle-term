//! Optional browser keyboard APIs and OS default-table selection (§12.2).
//!
//! No previous key is learned: getLayoutMap is the only source of an exact
//! unmodified layout character. Missing APIs are normal in Safari/Firefox;
//! their Option-as-Alt fallback is the documented physical-US code table.
//! Plain Option text needs neither API nor permission.
//!
//! The user-agent platform chooses the default binding table, not an encoder
//! format. Applications still negotiate legacy/kitty modes independently.
//! Client Hints may be absent outside Chromium, so navigator.platform remains
//! the required fallback. Neither value is used for a security decision.
//!
//! These optional APIs are reflected rather than invoked unconditionally.
//! An unsupported method returns no callable Function; a supported method can
//! still reject because a page lacks permission or a secure context.
//! Those cases must not prevent normal terminal input from being mounted.
//!
//! Lock acquisition is different: the host explicitly requested ownership of
//! browser shortcuts, so an absent API or rejected promise is returned to it.
//! The adapter changes its routing policy only after that promise succeeds.
//! Disposal releases an acquired page-wide lock so another terminal/tab does
//! not inherit shortcut interception from a removed canvas.
use crate::dom::{DomHost, KeyboardState};
use js_sys::{Function, Promise, Reflect};
use puzzle_term::keybind::Platform;
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

/// Prefers UA Client Hints, then the legacy platform string, never wasm's OS.
pub fn platform() -> Platform {
    // A worker or detached test environment has no window; Linux is the neutral
    // Ctrl-primary fallback, while rendered terminals always have a navigator.
    let navigator = web_sys::window().map(|window| JsValue::from(window.navigator()));
    let value = navigator
        .and_then(|nav| {
            let hints = Reflect::get(&nav, &"userAgentData".into()).ok();
            hints
                .and_then(|hints| Reflect::get(&hints, &"platform".into()).ok()?.as_string())
                .or_else(|| Reflect::get(&nav, &"platform".into()).ok()?.as_string())
        })
        .unwrap_or_default()
        .to_ascii_lowercase();
    if value.contains("mac") || value.contains("iphone") || value.contains("ipad") {
        Platform::Mac
    } else if value.contains("win") {
        Platform::Windows
    } else {
        Platform::Linux
    }
}

/// Resolves a keyboard method through Reflect; unavailable APIs are not errors.
fn method(name: &str) -> Option<(JsValue, Function)> {
    let navigator = web_sys::window()?.navigator();
    let keyboard = Reflect::get(navigator.as_ref(), &"keyboard".into()).ok()?;
    let function = Reflect::get(&keyboard, &name.into())
        .ok()?
        .dyn_into()
        .ok()?;
    // Keep the receiver: WebIDL methods reject a call with the wrong `this`.
    Some((keyboard, function))
}

/// Installs browser-owned base characters after validating each map entry.
pub(super) async fn load<H: DomHost>(state: &Rc<RefCell<KeyboardState<H>>>) -> Result<(), JsValue> {
    let Some((keyboard, function)) = method("getLayoutMap") else {
        return Ok(());
    };
    // Chromium can deny access in insecure or restricted embedded contexts.
    let map = JsFuture::from(Promise::resolve(&function.call0(&keyboard)?)).await?;
    let Some(entries) = js_sys::try_iter(&map)? else {
        return Ok(());
    };
    let mut state = state
        .try_borrow_mut()
        .map_err(|_| JsValue::from_str("Input host is busy"))?;
    for entry in entries {
        // maplike keyboard layouts iterate [physical code, unmodified character].
        let pair = js_sys::Array::from(&entry?);
        if let (Some(code), Some(text)) = (pair.get(0).as_string(), pair.get(1).as_string()) {
            state.keyboard.set_layout_key(&code, &text);
        }
    }
    Ok(())
}

/// A successful promise, not the presence of navigator.keyboard, grants ownership.
pub(super) async fn lock<H: DomHost>(state: &Rc<RefCell<KeyboardState<H>>>) -> Result<(), JsValue> {
    let fullscreen = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.fullscreen_element());
    if fullscreen.is_none() {
        return Err(JsValue::from_str("Keyboard lock requires fullscreen"));
    }
    let (keyboard, function) =
        method("lock").ok_or_else(|| JsValue::from_str("Keyboard lock unavailable"))?;
    // Invoke before awaiting, so the caller's activation reaches the browser API.
    JsFuture::from(Promise::resolve(&function.call0(&keyboard)?)).await?;
    state
        .try_borrow_mut()
        .map_err(|_| JsValue::from_str("Input host is busy"))?
        .locked = true;
    Ok(())
}

/// Lock is page-wide, so disposal/unfullscreen must release it explicitly.
pub(super) fn unlock<H: DomHost>(state: &Rc<RefCell<KeyboardState<H>>>) -> Result<(), JsValue> {
    let mut state = state
        .try_borrow_mut()
        .map_err(|_| JsValue::from_str("Input host is busy"))?;
    if !state.locked {
        // Avoid releasing a page lock this particular terminal never acquired.
        return Ok(());
    }
    if let Some((keyboard, function)) = method("unlock") {
        function.call0(&keyboard)?;
    }
    state.locked = false;
    Ok(())
}
