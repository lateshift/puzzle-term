//! Public fact fixtures stay independent from command routing and policy tests.
use super::*;

/// Representative payloads for every active event, separate from validation.
pub(super) fn values() -> Vec<(&'static str, Value)> {
    vec![
        ("render/frameRendered", frame_stats::fixture()),
        ("terminal/titleChanged", Value::String("shell".into())),
        ("terminal/iconNameChanged", Value::String("icon".into())),
        ("terminal/cwdChanged", Value::String("file:///tmp".into())),
        ("terminal/contextsChanged", lifecycle::context_fixture()),
        ("viewport/scrolled", Value::Number(20.0)),
        ("viewport/newOutputChanged", Value::Bool(true)),
        (
            "terminal/selectionChanged",
            super::super::selection_wire::range_value(None),
        ),
        ("terminal/searchRequested", Value::Null),
        ("terminal/bell", Value::Null),
        ("shell/promptStarted", Value::Null),
        ("shell/inputStarted", Value::Null),
        (
            "shell/commandStarted",
            super::super::shell_wire::started(Some("cargo build")),
        ),
        (
            "shell/commandFinished",
            Value::Map(BTreeMap::from([
                ("exit".into(), Value::Number(7.0)),
                ("duration".into(), Value::Null),
            ])),
        ),
        (
            "terminal/gridResized",
            Value::Map(BTreeMap::from([
                ("columns".into(), Value::Number(80.0)),
                ("rows".into(), Value::Number(24.0)),
            ])),
        ),
        (
            "terminal/progress",
            Value::Map(BTreeMap::from([
                ("state".into(), Value::Number(2.0)),
                ("percent".into(), Value::Number(50.0)),
            ])),
        ),
        (
            "settings/themeChanged",
            Value::Map(BTreeMap::from([
                ("name".into(), Value::String("puzzle-light".into())),
                ("appearance".into(), Value::String("light".into())),
            ])),
        ),
        ("session/exited", lifecycle::exit_fixture()),
        ("session/closeRequested", Value::Null),
    ]
}

/// Old unit start facts remain replayable after adding optional command labels.
#[test]
fn legacy_command_start_is_a_missing_label() {
    let registry = ExtensionRegistry::default();
    let event =
        Event::from_value("shell/commandStarted", &Value::Null, &registry).expect("old fact");
    assert!(matches!(event, Event::CommandStarted(None)));
    // External fact recordings must not bypass the host-label safety cap.
    for command in ["x".repeat(1025), "line\ncontrol".into()] {
        let payload = Value::Map(BTreeMap::from([("command".into(), Value::String(command))]));
        assert!(Event::from_value("shell/commandStarted", &payload, &registry).is_err());
    }
}
