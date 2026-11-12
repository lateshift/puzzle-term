//! Shortcut reference generated from the same portable data as both shells.
//!
//! Source spelling is shown alongside canonical matching in CLI output.
//! Deferred actions remain documented so users can keep portable configs.
use puzzle_term::keybind::{ACTIONS, Platform, defaults};

/// Lists every default and action without maintaining a second catalogue.
pub(super) fn document() -> String {
    // Guide rows retain source syntax so users can paste a default into
    // config.json; CLI introspection separately shows canonical identities.
    let mut output = String::from("| Platform | Trigger | Action |\n|---|---|---|\n");
    // Each OS table is tested on all hosts; this generator uses no target cfg.
    for (label, platform) in [
        ("macOS", Platform::Mac),
        ("Windows", Platform::Windows),
        ("Linux", Platform::Linux),
    ] {
        for line in defaults::lines(platform) {
            if let Some((trigger, action)) = line.split_once('=') {
                output.push_str(&format!("| {label} | `{trigger}` | `{action}` |\n"));
            }
        }
    }
    output.push_str("\nOn macOS, Cmd+C is performable copy (selection arrives in M8). Readline's Alt+B/F/D shortcuts need Option as Alt: `keyboard.optionAsAlt = \"left\"` keeps right Option for layout characters. On Windows, Ctrl+C without a selection reaches the application as `0x03`. The web uses the user's OS table and reports browser-reserved shortcuts.\n\nFor `keyboard.optionAsAlt` in Chromium, `navigator.keyboard.getLayoutMap()` supplies the exact unmodified character. Safari and Firefox lack that API: on non-US layouts, Option-as-Alt follows physical US key positions (for example German Z/Y). Chords are always encoded; no map is learned from earlier typing. The default `none` uses native layout text and needs no map in any browser.\n\n## Action catalogue\n\n| Action | Argument | Availability | Description |\n|---|---|---|---|\n");
    // Availability describes functionality, not grammar: future actions still
    // parse, and performable bindings preserve their encoder fallback.
    for action in ACTIONS {
        output.push_str(&format!(
            "| `{}` | {} | {} | {} |\n",
            action.name, action.argument, action.available, action.description
        ));
    }
    output
}
