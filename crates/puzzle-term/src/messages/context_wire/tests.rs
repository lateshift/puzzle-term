//! Context event replay is exact, bounded and lossless for u64 identities.
#![allow(
    clippy::expect_used,
    reason = "tests require successful wire construction and replay"
)]
use super::*;
use crate::messages::Event;
use puzzle_bus::wire::{ExtensionRegistry, FromWire, ToWire};
#[test]
fn context_event_roundtrip_and_rejection() {
    let mut term = puzzle_vt::Terminal::new(20, 4, 0, 0);
    term.advance(
        b"\x1b]3008;start=a\\x3bb;pid=18446744073709551615;type=remote;targethost=host\x1b\\",
    );
    let event = Event::ContextsChanged(term.contexts().to_vec());
    let wire = event.to_value().expect("context wire");
    let restored = Event::from_value(
        "terminal/contextsChanged",
        &wire,
        &ExtensionRegistry::default(),
    )
    .expect("context replay");
    assert_eq!(restored.to_value().expect("replayed wire"), wire);
    assert_eq!(decode(&wire).expect("valid path"), term.contexts());
    assert!(decode(&Value::List(vec![wire.clone()])).is_err());
    if let Value::Map(mut map) = wire {
        if let Some(Value::List(list)) = map.get_mut("contexts") {
            list.push(list[0].clone());
        }
        assert!(decode(&Value::Map(map)).is_err());
    }
    for (key, value) in [
        ("pid", "18446744073709551616"),
        ("type", "bogus"),
        ("user", "bad\0name"),
        ("unknown", "x"),
    ] {
        let bad = Value::List(vec![Value::Map(BTreeMap::from([
            ("id".into(), Value::String("a".into())),
            (
                "fields".into(),
                Value::Map(BTreeMap::from([(key.into(), Value::String(value.into()))])),
            ),
        ]))]);
        assert!(decode(&Value::Map(BTreeMap::from([("contexts".into(), bad)]))).is_err());
    }
    assert_eq!(
        decode(&Value::Map(BTreeMap::from([(
            "contexts".into(),
            Value::List(vec![])
        )])))
        .expect("empty path"),
        []
    );
}
