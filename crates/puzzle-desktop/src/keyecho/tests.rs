//! Decoder and screen tests pin the simulator independently of the harness.
//!
//! A simulator that mislabels a release or swallows RAW input would let a
//! broken desktop adapter appear correct in the more expensive PTY scenario.

use super::{
    decode::{Decoder, Event, KeyReport, Phase},
    screen::Screen,
};

/// Kitty alternate identities do not turn a valid Ctrl chord into RAW input.
#[test]
fn kitty_alternates_keep_primary_key_and_modifiers() {
    for (bytes, letter, modifiers) in [
        (b"\x1b[97::97;5u".as_slice(), 'a', 4),
        (b"\x1b[100:68:100;6u".as_slice(), 'd', 5),
    ] {
        let mut decoder = Decoder::default();
        decoder.feed(bytes);
        assert_eq!(
            decoder.next(),
            Some(Event::Key(KeyReport {
                letter,
                modifiers,
                phase: Phase::Press,
            }))
        );
        assert_eq!(decoder.next(), None);
    }
}

/// PTY reads may split a kitty report between its modifier and final byte.
#[test]
fn split_kitty_event_and_release() {
    let mut decoder = Decoder::default();
    decoder.feed(b"\x1b[100;5");
    assert_eq!(decoder.next(), None);
    decoder.feed(b"u\x1b[100;5:3u");
    let press = Event::Key(KeyReport {
        letter: 'd',
        modifiers: 4,
        phase: Phase::Press,
    });
    let release = Event::Key(KeyReport {
        letter: 'd',
        modifiers: 4,
        phase: Phase::Release,
    });
    assert_eq!(decoder.next(), Some(press));
    assert_eq!(decoder.next(), Some(release));
}

/// Legacy Ctrl+D is a single C0 byte; it is not a printable `d` event.
#[test]
fn legacy_ctrl_byte_and_query_replies() {
    let mut decoder = Decoder::default();
    decoder.feed(b"\x04\x1b[?2026;1$y\x1b[?0u");
    assert_eq!(
        decoder.next(),
        Some(Event::Key(KeyReport {
            letter: 'd',
            modifiers: 4,
            phase: Phase::Press,
        }))
    );
    assert_eq!(decoder.next(), Some(Event::SyncSupported));
    assert_eq!(decoder.next(), Some(Event::KittyReply));
}

/// The first update contains showcmd; the second erases and scrolls it.
#[test]
fn numbered_scroll_erases_show_command() {
    let mut screen = Screen::new(3);
    let mut bytes = Vec::new();
    assert!(screen.start(&mut bytes).is_ok());
    screen.set_sync(true);
    assert!(screen.enable_kitty(&mut bytes).is_ok());
    assert!(
        screen
            .key(
                &mut bytes,
                KeyReport {
                    letter: 'd',
                    modifiers: 4,
                    phase: Phase::Press,
                },
            )
            .is_ok()
    );
    assert!(
        bytes
            .windows(b"\x1b[30;70H^D".len())
            .any(|w| w == b"\x1b[30;70H^D")
    );
    assert!(
        bytes
            .windows(b"\x1b[30;70H          ".len())
            .any(|w| w == b"\x1b[30;70H          ")
    );
    assert!(bytes.windows(b"\x1b[11M".len()).any(|w| w == b"\x1b[11M"));
}

/// Unsupported flags cannot turn a test into a different protocol exercise.
#[test]
fn only_required_flag_sets_parse() {
    for value in ["0", "1", "3", "31"] {
        assert_eq!(
            super::parse_flags(&["--kitty".into(), value.into()]).ok(),
            value.parse::<u8>().ok().map(|flags| (flags, 2, None))
        );
    }
    assert!(super::parse_flags(&["--kitty".into(), "7".into()]).is_err());
}

/// Measured erase delays are explicit and cannot silently become zero.
#[test]
fn measured_gap_options() {
    for gap in [2, 5, 7] {
        let args = [
            "--kitty".into(),
            "1".into(),
            "--gap".into(),
            gap.to_string(),
        ];
        assert_eq!(super::parse_flags(&args).ok(), Some((1, gap, None)));
    }
    let bad = ["--kitty".into(), "1".into(), "--gap".into(), "17".into()];
    assert!(super::parse_flags(&bad).is_err());
}

/// A held key uses a fixed cadence, and malformed values cannot skip it.
#[test]
fn held_key_repeat_option() {
    let args = [
        "--kitty".into(),
        "1".into(),
        "--gap".into(),
        "5".into(),
        "--repeat-ms".into(),
        "33".into(),
    ];
    assert_eq!(super::parse_flags(&args).ok(), Some((1, 5, Some(33))));
    let mut bad = args;
    bad[5] = "16".into();
    assert!(super::parse_flags(&bad).is_err());
}
