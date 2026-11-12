//! Independent recorded lifecycle payloads for the complete wire catalogue.
use puzzle_bus::Value;
use std::collections::BTreeMap;

/// Independent host recording of one active remote context (not parser output).
pub(super) fn context_fixture() -> Value {
    Value::Map(BTreeMap::from([(
        "contexts".into(),
        Value::List(vec![Value::Map(BTreeMap::from([
            ("id".into(), Value::String("remote".into())),
            (
                "fields".into(),
                Value::Map(BTreeMap::from([(
                    "type".into(),
                    Value::String("remote".into()),
                )])),
            ),
        ]))]),
    )]))
}

/// Independent recorded lifecycle payload for the public exit fact.
pub(super) fn exit_fixture() -> Value {
    Value::Map(BTreeMap::from([
        ("code".into(), Value::Number(3.0)),
        ("signal".into(), Value::Null),
        ("runtimeMs".into(), Value::Number(200.0)),
    ]))
}
