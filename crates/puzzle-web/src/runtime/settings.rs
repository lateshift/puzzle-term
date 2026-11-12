//! Platform settings actions never mutate another terminal's shared context.
//!
//! Zoom and monitor changes select a new D-30 key. The old text resource stays
//! usable until the replacement pack arrives, so input and parsing keep running.
//! Only this host owns layers; the registry contains no settings stores (D-30).
//!
//! The binding interceptor returns typed actions after its synchronous turn.
//! Zoom changes the terminal's session layer and requests a matching text key;
//! it must never mutate the context another pane is still using. A new context
//! can reuse the page device but has independent metrics and prepared glyphs.
//!
//! Resource selection may await a network fetch. During that wait the old
//! context remains drawable, and the user's next key still reaches transport.
//! Another zoom can supersede the outstanding request, so completion compares
//! the requested key with the current settings rather than blindly installing it.
//! The registry remains weak and does not own per-terminal settings or callbacks.
//!
//! Reload here only recomputes the already validated local layers. A persistent
//! browser store and its asynchronous storage adapter belong to M10. Profiles,
//! however, already exist in the neutral store and must use the same font-key
//! selection rule when they change size, DPR-dependent hinting or other font input.
use super::*;
use puzzle_term::bus::Value;
use puzzle_term::{
    keybind::Action,
    settings::{Layer, Settings, ThemeMode},
};

/// A complete font key prevents ppem-specific contours crossing display scales.
pub(super) fn text_key(settings: &Settings, dpr: f64) -> String {
    // Build from the catalogue so a newly added font setting cannot be missed in identity.
    let font: Vec<_> = Settings::schema()
        .iter()
        .filter(|def| def.key.starts_with("font.") || def.key == "render.glyphMemoryMb")
        .filter_map(|def| settings.value(def.key).map(|v| (def.key, v)))
        .collect();
    // This is an internal lookup identity, never a wire type or persisted settings format.
    format!("{font:?}:{dpr}")
}

/// Drains host actions after the binding interceptor releases its borrow.
pub(super) fn actions(owner: &Owner) {
    loop {
        // Taking one action releases Session's borrow before resource work can begin.
        let action = owner.borrow_mut().take_binding_action();
        // Toggle uses Session's execution-time appearance, even if output changed it later.
        let Some((action, _all, dark)) = action else {
            return;
        };
        let changed = {
            let mut r = owner.borrow_mut();
            let before = text_key(&r.settings, r.dpr);
            apply_action(&mut r, action, dark);
            before != text_key(&r.settings, r.dpr)
        };
        // Color-only changes keep geometry and prepared glyphs; font changes select resources.
        if changed {
            select_text(owner);
        }
    }
}

/// Mutates only the owning terminal's layers, then resolves the shared schema.
fn apply_action(r: &mut Runtime, action: Action, dark: bool) {
    // Profiles and reloads use the same final font-identity comparison as zoom.
    let entry = match action {
        Action::Zoom(steps) => Some((
            "font.size",
            Value::Number(f64::from(r.settings.font_size) + f64::from(steps)),
        )),
        // Reset uses the product default, independent of desktop's run-only fallback.
        Action::ZoomReset => Some(("font.size", Value::Number(14.0))),
        Action::SetAppearance(mode) => Some((
            "theme.mode",
            Value::String(
                match mode {
                    ThemeMode::Auto => "auto",
                    ThemeMode::Light => "light",
                    ThemeMode::Dark => "dark",
                }
                .into(),
            ),
        )),
        // Auto may currently select either appearance; toggle the resolved one.
        Action::ToggleAppearance => Some((
            "theme.mode",
            Value::String(if dark { "light" } else { "dark" }.into()),
        )),
        Action::Profile(name) => {
            // Profile changes are resolved before selecting a replacement text context.
            r.store.activate_profile(&name);
            None
        }
        // Persistent browser storage is M10; M7 reloads the existing validated layers.
        Action::ReloadSettings => None,
        _ => return,
    };
    if let Some((key, value)) = entry {
        // Repeated zoom stays within schema limits before selecting any GPU/text resources.
        r.store.apply_key(key, value, Layer::Session);
    }
    apply(r);
}

/// Reduced motion is a host policy, not a replacement user-layer value.
pub(super) fn apply(r: &mut Runtime) {
    // Retain the actual user choice separately from the transient reduced-motion override.
    r.settings = r.store.effective().clone();
    let mut effective = r.settings.clone();
    if r.reduced_motion {
        effective.theme_transition_ms = 0;
    }
    // Core theme installation owns mode-2031 replies, cursor precedence and style epochs.
    if let Err(error) = r.apply(puzzle_term::SessionMessage::Settings(Box::new(effective))) {
        tracing::warn!(event = "web.settings_failed", %error);
    }
    // Canonical merge and browser-reserved warnings remain shared with the native path.
    let bindings = r.store.keybindings();
    if let Err(error) = r.apply(puzzle_term::SessionMessage::Keybindings(bindings)) {
        tracing::warn!(event = "web.bindings_failed", %error);
    }
}

/// A late resource result must still match this terminal's requested key.
pub(super) fn select_text(owner: &Owner) {
    // Snapshot the requested identity before yielding to another browser event.
    let (gpu, settings, dpr) = {
        let r = owner.borrow();
        // Initial creation supplies the first context; there is nothing to replace beforehand.
        let Some(text) = r.text.as_ref() else {
            return;
        };
        (text.gpu.clone(), r.settings.clone(), r.dpr)
    };
    // Each policy/scale combination owns independent size-specific raster shelves.
    let key = text_key(&settings, dpr);
    // DPR-specific hinted outlines cannot be installed under the previous monitor's key.
    // A pending fetch cannot pin a disposed terminal's DOM or Session indefinitely.
    let weak = Rc::downgrade(owner);
    wasm_bindgen_futures::spawn_local(async move {
        let result = resources::text(
            gpu,
            key.clone(),
            settings.font_size,
            dpr as f32,
            // Family policy is part of the text key and survives pack-first startup.
            settings.font_fallback.clone(),
            settings.render_glyph_memory_mb,
        )
        .await;
        let Some(owner) = weak.upgrade().filter(|o| !o.borrow().disposed) else {
            return;
        };
        // Preserve the previously drawable context if validation or network retrieval fails.
        let text = match result {
            Ok(text) => text,
            Err(error) => {
                events::emit(&owner, "error", error);
                return;
            }
        };
        {
            let mut r = owner.borrow_mut();
            // A later zoom or monitor change supersedes this result without replacing its owner.
            if key != text_key(&r.settings, r.dpr) {
                return;
            }
            // Pending old-key ink belongs to its old shared GPU owner, not to
            // the smaller replacement budget. Transfer it before switching.
            if let Err(error) = r.retire_text_uploads() {
                tracing::warn!(event = "web.text_replacement_failed", %error);
                return;
            }
            // Fonts stay shared; the host invalidates caches and passes measured geometry.
            if let Some(context) = text.context.borrow().as_ref()
                && let Err(error) = r.set_text_context(context)
            {
                tracing::warn!(event = "web.metrics_failed", %error);
                return;
            }
            // Completion follows only owners which still use this text resource.
            text.follow(&owner);
            r.text = Some(text.clone());
        }
        // New metrics change cell counts and reflow at the next paint, not inside this callback.
        let _ = draw::fit(&owner);
        super::create::load_fonts(&owner, text);
    });
}
