//! Custom theme registration and cycle-safe inheritance (§14.6).
//!
//! A named custom theme is a neutral Value map from settings, never a
//! platform JSON object. Its terminal colors override an inherited built-in
//! or custom parent. All missing UI tokens derive from the final terminal
//! palette, keeping them coherent when a user changes only background.
//!
//! Registration and resolution are separate transactions. A theme may
//! refer to a parent that is registered later, so accepting a structurally
//! valid definition cannot require that its parent already exists.
//! Resolution reports an unknown parent or a cycle when the theme is used.
//! This also lets a user repair one definition without deleting the rest.
//!
//! The built-in catalogue is immutable. A custom name cannot replace it:
//! otherwise a config file could silently change the colors of an app's
//! explicit `theme.dark = "puzzle-dark"` after a reload. Custom maps carry
//! only data from the bounded bus Value tree, with no host file handles.
//!
//! The terminal palette is an all-or-nothing list of 16 colors. Partial
//! lists would make ANSI indices change meaning depending on inheritance
//! depth, and could conflict with OSC 4 overrides on existing cells.
//! UI overlays are derived after the terminal colors are resolved; their
//! source is Puzzle's OKLab table, not the upstream palette project.
/// UI sources are applied after derivation, including explicitly inherited tokens.
mod ui;
use super::{Appearance, CustomMap, Theme, color};
use puzzle_bus::Value;
use std::collections::BTreeMap;

/// Built-in names plus validated custom Value maps.
#[derive(Default)]
pub struct ThemeRegistry {
    /// User definitions indexed by stable selector name.
    custom: BTreeMap<String, CustomMap>,
}

impl ThemeRegistry {
    /// Inserts a custom theme only after its plain-data shape is valid.
    pub fn register(&mut self, name: &str, value: &Value) -> Result<(), String> {
        // A built-in selector is stable on the wire and in settings. The
        // length bound also prevents a huge user key from entering every
        // frame's diagnostic and gallery label.
        if name.is_empty() || name.len() > 128 || Theme::builtin(name).is_some() {
            return Err(format!("invalid or reserved theme name: {name}"));
        }
        puzzle_bus::value::validate_tree(value)
            .map_err(|report| format!("theme {name} is not plain data: {report:?}"))?;
        // A map root gives token names stable meaning. A list or scalar
        // could not express `extends` alongside terminal color overrides.
        let Value::Map(map) = value else {
            return Err(format!("theme {name} must be an object"));
        };
        // Validate all supplied colors before changing the active registry;
        // an invalid edit must leave the previous theme usable.
        validate(map)?;
        // Keep the old entry in place until every supplied field passes.
        // A live config reload may arrive while the current theme is drawn.
        self.custom.insert(name.into(), map.clone());
        Ok(())
    }

    /// Finds a built-in or resolves a custom chain with cycle reporting.
    pub fn resolve(&self, name: &str) -> Result<Theme, String> {
        // The recursion stack belongs to one lookup. It must not be shared
        // across windows because two sessions can resolve different pairs.
        self.resolve_inner(name, &mut Vec::new())
    }

    /// Lists built-ins and custom selectors without duplicating one name.
    #[must_use]
    pub fn list(&self) -> Vec<String> {
        // Generated built-ins retain source order for deterministic docs;
        // BTreeMap keeps the user-defined tail stable across JSON parsers.
        let mut names: Vec<String> = Theme::builtin_names()
            .into_iter()
            .map(str::to_owned)
            .collect();
        names.extend(self.custom.keys().cloned());
        names
    }

    /// Resolves inheritance before applying child terminal overrides.
    fn resolve_inner(&self, name: &str, chain: &mut Vec<String>) -> Result<Theme, String> {
        // Detect the repeated selector before descending again. The path
        // names the exact loop, which is more useful than a depth error.
        if chain.iter().any(|item| item == name) {
            chain.push(name.into());
            return Err(format!("theme inheritance cycle: {}", chain.join(" -> ")));
        }
        if chain.len() >= 64 {
            // The Value tree is already depth-bounded; this separate limit
            // bounds a long chain of individually shallow custom themes.
            return Err("theme inheritance exceeds 64 ancestors".into());
        }
        // Generated built-ins terminate a chain and never allocate a JSON
        // parser or inspect a file in the platform-neutral runtime.
        if let Some(builtin) = Theme::builtin(name) {
            return Ok(builtin);
        }
        let map = self
            .custom
            .get(name)
            .ok_or_else(|| format!("unknown theme: {name}"))?;
        // A missing `extends` takes the dark Puzzle base so sparse custom
        // themes still have a complete ANSI palette and default cursor.
        chain.push(name.into());
        let parent = match map.get("extends") {
            Some(Value::String(name)) => name.as_str(),
            _ => "puzzle-dark",
        };
        let mut theme = self.resolve_inner(parent, chain)?;
        chain.pop();
        // The child changes only supplied terminal tokens. The parent can
        // provide all 16 palette slots and the unmodified source credit.
        apply(&mut theme, map)?;
        theme.name = name.into();
        // Recompute missing overlays from the final terminal colors; a
        // parent built-in's derived tints are not source palette values.
        theme.rederive();
        // Missing UI colors derive from final terminal colors; authored ancestors survive.
        self.apply_ui_chain(&mut theme, name)?;
        // A custom low-contrast foreground is allowed, unlike a Puzzle
        // built-in, but the author receives a diagnostic for readability.
        let ratio = color::contrast(theme.foreground, theme.background);
        tracing::debug!(theme = name, contrast = ratio, "custom theme contrast");
        if ratio < 3.0 {
            tracing::warn!(
                theme = name,
                contrast = ratio,
                "custom theme foreground contrast below 3:1"
            );
        }
        Ok(theme)
    }
    /// Replays validated authored ancestors after final derivation, bounded by resolve_inner.
    fn apply_ui_chain(&self, theme: &mut Theme, name: &str) -> Result<(), String> {
        // This is reached only after cycle/depth validation has completed successfully.
        let Some(map) = self.custom.get(name) else {
            return Ok(());
        };
        if let Some(Value::String(parent)) = map.get("extends") {
            self.apply_ui_chain(theme, parent)?;
        }
        ui::apply(theme, map)
    }
}

/// Parses a supplied literal color without altering the source bytes.
fn rgb(value: &Value, key: &str) -> Result<[u8; 3], String> {
    // CSS-like hex is the one portable input form shared by browser JS,
    // desktop JSON and scenario commands. A malformed value is reported
    // before it can be stored as an active rendering color.
    let Value::String(text) = value else {
        return Err(format!("{key} must be a #rrggbb color"));
    };
    let digits = text
        .strip_prefix('#')
        .filter(|digits| digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| format!("{key} must be #rrggbb"))?;
    let mut result = [0; 3];
    // Parse each byte pair separately: slicing only after the six-digit
    // length check avoids panics from externally supplied short strings.
    for (index, channel) in result.iter_mut().enumerate() {
        let start = index * 2;
        *channel = u8::from_str_radix(&digits[start..start + 2], 16)
            .map_err(|error| format!("{key}: {error}"))?;
    }
    Ok(result)
}

/// Checks known terminal tokens before registration becomes visible.
fn validate(map: &CustomMap) -> Result<(), String> {
    // Parsing against a temporary theme validates all supplied UI surfaces as well.
    ui::apply(
        &mut Theme::builtin("puzzle-dark").ok_or("default theme absent")?,
        map,
    )?;
    // Token validation is independent of inheritance. A child can be
    // registered before its parent while still rejecting a broken color.
    for key in [
        "foreground",
        "background",
        "cursor",
        "cursorText",
        "selectionBackground",
        "selectionForeground",
        "searchMatch",
        "searchCurrent",
    ] {
        if let Some(value) = map.get(key) {
            let _ = rgb(value, key)?;
        }
    }
    if let Some(Value::List(colors)) = map.get("palette") {
        // ANSI slots are indexed by terminal bytes already held in cells.
        // Requiring all 16 prevents a partial child palette from shifting
        // the meaning of any retained palette-tagged cell.
        if colors.len() != 16 {
            return Err("custom palette must have 16 colors".into());
        }
        for value in colors {
            let _ = rgb(value, "palette")?;
        }
    } else if map.contains_key("palette") {
        // A string palette might pass generic Value validation, but it
        // cannot map sixteen distinct ANSI indices during frame building.
        return Err("custom palette must be a list".into());
    }
    // Metadata is kept in the same neutral tree as colors. Requiring text
    // here avoids lossier formatting when the gallery or docs read it.
    for key in ["extends", "displayName", "appearance", "credit"] {
        if let Some(value) = map.get(key)
            && !matches!(value, Value::String(_))
        {
            return Err(format!("{key} must be a string"));
        }
    }
    if let Some(Value::String(appearance)) = map.get("appearance")
        && !matches!(appearance.as_str(), "light" | "dark")
    {
        // A misspelled appearance would choose the wrong COLORFGBG and
        // mode-2031 response even if its colors happened to look correct.
        return Err("appearance must be light or dark".into());
    }
    Ok(())
}

/// Overlays the supplied terminal tokens on an inherited theme.
fn apply(theme: &mut Theme, map: &CustomMap) -> Result<(), String> {
    // The targets are semantic fields, not borrowed slices into a compact
    // palette: OSC 10/11/12 can override each default independently.
    for (key, target) in [
        ("foreground", &mut theme.foreground),
        ("background", &mut theme.background),
        ("cursor", &mut theme.cursor),
        ("cursorText", &mut theme.cursor_text),
        ("selectionBackground", &mut theme.selection_background),
    ] {
        if let Some(value) = map.get(key) {
            *target = rgb(value, key)?;
        }
    }
    if let Some(value) = map.get("selectionForeground") {
        // Absence preserves cell ink; an authored token overrides it.
        theme.selection_foreground = Some(rgb(value, "selectionForeground")?);
    }
    if let Some(Value::List(colors)) = map.get("palette") {
        // `validate` already checked the count, and resolution checks again
        // through `rgb` so a future registration path cannot bypass it.
        for (index, value) in colors.iter().enumerate() {
            theme.palette[index] = rgb(value, "palette")?;
        }
    }
    if let Some(Value::String(value)) = map.get("displayName") {
        // A child keeps its parent's friendly label unless it supplies one;
        // its stable selector name is replaced separately by the caller.
        theme.display_name = value.clone();
    }
    if let Some(Value::String(value)) = map.get("credit") {
        theme.credit = value.clone();
    }
    if let Some(Value::String(value)) = map.get("appearance") {
        // Appearance is explicit metadata; lightness cannot be inferred
        // reliably from custom backgrounds or transparent overlays.
        theme.appearance = if value == "light" {
            Appearance::Light
        } else {
            Appearance::Dark
        };
    }
    Ok(())
}
