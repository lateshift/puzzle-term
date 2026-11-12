//! G1 host labels are parsed by Puzzle, never by the embedding application's UI.
use super::*;

/// Preserve UTF-8 and command boundaries even with absent/malformed/large metadata.
#[test]
fn command_started_carries_bounded_labels_through_the_host_outbox() {
    let (mut session, _, _) = fixture();
    session.enable_host_events().expect("host outbox");
    let cases = [
        (
            "cargo%20build%20%27a%3Bb%27%20%C3%A9".into(),
            Some("cargo build 'a;b' é".into()),
        ),
        ("a".repeat(1024), Some("a".repeat(1024))),
        ("%C3%A9".repeat(600), Some(format!("{}…", "é".repeat(510)))),
        ("bad%GG".into(), None),
        ("x%1B%0Ay".into(), Some("x  y".into())),
    ];
    for (encoded, expected) in cases {
        let bytes = format!("\x1b]133;C;ignored=yes;cmdline_url={encoded}\x07").into_bytes();
        // An arbitrary transport split must produce one complete command fact.
        for chunk in bytes.chunks(17) {
            session.feed(chunk, Instant::now());
        }
        let labels = command_labels(&mut session);
        assert_eq!(labels, [expected]);
        for command in labels.into_iter().flatten() {
            assert!(command.len() <= puzzle_vt::MAX_COMMAND_BYTES);
            assert!(!command.chars().any(char::is_control));
        }
    }
    // Old integrations still emit the start event, explicitly without a label.
    session.feed(b"\x1b]133;C\x07", Instant::now());
    let event = std::iter::from_fn(|| session.take_event())
        .find_map(|event| match event {
            crate::HostEvent::Fact(Event::CommandStarted(command)) => {
                Some(Event::CommandStarted(command))
            }
            _ => None,
        })
        .expect("bare C fact");
    assert_eq!(
        event.to_value().expect("wire"),
        puzzle_bus::Value::Map(std::collections::BTreeMap::from([(
            "command".into(),
            puzzle_bus::Value::Null
        )]))
    );
}

/// Companion tab facts share the outbox without a host OSC decoder.
#[test]
fn shell_companion_facts_reach_the_host_outbox() {
    let (mut session, _, _) = fixture();
    session.enable_host_events().expect("host outbox");
    // The companion tab facts share the same outbox, with no host OSC decoder.
    session.feed(
        b"\x1b]7;file:///repo\x07\x1b]9;4;1;50\x07\x1b]133;D;7\x07",
        Instant::now(),
    );
    let names: Vec<_> = std::iter::from_fn(|| session.take_event())
        .filter_map(|event| match event {
            crate::HostEvent::Fact(fact) => Some(fact.type_name().to_owned()),
            _ => None,
        })
        .collect();
    assert_eq!(
        names,
        [
            "terminal/cwdChanged",
            "terminal/progress",
            "shell/commandFinished"
        ]
    );
}

/// Drain start facts without confusing absent labels with absent start events.
fn command_labels(session: &mut TestHost) -> Vec<Option<String>> {
    std::iter::from_fn(|| session.take_event())
        .filter_map(|event| match event {
            crate::HostEvent::Fact(Event::CommandStarted(command)) => Some(command),
            _ => None,
        })
        .collect()
}
