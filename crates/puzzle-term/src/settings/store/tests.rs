//! External patches exercise precedence and transaction boundaries (§14.2).
use super::{Layer, SettingsStore};
use puzzle_bus::Value;
use std::collections::BTreeMap;

/// Creates the neutral shape a JSON adapter would hand to the store.
fn object(items: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    Value::Map(
        items
            .into_iter()
            .map(|(key, value)| (key.into(), value))
            .collect(),
    )
}

#[test]
/// Session values outrank profiles and users regardless of write order.
fn layers_resolve_in_declared_order() {
    let mut store = SettingsStore::default();
    store.apply_key("font.size", Value::Number(16.0), Layer::Session);
    store.apply_key("font.size", Value::Number(12.0), Layer::User);
    store.apply_key("font.size", Value::Number(18.0), Layer::Profile);
    assert_eq!(store.effective().font_size, 16.0);
    store.reset("font.size", Layer::Session);
    assert_eq!(store.effective().font_size, 18.0);
    store.reset("font.size", Layer::Profile);
    assert_eq!(store.effective().font_size, 12.0);
}

#[test]
/// A numeric bound reports a clamp; invalid types retain prior state.
fn clamping_and_rejection_are_per_key() {
    let mut store = SettingsStore::default();
    let clamped = store.apply_key("font.size", Value::Number(900.0), Layer::User);
    assert_eq!(clamped.clamped, ["font.size"]);
    assert_eq!(store.effective().font_size, 200.0);
    let rejected = store.apply_key("font.size", Value::String("bad".into()), Layer::User);
    assert_eq!(rejected.rejected.len(), 1);
    assert_eq!(store.effective().font_size, 200.0);
}

#[test]
/// An older build exports unfamiliar nested keys without dropping them.
fn unknown_user_keys_survive_round_trip() {
    let future = object([("motion", object([("spring", Value::Bool(true))]))]);
    let mut store = SettingsStore::default();
    let report = store.apply_patch(&future, Layer::User);
    assert_eq!(report.unknown, ["motion.spring"]);
    assert_eq!(store.export_user(), future);
}

#[test]
/// Profiles validate before replacing the active profile's settings.
fn profile_activation_is_transactional() {
    let mut presentation = BTreeMap::new();
    presentation.insert("font".into(), object([("size", Value::Number(22.0))]));
    let profiles = object([("presentation", Value::Map(presentation))]);
    let mut store = SettingsStore::default();
    store.apply_key("profiles", profiles, Layer::User);
    let report = store.activate_profile("presentation");
    assert!(report.rejected.is_empty());
    assert_eq!(store.effective().font_size, 22.0);
    let bad = store.activate_profile("missing");
    assert_eq!(bad.rejected.len(), 1);
    assert_eq!(store.effective().font_size, 22.0);
}

#[test]
/// A bad replacement document cannot displace a working user layer.
fn broken_reload_keeps_last_good() {
    let mut store = SettingsStore::default();
    let good = object([("font", object([("size", Value::Number(18.0))]))]);
    assert!(store.replace_user(&good).rejected.is_empty());
    let bad = object([("font", object([("size", Value::String("huge".into()))]))]);
    assert_eq!(store.replace_user(&bad).rejected.len(), 1);
    assert_eq!(store.effective().font_size, 18.0);
}

#[test]
/// Profile reload must recompute its overrides from the replacement document.
fn active_profile_reloads_transactionally() {
    let mut store = SettingsStore::default();
    let first = object([(
        "profiles",
        object([(
            "demo",
            object([("font", object([("size", Value::Number(18.0))]))]),
        )]),
    )]);
    assert!(store.replace_user(&first).rejected.is_empty());
    assert!(store.activate_profile("demo").rejected.is_empty());
    let second = object([(
        "profiles",
        object([(
            "demo",
            object([("font", object([("size", Value::Number(21.0))]))]),
        )]),
    )]);
    assert!(store.replace_user(&second).rejected.is_empty());
    assert_eq!(store.effective().font_size, 21.0);
    assert!(store.replace_user(&object([])).rejected.len() == 1);
    assert_eq!(store.effective().font_size, 21.0);
}

#[test]
/// Trigger precedence changes one binding without erasing its siblings.
fn bindings_merge_by_trigger() {
    let mut store = SettingsStore::default();
    store.apply_key(
        "keyboard.bindings",
        Value::List(vec![
            Value::String("super+v=paste".into()),
            Value::String("super+c=copy".into()),
        ]),
        Layer::User,
    );
    store.apply_key(
        "keyboard.bindings",
        Value::List(vec![Value::String("super+v=sendText:x".into())]),
        Layer::Profile,
    );
    assert_eq!(
        store.effective().keyboard_bindings,
        ["super+v=sendText:x", "super+c=copy"]
    );
}
