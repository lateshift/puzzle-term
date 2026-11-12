//! Selection wire shapes and exact-number rejection at the external seam.
use super::*;
/// Each selection operation and fact round-trips through its stable public lane.
#[test]
fn selection_wire_roundtrips_and_invalid_positions() {
    use crate::selection::{SelectionCommand, SelectionRange};
    use crate::vt::selection::{SelectPoint, SelectionKind};
    let registry = ExtensionRegistry::default();
    let start = SelectPoint {
        line: crate::vt::tracked::LineId(123),
        column: 4,
    };
    let end = SelectPoint {
        line: crate::vt::tracked::LineId(456),
        column: 7,
    };
    let mut requests = vec![
        SelectionCommand::Between { start, end },
        SelectionCommand::All,
        SelectionCommand::Clear,
        SelectionCommand::CommandOutput(None),
        SelectionCommand::CommandOutput(Some(start)),
    ];
    for kind in [
        SelectionKind::Simple,
        SelectionKind::Word,
        SelectionKind::Line,
        SelectionKind::Block,
        SelectionKind::Semantic,
    ] {
        requests.push(SelectionCommand::Range { kind, start, end });
    }
    for request in requests {
        let value = crate::messages::selection_wire::to_value(request);
        let parsed = Command::from_value("terminal/select", &value, &registry).expect("select");
        assert_eq!(parsed.to_value().expect("wire"), value);
    }
    for block in [false, true] {
        let event = Event::SelectionChanged(Some(SelectionRange { start, end, block }));
        let value = event.to_value().expect("wire");
        let parsed =
            Event::from_value("terminal/selectionChanged", &value, &registry).expect("fact");
        assert_eq!(parsed.to_value().expect("wire"), value);
    }
    // Malformed wire numbers must never narrow into a different valid selected cell.
    for number in [f64::NAN, f64::INFINITY, -1.0, 0.5, 9_007_199_254_740_992.0] {
        let mut value = crate::messages::selection_wire::to_value(SelectionCommand::Range {
            kind: SelectionKind::Simple,
            start,
            end,
        });
        if let Value::Map(map) = &mut value
            && let Some(Value::Map(point)) = map.get_mut("start")
        {
            point.insert("lineId".into(), Value::Number(number));
        }
        assert!(Command::from_value("terminal/select", &value, &registry).is_err());
    }
}

/// Signed pointer sample is shared by the full catalogue coverage witness.
pub(super) fn pointer_value() -> Value {
    super::super::pointer_wire::to_value(crate::pointer::PointerInput {
        event: super::mouse_event(),
        position: [-10.0, 20.0],
        cell: [8, 16],
    })
}

/// New M8 command payloads also participate in the complete catalogue coverage.
pub(super) fn command_values() -> Vec<(&'static str, Value)> {
    vec![
        (
            "input/copy",
            super::super::copy_wire::to_value(crate::selection::CopyTarget::Selection(
                crate::vt::view::Format::PlainText,
            )),
        ),
        ("input/pointer", pointer_value()),
        (
            "platform/clipboardWrite",
            super::super::copy_wire::html_value("<pre>copy</pre>", "copy"),
        ),
    ]
}

/// Every rich representation round-trips, and missing plain alternatives are rejected.
#[test]
fn copy_and_rich_clipboard_wire_contracts() {
    use crate::{selection::CopyTarget, vt::view::Format};
    let registry = ExtensionRegistry::default();
    for target in [
        CopyTarget::Selection(Format::PlainText),
        CopyTarget::Selection(Format::Vt),
        CopyTarget::Selection(Format::Html),
        CopyTarget::CommandOutput,
        CopyTarget::ZoneOutput(42),
        CopyTarget::ZoneOutput(u64::MAX),
    ] {
        let value = super::super::copy_wire::to_value(target);
        let command = Command::from_value("input/copy", &value, &registry).expect("copy");
        assert_eq!(command.to_value().expect("wire"), value);
    }
    let valid = super::super::copy_wire::html_value("<pre>plain</pre>", "plain");
    assert_eq!(
        Command::from_value("platform/clipboardWrite", &valid, &registry)
            .expect("rich")
            .to_value()
            .expect("wire"),
        valid
    );
    for key in ["html", "text"] {
        let mut missing = valid.clone();
        if let Value::Map(map) = &mut missing {
            map.remove(key);
        }
        assert!(Command::from_value("platform/clipboardWrite", &missing, &registry).is_err());
        let mut wrong = valid.clone();
        if let Value::Map(map) = &mut wrong {
            map.insert(key.into(), Value::Null);
        }
        assert!(Command::from_value("platform/clipboardWrite", &wrong, &registry).is_err());
    }
    let mut extra = valid;
    if let Value::Map(map) = &mut extra {
        map.insert("origin".into(), Value::String("user".into()));
    }
    assert!(Command::from_value("platform/clipboardWrite", &extra, &registry).is_err());
    let mut unsupported = super::super::copy_wire::to_value(CopyTarget::CommandOutput);
    if let Value::Map(map) = &mut unsupported {
        map.insert("format".into(), Value::String("html".into()));
    }
    assert!(Command::from_value("input/copy", &unsupported, &registry).is_err());
}

/// Signed outside distance is legal, but nonfinite coordinates and invalid metrics are not.
#[test]
fn pointer_wire_rejects_invalid_geometry() {
    let registry = ExtensionRegistry::default();
    let valid = pointer_value();
    assert_eq!(
        Command::from_value("input/pointer", &valid, &registry)
            .expect("pointer")
            .to_value()
            .expect("wire"),
        valid
    );
    for (key, number) in [
        ("x", f64::NAN),
        ("y", f64::INFINITY),
        ("cellWidth", 0.0),
        ("cellHeight", -1.0),
        ("cellWidth", 1.5),
        ("cellHeight", f64::from(u32::MAX) + 1.0),
    ] {
        let mut malformed = valid.clone();
        if let Value::Map(map) = &mut malformed {
            map.insert(key.into(), Value::Number(number));
        }
        assert!(Command::from_value("input/pointer", &malformed, &registry).is_err());
    }
}
