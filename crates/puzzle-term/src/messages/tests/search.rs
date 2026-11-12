//! Query option validation and stable search bus names.
use super::*;
/// Literal options and UI requests have exact wire shapes; regex remains deferred.
#[test]
fn search_wire_roundtrips_and_rejects_unknown_options() {
    use crate::{search::SearchCommand, vt::search::FindOptions};
    let registry = ExtensionRegistry::default();
    let mut requests = vec![SearchCommand::Clear, SearchCommand::Request];
    for case_sensitive in [None, Some(false), Some(true)] {
        requests.push(SearchCommand::Find {
            query: "Ab界".into(),
            options: FindOptions { case_sensitive },
        });
    }
    for request in requests {
        let value = super::super::search_wire::to_value(&request);
        let parsed = Command::from_value("terminal/search", &value, &registry).expect("search");
        assert_eq!(parsed.to_value().expect("wire"), value);
    }
    let value = Event::SearchRequested.to_value().expect("request");
    assert_eq!(
        Event::from_value("terminal/searchRequested", &value, &registry)
            .expect("request")
            .to_value()
            .expect("wire"),
        value
    );
    for (key, invalid) in [
        ("regex", Value::Bool(true)),
        ("caseSensitive", Value::Number(1.0)),
        ("query", Value::Null),
    ] {
        let mut value = super::super::search_wire::to_value(&SearchCommand::Find {
            query: "x".into(),
            options: FindOptions::default(),
        });
        if let Value::Map(map) = &mut value {
            map.insert(key.into(), invalid);
        }
        assert!(Command::from_value("terminal/search", &value, &registry).is_err());
    }
}

/// One representative activates Search in complete command-catalogue coverage.
pub(super) fn command_values() -> Vec<(&'static str, Value)> {
    vec![(
        "terminal/search",
        super::super::search_wire::to_value(&crate::search::SearchCommand::Clear),
    )]
}
