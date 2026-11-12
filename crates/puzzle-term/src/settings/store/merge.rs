//! Merge rules for the four settings layers (§14.2).
//!
//! Ordinary objects merge deeply and lists replace. Keybindings are the
//! one list exception: later layers replace entries with the same trigger.
//! User-document export reconstructs dotted keys without losing unknowns.
use puzzle_bus::Value;
use std::collections::BTreeMap;

/// Uses the parser's canonical identity, ignoring consumption prefixes.
pub(super) fn merge_bindings(
    base: &mut Value,
    override_value: &Value,
    platform: crate::keybind::Platform,
) {
    let (Value::List(existing), Value::List(changes)) = (base, override_value) else {
        return;
    };
    // Normalizing before replacement lets alias and modifier-order edits
    // update a binding instead of leaving an older duplicate active.
    // Prefixes are deliberately excluded: consumption is the replacement
    // value, not a way to create a second shortcut with the same chord.
    for change in changes {
        let Value::String(text) = change else {
            continue;
        };
        // A clear marker must remain in the merged list too: the later
        // matcher starts with platform defaults and must clear those as well.
        if text == "clear" {
            existing.clear();
            existing.push(change.clone());
            continue;
        }
        let Ok(crate::keybind::Entry::Binding(binding)) =
            crate::keybind::Entry::parse(text, platform, crate::keybind::Source::User)
        else {
            continue;
        };
        let canonical = binding.canonical();
        let position = existing.iter().position(|entry| {
            let Value::String(old) = entry else { return false; };
            matches!(crate::keybind::Entry::parse(old, platform, crate::keybind::Source::User), Ok(crate::keybind::Entry::Binding(old)) if old.canonical() == canonical)
        });
        // Keep unbind tombstones until matching, so they also remove a
        // default introduced by a different platform's initial table.
        if let Some(position) = position {
            // Replacing in place keeps unrelated introspection entries stable.
            existing[position] = change.clone();
        } else {
            existing.push(change.clone());
        }
    }
}

/// Deep-merges maps while replacing scalars and ordinary lists as units.
pub(super) fn merge_value(base: &mut Value, override_value: Value) {
    match (base, override_value) {
        (Value::Map(existing), Value::Map(changes)) => {
            // A profile can replace one nested token without repeating
            // every theme color inherited from the user layer.
            for (key, value) in changes {
                if let Some(old) = existing.get_mut(&key) {
                    merge_value(old, value);
                } else {
                    existing.insert(key, value);
                }
            }
        }
        // Scalar and ordinary list values are indivisible settings: joining
        // their elements would produce a value no layer actually chose.
        (base, replacement) => *base = replacement,
    }
}

/// Recreates a dotted JSON path without discarding unknown sibling fields.
pub(super) fn insert_nested(root: &mut BTreeMap<String, Value>, key: &str, value: Value) {
    let Some((head, tail)) = key.split_once('.') else {
        // The last component keeps the exact unknown value for a newer
        // version of Puzzle to recognize after an older-version save.
        root.insert(key.into(), value);
        return;
    };
    let child = root
        .entry(head.into())
        .or_insert_with(|| Value::Map(BTreeMap::new()));
    if let Value::Map(map) = child {
        // A parent created by this function is a map. A conflicting user
        // scalar remains intact rather than being silently overwritten.
        insert_nested(map, tail, value);
    }
}
