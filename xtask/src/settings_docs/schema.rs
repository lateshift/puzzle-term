//! Draft 2020-12 config schema follows the typed settings metadata (§14.8).
//!
//! Dotted keys become nested JSON objects, matching the native config
//! parser. The root and each group accept unknown properties: the runtime
//! preserves newer settings across an older executable's read/write cycle.
//! Numeric ranges document editor controls; the runtime clamps a value
//! outside them and reports it rather than silently changing the default.
//! Every leaf keeps its typed default beside the validator. Editors can
//! offer completion for the current build while the runtime still accepts
//! unknown future siblings and preserves them when saving config.json.
//! The generator does not turn dotted names into literal JSON keys:
//! `cursor.blink` is `cursor` then `blink` in the config document.
//! Choice enums are copied from the same validator used by live patches,
//! preventing the editor from offering a string the runtime would reject.
use super::to_json;
use puzzle_term::settings::{SettingDef, Validator};
use serde_json::{Value as Json, json};

/// Returns the checked-in editor schema for every typed setting.
pub(super) fn document(definitions: &[SettingDef]) -> Json {
    let mut root = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "Puzzle Terminal settings",
        "type": "object",
        "properties": {},
        "additionalProperties": true
    });
    for definition in definitions {
        let mut property = property(definition);
        property["description"] = Json::String(definition.doc.into());
        property["default"] = to_json(&definition.default);
        // The path comes from `define_settings!`, never from a manually
        // mirrored JSON key catalogue in this generator.
        insert(&mut root, definition.key, property);
    }
    root
}

/// Maps a Rust value codec and validator to editor-facing JSON rules.
fn property(definition: &SettingDef) -> Json {
    let mut value = match definition.kind {
        "boolean" => json!({"type": "boolean"}),
        "number" => json!({"type": "number"}),
        "integer" => json!({"type": "integer"}),
        "string[]" => json!({"type": "array", "items": {"type": "string"}}),
        "object" => json!({"type": "object", "additionalProperties": true}),
        // Cursor blink accepts either the application-driven default or
        // a forced Boolean; describing only a string would reject `false`.
        "auto|boolean" => json!({"anyOf": [{"const": "auto"}, {"type": "boolean"}]}),
        // Capture defaults permit program overrides; the string forms fix ownership.
        "boolean|always|never" => {
            json!({"anyOf": [{"type": "boolean"}, {"enum": ["always", "never"]}]})
        }
        _ => json!({"type": "string"}),
    };
    match definition.validator {
        Validator::Range { minimum, maximum } => {
            value["minimum"] = json!(minimum);
            value["maximum"] = json!(maximum);
        }
        // Choice values are emitted from the validator, not copied from
        // a separately maintained settings documentation table.
        Validator::Choice(options) => value["enum"] = json!(options),
        Validator::Any | Validator::CursorColor => {}
    }
    value
}

/// Creates group objects only at path components before a schema leaf.
fn insert(root: &mut Json, key: &str, property: Json) {
    let mut current = root;
    let mut parts = key.split('.').peekable();
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            current["properties"][part] = property;
            return;
        }
        if current["properties"][part].is_null() {
            // Multiple leaves in `cursor` or `font` share one parent;
            // unknown siblings survive config round trips (§14.4).
            current["properties"][part] =
                json!({"type": "object", "properties": {}, "additionalProperties": true});
        }
        current = &mut current["properties"][part];
    }
}
