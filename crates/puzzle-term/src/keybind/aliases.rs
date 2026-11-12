//! Explicit compatibility aliases for alternate-spelling configuration (§14.9).
//!
//! Unsupported actions keep their names and are rejected per entry. Tabs
//! and splits belong to a host, so no alias pretends the engine has them.
//! Alias translation precedes argument validation; a misspelled payload
//! therefore receives the same report as its native camelCase equivalent.
//! Decreasing font size negates the step instead of adding another action.
//! Only the first colon splits a name from its payload, preserving CSI
//! subparameters and literal text that itself contains colons.

/// One alias and the portable replacement for its argument-free name.
pub const ALIASES: &[(&str, &str)] = &[
    // Clipboard aliases deliberately preserve selection availability.
    ("copy_to_clipboard", "copy"),
    ("paste_from_clipboard", "paste"),
    // Font actions retain signed steps rather than a second zoom mechanism.
    ("increase_font_size", "zoom"),
    ("reset_font_size", "zoomReset"),
    // screen actions have the same prompt-preserving semantics as the reference.
    ("clear_screen", "clearScreen"),
    ("select_all", "selectAll"),
    // Fractional scrolling remains typed even before M8 implements it.
    ("scroll_page_fractional", "scrollPages"),
    ("scroll_page_lines", "scrollLines"),
    ("scroll_to_top", "scrollTop"),
    ("scroll_to_bottom", "scrollBottom"),
    ("jump_to_prompt", "jumpToPrompt"),
    // Input aliases use our one escape decoder, including CSI and ESC.
    ("text", "sendText"),
    ("csi", "csi"),
    ("esc", "esc"),
    // Desktop actions are listed even when their feature is deferred.
    ("reload_config", "reloadSettings"),
    ("open_config", "openSettings"),
    ("toggle_fullscreen", "toggleFullscreen"),
    ("new_window", "newWindow"),
    ("close_window", "closeWindow"),
    ("ignore", "ignore"),
    ("unbind", "unbind"),
];

/// Translates spelling only; the typed parser still validates the payload.
pub(super) fn translate(text: &str) -> String {
    let (name, arg) = text
        .split_once(':')
        .map_or((text, None), |(n, a)| (n, Some(a)));
    match name {
        // Directional aliases have implicit arguments in the reference terminal's grammar.
        "decrease_font_size" => format!("zoom:-{}", arg.unwrap_or("1")),
        "increase_font_size" => format!("zoom:{}", arg.unwrap_or("1")),
        "scroll_page_up" => "scrollPages:-1".into(),
        "scroll_page_down" => "scrollPages:1".into(),
        _ => {
            let mapped = ALIASES
                .iter()
                .find(|(old, _)| *old == name)
                .map_or(name, |(_, new)| *new);
            // Arguments may contain colons; split_once preserves their tail.
            arg.map_or_else(|| mapped.into(), |arg| format!("{mapped}:{arg}"))
        }
    }
}
