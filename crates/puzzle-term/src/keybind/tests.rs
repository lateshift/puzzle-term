//! Public shortcut contract: layout identities, leaders and config reports.
#![allow(
    clippy::expect_used,
    reason = "binding failures identify the contract row under test"
)]
use super::*;
use crate::{
    settings::{SettingsStore, store::Layer},
    vt::input::{Key, Modifiers},
};
use proptest::prelude::*;
use puzzle_bus::Value;

/// Creates an isolated table without platform defaults masking a test chord.
fn table(lines: &[&str]) -> Keybindings {
    let mut table = Keybindings::default();
    table.apply(
        &lines.iter().map(|s| (*s).into()).collect::<Vec<_>>(),
        Source::User,
        Platform::Mac,
        false,
    );
    table
}
/// A test event keeps physical and layout identities separate.
fn key(ch: char, mods: u8, code: &str) -> KeyEvent {
    let mut key = KeyEvent::press(Key::Char(ch));
    key.modifiers = Modifiers(mods);
    key.code = Some(code.into());
    key
}

#[test]
/// Validates the grammar and reports invalid siblings without discarding valid keys.
fn parsing_and_per_entry_reports() {
    let Entry::Binding(binding) = Entry::parse(
        "all:performable:unconsumed:cmd+arrow_up>c=sendText:\\x1b\\n\\t\\u{1f600}",
        Platform::Mac,
        Source::User,
    )
    .expect("valid contract row") else {
        panic!("binding");
    };
    assert_eq!(binding.canonical(), "super+arrowup>c");
    assert_eq!(binding.action, Action::SendText("\x1b\n\t😀".into()));
    assert!(binding.prefixes.all && binding.prefixes.performable && binding.prefixes.unconsumed);
    for invalid in [
        "ctrl+=ignore",
        "ctrl+ctrl+a=ignore",
        "a=sendText:\\xq1",
        "a=sendText:\\u{d800}",
        "a=zoom:nan",
        "a=scrollPages:NaN",
        "a=paste:bad",
        "global:a=ignore",
        "a=new_tab",
        "a>b>=ignore",
    ] {
        assert!(
            Entry::parse(invalid, Platform::Mac, Source::User).is_err(),
            "{invalid}"
        );
    }
    let mut store = SettingsStore::default();
    let report = store.apply_key(
        "keyboard.bindings",
        Value::List(
            ["a=ignore", "b=notBuilt", "c=search"]
                .map(|s| Value::String(s.into()))
                .to_vec(),
        ),
        Layer::User,
    );
    assert_eq!(report.rejected.len(), 1);
    assert_eq!(report.rejected[0].path, "keyboard.bindings[1]");
    assert!(
        report.unavailable.is_empty(),
        "native search is available from M9.4"
    );
    assert_eq!(store.keybindings().list().len(), 2);
}

/// Every catalogue action parses with its declared argument shape.
#[test]
fn catalogue_arguments_parse() {
    for entry in ACTIONS {
        let arg = match entry.argument {
            "text" => ":text",
            "integer" | "number" => ":1",
            "appearance" => ":auto",
            _ => "",
        };
        let line = format!("a={}{}", entry.name, arg);
        let Entry::Binding(binding) =
            Entry::parse(&line, Platform::Mac, Source::User).expect("valid contract row")
        else {
            panic!("catalogue");
        };
        assert_eq!(binding.action.info().name, entry.name);
    }
}

#[test]
/// US shifted plus and German physical Y keep separate layout identities.
fn layouts_and_physical_precedence() {
    let mut table = table(&[
        "primary+plus=zoom:1",
        "super+KeyY=sendText:physical",
        "super+z=sendText:logical",
    ]);
    for (mods, code) in [(9, "Equal"), (8, "BracketRight")] {
        let mut event = key('+', mods, code);
        event.shifted = Some('+');
        assert_eq!(
            table.match_key(&event, 0).action,
            Some((Action::Zoom(1), false))
        );
        event.modifiers.0 |= 2;
        assert!(!table.match_key(&event, 0).consumed);
    }
    assert_eq!(
        table.match_key(&key('z', 8, "KeyY"), 0).action,
        Some((Action::SendText("physical".into()), false))
    );
    assert_eq!(
        table.match_key(&key('z', 8, "KeyZ"), 0).action,
        Some((Action::SendText("logical".into()), false))
    );
    assert!(!table.match_key(&key('x', 8, "KeyX"), 0).consumed);
}

#[test]
/// Pending leaders win over global chords and flush under the declared timeout.
fn sequences_flush_and_timeout() {
    let a = key('a', 4, "KeyA");
    let x = key('x', 0, "KeyX");
    let mut table = table(&["ctrl+a>c=ignore", "KeyC=sendText:physical"]);
    assert!(table.match_key(&a, 0).consumed);
    assert_eq!(table.pending(), std::slice::from_ref(&a));
    assert_eq!(
        table.match_key(&key('c', 0, "KeyC"), 1).action,
        Some((Action::Ignore, false))
    );
    assert!(table.match_key(&a, 10).consumed);
    let unmatched = table.match_key(&x, 20);
    assert_eq!(unmatched.forward, std::slice::from_ref(&a));
    assert!(!unmatched.consumed);
    table.match_key(&a, 30);
    assert!(table.expire(1_000_000).is_empty());
    table.timeout_ms = 10;
    assert_eq!(table.expire(40), [a]);
    assert!(table.pending().is_empty());
}

#[test]
/// Unavailable copy falls through, unconsumed actions encode, and IME owns composition.
fn prefixes_and_composition() {
    let mut table = table(&[
        "performable:super+c=copy",
        "unconsumed:all:a=sendText:hello",
        "b=ignore",
    ]);
    assert!(!table.match_key(&key('c', 8, "KeyC"), 0).consumed);
    let result = table.match_key(&key('a', 0, "KeyA"), 0);
    assert!(!result.consumed);
    assert_eq!(
        result.action,
        Some((Action::SendText("hello".into()), true))
    );
    let mut event = key('b', 0, "KeyB");
    event.composing = true;
    assert!(!table.match_key(&event, 0).consumed);
    assert!(table.test(&event).is_none());
    event.composing = false;
    assert!(table.match_key(&event, 0).consumed);
}

#[test]
/// Aliases and prefixes share one layer identity, while clear/unbind remove entries.
fn canonical_layers_and_clearing() {
    let mut store = SettingsStore::default();
    store.set_binding_platform(Platform::Mac, true);
    let strings =
        |lines: &[&str]| Value::List(lines.iter().map(|s| Value::String((*s).into())).collect());
    store.apply_key(
        "keyboard.bindings",
        strings(&[
            "cmd+v=ignore",
            "shift+ctrl+a=ignore",
            "performable:super+c=copy",
        ]),
        Layer::User,
    );
    store.apply_key(
        "keyboard.bindings",
        strings(&[
            "super+v=sendText:V",
            "ctrl+shift+a=sendText:A",
            "super+c=ignore",
        ]),
        Layer::Session,
    );
    let bindings = store.keybindings();
    for (ch, mods, output) in [('v', 8, "V"), ('a', 5, "A")] {
        let binding = bindings
            .test(&key(ch, mods, ""))
            .expect("valid contract row");
        assert_eq!(binding.source, Source::Session);
        assert_eq!(binding.action, Action::SendText(output.into()));
    }
    assert_eq!(
        bindings
            .list()
            .iter()
            .filter(|b| b.canonical() == "super+c")
            .count(),
        1
    );
    let report = store.apply_key(
        "keyboard.bindings",
        strings(&["super+w=ignore"]),
        Layer::Session,
    );
    assert!(
        report
            .unavailable
            .iter()
            .any(|i| i.message == "browser-reserved shortcut")
    );
    clearing(&mut store);
}

/// Clear and unbind apply after canonical layer precedence.
fn clearing(store: &mut SettingsStore) {
    let strings =
        |lines: &[&str]| Value::List(lines.iter().map(|s| Value::String((*s).into())).collect());
    store.apply_key(
        "keyboard.bindings",
        strings(&["clear", "a=ignore", "a=unbind", "b=ignore"]),
        Layer::Session,
    );
    assert_eq!(store.keybindings().list().len(), 1);
    store.apply_key("keyboard.bindings", strings(&[]), Layer::User);
    store.apply_key("keyboard.bindings", strings(&[]), Layer::Session);
    store.apply_key("keyboard.clearDefaults", Value::Bool(true), Layer::Session);
    assert!(store.keybindings().list().is_empty());
}

#[test]
/// Portable alias configurations retain action arguments and report unsupported tabs.
fn alias_spellings_and_real_world_lines() {
    aliases();
    alias_lines();
}

/// Every compatibility alias maps to a real typed action.
fn aliases() {
    for &(alias, name) in aliases::ALIASES {
        let info = ACTIONS
            .iter()
            .find(|a| a.name == name)
            .expect("valid contract row");
        let arg = match info.argument {
            "integer" | "number" => ":1",
            "text" => ":text",
            _ => "",
        };
        let Entry::Binding(binding) =
            Entry::parse(&format!("a={alias}{arg}"), Platform::Mac, Source::User)
                .expect("valid contract row")
        else {
            panic!("alias")
        };
        assert_eq!(binding.action.info().name, name);
    }
}

/// Representative alias configs preserve layout and action syntax.
fn alias_lines() {
    // Representative alias keybind configuration idioms (§14.9): snake
    // names, leaders, aliases and control-text payloads are preserved.
    for line in [
        "cmd+c=copy_to_clipboard",
        "cmd+v=paste_from_clipboard",
        "cmd+plus=increase_font_size:1",
        "cmd+minus=decrease_font_size:1",
        "cmd+0=reset_font_size",
        "cmd+k=clear_screen",
        "cmd+a=select_all",
        "shift+page_up=scroll_page_up",
        "shift+page_down=scroll_page_down",
        "cmd+home=scroll_to_top",
        "cmd+end=scroll_to_bottom",
        "cmd+arrow_up=jump_to_prompt:-1",
        "ctrl+a>c=text:\\x01",
        "cmd+r=reload_config",
        "cmd+comma=open_config",
        "alt+enter=toggle_fullscreen",
        "cmd+n=new_window",
        "cmd+w=close_window",
    ] {
        assert!(
            Entry::parse(line, Platform::Mac, Source::User).is_ok(),
            "{line}"
        );
    }
    assert!(Entry::parse("cmd+t=new_tab", Platform::Mac, Source::User).is_err());
}

#[test]
/// All OS tables retain the M6 clipboard and natural editing byte contract.
fn platform_defaults_preserve_m6_bytes() {
    use crate::vt::input::{InputModes, encode_key};
    for platform in [Platform::Mac, Platform::Windows, Platform::Linux] {
        let mut bindings = Keybindings::default();
        bindings.apply(
            &defaults::lines(platform),
            Source::Defaults,
            platform,
            false,
        );
        let mods = if platform == Platform::Mac { 8 } else { 5 };
        assert_eq!(
            bindings.match_key(&key('v', mods, "KeyV"), 0).action,
            Some((Action::Paste, false))
        );
        let copy = key('c', if platform == Platform::Mac { 8 } else { 4 }, "KeyC");
        assert!(!bindings.match_key(&copy, 0).consumed);
        assert_eq!(
            encode_key(&copy, InputModes::default()),
            if platform == Platform::Mac {
                vec![]
            } else {
                vec![3]
            }
        );
    }
    let mut bindings = Keybindings::default();
    bindings.apply(
        &defaults::lines(Platform::Mac),
        Source::Defaults,
        Platform::Mac,
        false,
    );
    for (name, mods, expected) in [
        (Key::Left, 8, Action::SendText("\x01".into())),
        (Key::Right, 8, Action::SendText("\x05".into())),
        (Key::Backspace, 8, Action::SendText("\x15".into())),
        (Key::Left, 2, Action::Esc("b".into())),
        (Key::Right, 2, Action::Esc("f".into())),
    ] {
        let mut event = KeyEvent::press(name);
        event.code = Some(
            match name {
                Key::Left => "ArrowLeft",
                Key::Right => "ArrowRight",
                _ => "Backspace",
            }
            .into(),
        );
        event.modifiers = Modifiers(mods);
        assert_eq!(
            bindings.match_key(&event, 0).action,
            Some((expected, false))
        );
    }
}
proptest! {
    #[test]
    /// Hostile configuration cannot panic while parsing escapes or chord identities.
    fn arbitrary_binding_text_never_panics(text in ".{0,5000}") {
        let parsed = Entry::parse(&text, Platform::Mac, Source::User);
        let mut store = SettingsStore::default();
        store.set_binding_platform(Platform::Mac, false);
        let report = store.apply_key("keyboard.bindings", Value::List(vec![
            Value::String("a=ignore".into()), Value::String(text.clone()),
        ]), Layer::User);
        if let Err(reason) = parsed {
            // The index identifies the rejected entry without copying arbitrary input
            // into logs; a sibling's successful parse cannot hide its failure.
            proptest::prop_assert!(!reason.trim().is_empty());
            proptest::prop_assert!(report.rejected.iter().any(|issue|
                issue.path == "keyboard.bindings[1]" && !issue.message.trim().is_empty()));
        }
    }
}
