//! Callback payload bytes stay exact, borrowed and immutable across translation.
#![allow(
    clippy::unwrap_used,
    reason = "test asserts the documented binary callback conversion succeeds"
)]
use super::*;
use std::sync::Arc;

#[test]
fn binary_optional_effect_preserves_every_byte_and_the_original_allocation() {
    let bytes = Arc::new(
        [
            b"PZAD\x01\0\0\0".as_slice(),
            "exact UTF-8 é一\n".as_bytes(),
            &[0, 255],
        ]
        .concat(),
    );
    let original = bytes.as_ptr();
    let event = host(HostEvent::OscEffect(puzzle_term::osc::OscHostEffect {
        kind: "ext:example/dragSource",
        data: bytes.clone(),
    }))
    .unwrap();
    assert_eq!(event.0, "ext:example/dragSource");
    assert_eq!(&*event.1, bytes.as_slice());
    assert_eq!(
        event.1.as_ptr(),
        original,
        "callback translation copied the export"
    );
    // Dropping the owner's reference cannot invalidate the call-scoped payload.
    drop(bytes);
    assert_eq!(&event.1[..4], b"PZAD");
}

/// A generic extension reaches the C JSON callback without adding an ABI function.
struct Meaning(puzzle_term::bus::Value);
impl puzzle_term::bus::wire::WireExt for Meaning {
    const TYPE_NAME: &'static str = "ext:example/semanticChanged";
    fn to_value(&self) -> puzzle_term::bus::Value {
        self.0.clone()
    }
    fn from_value(
        value: &puzzle_term::bus::Value,
    ) -> Result<Self, puzzle_term::bus::value::ValidationReport> {
        Ok(Self(value.clone()))
    }
}

#[test]
fn semantic_facts_preserve_provenance_and_exact_counter_strings_in_c_json() {
    use puzzle_term::bus::{Value, wire::Extension};
    let row = Value::Map(
        [
            ("id".into(), Value::String("reply".into())),
            ("revision".into(), Value::String(u64::MAX.to_string())),
            ("producer".into(), Value::String("22".repeat(16))),
            ("generation".into(), Value::String("1".into())),
            ("kind".into(), Value::String("region".into())),
            ("detail".into(), Value::String("reply".into())),
            ("attachment".into(), Value::String("attached".into())),
            ("selected".into(), Value::Bool(true)),
            ("folded".into(), Value::Bool(true)),
            ("foldedLines".into(), Value::String("12".into())),
        ]
        .into(),
    );
    let fact = Value::Map(
        [
            ("session".into(), Value::String("11".repeat(16))),
            ("epoch".into(), Value::String("1".into())),
            ("reason".into(), Value::String("fold".into())),
            ("records".into(), Value::List(vec![row])),
        ]
        .into(),
    );
    // This is the same owned Fact path that puzzle_app_tick forwards to C callbacks.
    let event = convert(AppEvent::Host(HostEvent::Fact(
        puzzle_term::messages::Event::Ext(Extension::new(Meaning(fact.clone())).unwrap()),
    )))
    .unwrap();
    assert_eq!(event.0, "ext:example/semanticChanged");
    let actual: Json = serde_json::from_slice(&event.1).unwrap();
    assert_eq!(actual, json_value(fact));
    assert_eq!(actual["records"][0]["revision"], u64::MAX.to_string());
    assert!(actual.get("key").is_none());
}
