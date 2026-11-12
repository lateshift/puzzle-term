//! Wire and origin rules for the concrete terminal message domain (§5.7).
// Test assertions identify the fixture whose protocol contract changed.
#![allow(
    clippy::expect_used,
    reason = "test assertions name protocol fixture failures"
)]
use super::{Command, Event, catalogue};
use proptest::prelude::*;
use puzzle_bus::{
    bus::Bus,
    hooks::{CommandKinds, Flow, HookCtx, Interceptor},
    kinds::{Availability, KindDef, Visibility},
    message::{Message, Origin},
    value::{ValidationReport, Value},
    wire::{ExtensionRegistry, FromWire, ToWire, WireExt},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

/// A tiny registered plugin verifies both concrete lanes use the wire seam.
#[derive(Debug, PartialEq)]
struct PluginCount(u32);

/// Plugin values are typed locally and validated when decoded.
impl WireExt for PluginCount {
    /// Stable test namespace shared by both lanes.
    const TYPE_NAME: &'static str = "ext:fixture/count";
    /// Exact integer counts use a JSON-compatible number.
    fn to_value(&self) -> Value {
        Value::Number(f64::from(self.0))
    }
    /// Reject numbers that cannot be represented as a count.
    fn from_value(value: &Value) -> Result<Self, ValidationReport> {
        if let Value::Number(number) = value
            && number.is_finite()
            && *number >= 0.0
            && *number <= f64::from(u32::MAX)
            && number.fract() == 0.0
        {
            return Ok(Self(*number as u32));
        }
        let mut report = ValidationReport::default();
        report.push("payload", "expected unsigned count");
        Err(report)
    }
}

/// Every active public command must preserve its exact name and payload.
#[test]
fn every_public_command_round_trips() {
    let registry = ExtensionRegistry::default();
    let mut covered = BTreeSet::new();
    for (name, payload) in command_values()
        .chain(selection::command_values())
        .chain(search::command_values())
    {
        covered.insert(name);
        let command = Command::from_value(name, &payload, &registry).expect("active command");
        assert_eq!(command.type_name(), name);
        assert_eq!(command.to_value().expect("wire command"), payload);
        assert_eq!(command.visibility(), Visibility::Public);
        assert_eq!(
            <Command as Message>::catalogue()[command.kind_index()].name,
            name
        );
    }
    assert_active_coverage(covered, catalogue::COMMANDS);
}

/// Representative requests keep exhaustive wire coverage separate from assertions.
fn command_values() -> impl Iterator<Item = (&'static str, Value)> {
    [
        (
            "input/key",
            super::key_wire::to_value(&puzzle_vt::input::KeyEvent::press(
                puzzle_vt::input::Key::Up,
            )),
        ),
        (
            "terminal/select",
            super::selection_wire::to_value(crate::selection::SelectionCommand::All),
        ),
        ("input/mouse", super::mouse_wire::to_value(&mouse_event())),
        ("viewport/scroll", Value::Number(-1.5)),
        (
            "viewport/scrollTo",
            super::viewport_wire::target_value(crate::viewport::ScrollTarget::Top),
        ),
        ("input/text", Value::String("ä🦀".into())),
        ("input/focus", Value::Bool(true)),
        (
            "input/preedit",
            Value::Map(BTreeMap::from([
                ("text".into(), Value::String("日本".into())),
                ("cursor".into(), Value::Number(3.0)),
            ])),
        ),
        ("input/wheel", wheel_value()),
        ("input/paste", Value::String("line 1\nline 2".into())),
        ("platform/clipboardWrite", Value::String("copy".into())),
        ("platform/setPointerShape", Value::String("pointer".into())),
        (
            "platform/notify",
            Value::Map(BTreeMap::from([
                ("title".into(), Value::String("task".into())),
                ("body".into(), Value::String("done".into())),
            ])),
        ),
        (
            "platform/openLink",
            Value::String("https://example.org".into()),
        ),
        ("platform/ringBell", Value::Null),
        ("settings/systemAppearance", Value::String("light".into())),
    ]
    .into_iter()
}

/// Every active public fact must preserve its exact name and payload.
#[test]
fn every_public_event_round_trips() {
    let registry = ExtensionRegistry::default();
    let mut covered = BTreeSet::new();
    for (name, payload) in events::values() {
        covered.insert(name);
        let event = Event::from_value(name, &payload, &registry).expect("active fact");
        assert_eq!(event.type_name(), name);
        assert_eq!(event.to_value().expect("wire fact"), payload);
        assert_eq!(event.visibility(), Visibility::Public);
        assert_eq!(
            <Event as Message>::catalogue()[event.kind_index()].name,
            name
        );
    }
    assert_active_coverage(covered, catalogue::EVENTS);
}

/// Requires a sample whenever a public catalogue row becomes active.
fn assert_active_coverage(covered: BTreeSet<&str>, kinds: &[KindDef]) {
    // Plugin kinds use their own registered names and have a separate
    // round-trip test; built-ins must all appear in the fixed sample table.
    let active = kinds
        .iter()
        .filter(|kind| {
            kind.visibility == Visibility::Public
                && kind.availability == Availability::Active
                && kind.name != "ext:<plugin>/<name>"
        })
        .map(|kind| kind.name)
        .collect::<BTreeSet<_>>();
    assert_eq!(covered, active);
}

/// One plugin payload makes the trip through both typed enum lanes.
#[test]
fn extension_events_and_commands_round_trip() {
    let mut registry = ExtensionRegistry::default();
    registry
        .register::<PluginCount>()
        .expect("plugin registration");
    let payload = Value::Number(7.0);
    let command =
        Command::from_value(PluginCount::TYPE_NAME, &payload, &registry).expect("request");
    let event = Event::from_value(PluginCount::TYPE_NAME, &payload, &registry).expect("fact");
    assert_eq!(command.type_name(), PluginCount::TYPE_NAME);
    assert_eq!(event.type_name(), PluginCount::TYPE_NAME);
    assert_eq!(command.to_value().expect("request wire"), payload);
    assert_eq!(event.to_value().expect("fact wire"), payload);
}

/// Reserved and internal names are documented but cannot be constructed.
#[test]
fn reserved_and_internal_kinds_cannot_enter_wire() {
    let registry = ExtensionRegistry::default();
    for kind in catalogue::COMMANDS {
        if !matches!(kind.availability, Availability::Active)
            || kind.visibility == Visibility::Internal
        {
            assert!(Command::from_value(kind.name, &Value::Null, &registry).is_err());
        }
    }
    for kind in catalogue::EVENTS {
        if !matches!(kind.availability, Availability::Active)
            || kind.visibility == Visibility::Internal
        {
            assert!(Event::from_value(kind.name, &Value::Null, &registry).is_err());
        }
    }
}

/// Active variants retain their catalogue bits while reserved rows remain holes.
#[test]
fn concrete_kind_indices_match_catalogue() {
    let commands = [
        Command::Key(puzzle_vt::input::KeyEvent::press(puzzle_vt::input::Key::Up)),
        Command::Text(String::new()),
        Command::Paste(String::new()),
        Command::ClipboardWrite(String::new().into()),
        Command::Notify {
            title: String::new(),
            body: String::new(),
        },
        Command::SetPointerShape(String::new()),
        Command::RingBell,
        Command::OpenLink("https://example.org".into()),
        Command::SetSystemAppearance(crate::theme::Appearance::Light),
        Command::Ext(puzzle_bus::wire::Extension::new(PluginCount(1)).expect("plugin")),
    ];
    for command in commands {
        let kind = &catalogue::COMMANDS[command.kind_index()];
        assert_eq!(kind.availability, Availability::Active);
        assert_eq!(
            kind.name,
            if matches!(command, Command::Ext(_)) {
                "ext:<plugin>/<name>"
            } else {
                command.type_name()
            }
        );
    }
}

/// Event mask bits must still point at their named catalogue entries.
#[test]
fn concrete_event_indices_match_catalogue() {
    let events = [
        Event::TitleChanged(String::new()),
        Event::IconNameChanged(String::new()),
        Event::CwdChanged(String::new()),
        Event::ContextsChanged(Vec::new()),
        Event::Bell,
        Event::PromptStarted,
        Event::InputStarted,
        Event::CommandStarted(None),
        Event::CommandFinished(None),
        Event::GridResized {
            columns: 80,
            rows: 24,
        },
        Event::Progress {
            state: 0,
            percent: 0,
        },
        Event::ThemeChanged {
            name: "puzzle-light".into(),
            appearance: crate::theme::Appearance::Light,
        },
        Event::SessionExited(super::ExitInfo {
            code: Some(3),
            signal: None,
            runtime_ms: 200,
        }),
        Event::SessionCloseRequested,
        Event::Ext(puzzle_bus::wire::Extension::new(PluginCount(1)).expect("plugin")),
    ];
    for event in events {
        let kind = &catalogue::EVENTS[event.kind_index()];
        assert_eq!(kind.availability, Availability::Active);
        assert_eq!(
            kind.name,
            if matches!(event, Event::Ext(_)) {
                "ext:<plugin>/<name>"
            } else {
                event.type_name()
            }
        );
    }
}

/// A known kind with the wrong shape returns an explanatory report.
#[test]
fn wrong_payload_shape_has_report() {
    let registry = ExtensionRegistry::default();
    let command = Command::from_value("input/text", &Value::Bool(true), &registry);
    let event = Event::from_value("terminal/bell", &Value::Bool(true), &registry);
    assert!(command.is_err_and(|report| !report.issues.is_empty()));
    assert!(event.is_err_and(|report| !report.issues.is_empty()));
}

/// Host-created text must obey the same wire limit as external submissions.
#[test]
fn typed_text_too_large_for_wire_is_reported() {
    let oversized = "x".repeat(1_048_577);
    assert!(Command::Text(oversized.clone()).to_value().is_err());
    assert!(Event::TitleChanged(oversized).to_value().is_err());
}

/// A policy sees the trusted host origin of a validated external command.
#[test]
fn external_dispatch_has_api_origin() {
    /// Records the source and preserves normal policy flow.
    struct Inspect(Arc<Mutex<Vec<Origin>>>);
    /// The production generic hook contract is exercised with term enums.
    impl Interceptor<Command, Event> for Inspect {
        /// The API source arrives after fallible wire conversion.
        fn intercept(&mut self, _: &mut Command, ctx: &mut HookCtx<'_, Command, Event>) -> Flow {
            self.0
                .lock()
                .expect("test lock")
                .push(ctx.origin().expect("origin"));
            Flow::Continue
        }
    }
    let origins = Arc::new(Mutex::new(Vec::new()));
    let mut bus = Bus::<Command, Event>::default();
    bus.intercept(CommandKinds::ALL, 0, "inspect", Inspect(origins.clone()))
        .expect("hook");
    bus.submit_wire(
        "input/text",
        &Value::String("hi".into()),
        &ExtensionRegistry::default(),
    )
    .expect("wire");
    bus.drain(|_, _| {});
    assert_eq!(*origins.lock().expect("test lock"), [Origin::Api]);
}

/// Nested arbitrary input catches panics from shape checks and tree limits.
fn arbitrary_value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<f64>().prop_map(Value::Number),
        ".{0,32}".prop_map(Value::String),
        prop::collection::vec(any::<u8>(), 0..32).prop_map(Value::Bytes),
    ];
    leaf.prop_recursive(5, 64, 8, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..8).prop_map(Value::List),
            prop::collection::btree_map("[a-z]{0,8}", inner, 0..8)
                .prop_map(|fields: BTreeMap<String, Value>| Value::Map(fields)),
        ]
    })
}

proptest! {
    /// Random names and scalar payloads cannot unwind in either decoder.
    #[test]
    fn invalid_wire_input_never_panics(name in ".{0,80}", number in any::<f64>(), text in ".{0,100}") {
        let registry = ExtensionRegistry::default();
        for value in [Value::Number(number), Value::String(text), Value::Null] {
            let _ = Command::from_value(&name, &value, &registry);
            let _ = Event::from_value(&name, &value, &registry);
        }
    }

    /// Every catalogue name must reject or decode arbitrary trees safely.
    #[test]
    fn nested_wire_input_never_panics(value in arbitrary_value()) {
        let registry = ExtensionRegistry::default();
        for kind in catalogue::COMMANDS { let _ = Command::from_value(kind.name, &value, &registry); }
        for kind in catalogue::EVENTS { let _ = Event::from_value(kind.name, &value, &registry); }
    }
}

/// Uses distinct device and cell positions to detect coordinate-unit loss.
fn mouse_event() -> puzzle_vt::input::MouseEvent {
    use puzzle_vt::input::{Modifiers, MouseEvent, MouseKind};
    MouseEvent {
        kind: MouseKind::Move,
        button: 0,
        column: 3,
        row: 4,
        pixel_x: 24,
        pixel_y: 64,
        modifiers: Modifiers(4),
        button_held: true,
        click_count: 2,
    }
}

/// A wire caret cannot split an emoji scalar or exceed provisional text.
#[test]
fn preedit_rejects_invalid_caret_offsets() {
    let registry = ExtensionRegistry::default();
    for cursor in [-1.0, 1.0, 3.5, 7.0, f64::NAN, f64::INFINITY] {
        let value = Value::Map(BTreeMap::from([
            ("text".into(), Value::String("日語".into())),
            ("cursor".into(), Value::Number(cursor)),
        ]));
        assert!(Command::from_value("input/preedit", &value, &registry).is_err());
    }
}

/// Viewport messages retain their types through recordings and reject bad numbers.
#[test]
fn viewport_wire_roundtrips() {
    use crate::viewport::ScrollTarget;
    use crate::vt::tracked::LineId;
    let registry = puzzle_bus::wire::ExtensionRegistry::default();
    for command in [
        Command::Scroll(-1.5),
        Command::ScrollTo(ScrollTarget::Top),
        Command::ScrollTo(ScrollTarget::Bottom),
        Command::ScrollTo(ScrollTarget::Line(LineId(42))),
        Command::ScrollTo(ScrollTarget::LineTop(LineId(42))),
        Command::ScrollTo(ScrollTarget::Prompt(-1)),
        Command::ScrollTo(ScrollTarget::Match(1)),
    ] {
        let value = command.to_value().expect("wire");
        let restored = Command::from_value(command.type_name(), &value, &registry).expect("typed");
        assert_eq!(restored.to_value().expect("roundtrip"), value);
    }
    for event in [Event::ViewportScrolled(20.0), Event::NewOutputChanged(true)] {
        let value = event.to_value().expect("event");
        let restored =
            Event::from_value(event.type_name(), &value, &registry).expect("typed event");
        assert_eq!(restored.to_value().expect("roundtrip"), value);
    }
    assert!(Command::from_value("viewport/scroll", &Value::Number(f64::NAN), &registry).is_err());
}

/// Supplies a unit-tagged hardware record to the catalogue coverage witness.
fn wheel_value() -> Value {
    super::wheel_wire::to_value(
        crate::viewport::ScrollInput::Pixels(-3.5),
        &crate::vt::input::MouseEvent {
            button: 64,
            kind: crate::vt::input::MouseKind::Wheel,
            column: 1,
            row: 2,
            pixel_x: 10,
            pixel_y: 20,
            modifiers: crate::vt::input::Modifiers(0),
            button_held: false,
            click_count: 0,
        },
    )
}

/// Selection protocol cases keep the main catalogue coverage concise.
mod selection;

/// Literal-search options and request facts at the public wire seam.
mod search;

/// Completed-frame wire counter boundaries.
mod frame_stats;

/// Recorded lifecycle facts, constructed independently of their codecs.
mod lifecycle;

/// Recorded fact payloads and bounded command-label compatibility.
mod events;
