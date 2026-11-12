//! Appearance reports use the same transport order as VT query replies.
//!
//! These tests keep the platform's RGB choice out of puzzle-vt: the
//! session updates only the parser's query defaults and frame policy.
use super::*;
use crate::{
    settings::{BlinkMode, Settings, ThemeMode},
    theme::{Appearance, Theme, color},
};
use puzzle_text::{FontData, font_set::FontSet};
use puzzle_vt::color_table::{ColorSlot, ColorValue, DynamicColor};

/// Keeps approved font bytes outside the platform-neutral Session.
fn frame_context() -> crate::frame_builder::context::TextContext {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/JetBrainsMono-Variable.ttf"
    ))
    .expect("bundled font fixture");
    let fonts = FontSet::new(FontData::new(bytes, 0), None, None).expect("font set");
    crate::frame_builder::context::TextContext::new(fonts, 14.0, 1.0).expect("context")
}

/// Browser startup resolves settings before asynchronous font data arrives.
#[test]
fn theme_before_fonts_reaches_first_frame_and_context_replacement() {
    let (mut session, _, _) = fixture();
    let now = Instant::now();
    session.apply_settings(
        &Settings {
            theme_mode: ThemeMode::Light,
            cursor_blink: BlinkMode::Off,
            ..Settings::default()
        },
        now,
    );
    let expected = Theme::builtin("puzzle-light").expect("light theme");
    // Both the pack-first builder and its full-font replacement keep colors.
    for _ in 0..2 {
        let mut context = frame_context();
        session.set_text_context(&context);
        let frame = session.draw(now, &mut context).expect("frame");
        let cursor = frame.rects.last().expect("cursor").color;
        assert_eq!(
            [cursor as u8, (cursor >> 8) as u8, (cursor >> 16) as u8],
            expected.cursor
        );
        for (channel, byte) in frame.clear[..3].iter().zip(expected.background) {
            assert!((channel - f32::from(byte) / 255.0).abs() < 0.0001);
        }
    }
}

/// Reads the filled cursor's RGB from the same frame sent to the GPU.
fn cursor_rgb(
    session: &mut TestHost,
    context: &mut crate::frame_builder::context::TextContext,
    now: Instant,
) -> [u8; 3] {
    let frame = session.draw(now, context).expect("appearance frame");
    let packed = frame
        .rects
        .last()
        .unwrap_or_else(|| {
            panic!(
                "cursor rectangle; batches={}, slugs={}",
                frame.batches.len(),
                frame.slugs.len()
            )
        })
        .color;
    [packed as u8, (packed >> 8) as u8, (packed >> 16) as u8]
}

/// A fade begins at the prior cursor token and reaches its exact target.
fn switch_cursor(
    session: &mut TestHost,
    context: &mut crate::frame_builder::context::TextContext,
    at: Instant,
    appearance: Appearance,
    from: [u8; 3],
    to: [u8; 3],
) {
    session.submit(Command::SetSystemAppearance(appearance), Origin::Platform);
    session.pump(at);
    assert_eq!(cursor_rgb(session, context, at), from);
    assert_eq!(
        cursor_rgb(session, context, at + Duration::from_millis(150)),
        to
    );
}

/// An OS theme switch must not inherit an invisible cursor blink phase.
#[test]
fn appearance_switch_reveals_cursor_after_blink_off_phase() {
    let mut context = frame_context();
    let (mut session, _, _) = fixture();
    let start = Instant::now();
    session.set_text_context(&context);
    session.apply_settings(
        &Settings {
            theme_transition_ms: 150,
            ..Settings::default()
        },
        start,
    );
    assert_eq!(
        cursor_rgb(&mut session, &mut context, start),
        Theme::builtin("puzzle-dark").expect("dark").cursor
    );
    let hidden = session
        .draw(start + Duration::from_millis(530), &mut context)
        .expect("blink frame");
    assert!(hidden.rects.is_empty(), "blink phase must first be off");
    let changed = start + Duration::from_millis(600);
    let dark = Theme::builtin("puzzle-dark").expect("dark");
    let light = Theme::builtin("puzzle-light").expect("light");
    switch_cursor(
        &mut session,
        &mut context,
        changed,
        Appearance::Light,
        dark.cursor,
        light.cursor,
    );
    let hidden_again = session
        .draw(changed + Duration::from_millis(530), &mut context)
        .expect("second blink frame");
    assert!(
        hidden_again.rects.is_empty(),
        "blink phase must be off again"
    );
    let changed_back = changed + Duration::from_millis(600);
    switch_cursor(
        &mut session,
        &mut context,
        changed_back,
        Appearance::Dark,
        light.cursor,
        dark.cursor,
    );
}

/// Repeated OS changes retain the resolved cursor token through every fade.
#[test]
fn repeated_appearance_switch_recolors_the_cursor() {
    let mut context = frame_context();
    let (mut session, _, _) = fixture();
    let start = Instant::now();
    session.set_text_context(&context);
    session.apply_settings(
        &Settings {
            theme_transition_ms: 150,
            cursor_blink: BlinkMode::Off,
            ..Settings::default()
        },
        start,
    );
    assert!(
        session
            .terminal
            .buffer()
            .cursor()
            .expect("cursor view")
            .visible
    );
    let dark = Theme::builtin("puzzle-dark").expect("dark");
    assert_eq!(cursor_rgb(&mut session, &mut context, start), dark.cursor);
    for (index, appearance) in [
        Appearance::Light,
        Appearance::Dark,
        Appearance::Light,
        Appearance::Dark,
    ]
    .into_iter()
    .enumerate()
    {
        let now = start + Duration::from_millis(500 * (index as u64 + 1));
        session.submit(Command::SetSystemAppearance(appearance), Origin::Platform);
        session.pump(now);
        let active = session.active_theme().expect("active").clone();
        let _ = cursor_rgb(&mut session, &mut context, now);
        assert_eq!(
            cursor_rgb(&mut session, &mut context, now + Duration::from_millis(75)),
            color::mix(
                if appearance == Appearance::Light {
                    dark.cursor
                } else {
                    Theme::builtin("puzzle-light").expect("light").cursor
                },
                active.cursor,
                0.5
            )
        );
        assert_eq!(
            cursor_rgb(&mut session, &mut context, now + Duration::from_millis(150)),
            active.cursor
        );
    }
}

/// A host switch clears every OSC color and reports exact mode-2031 bytes.
#[test]
fn appearance_switch_reports_2031_and_recolors_queries() {
    let (mut session, writes, wakes) = fixture();
    session.apply_settings(&Settings::default(), Instant::now());
    // The child owns all four dynamic overrides until the host changes
    // appearance; old RGB values must not tint a new theme's blank cells.
    session.feed(
        b"\x1b[?2031h\x1b]4;1;#123456\x07\x1b]10;#223344\x07\x1b]11;#334455\x07\x1b]12;#445566\x07",
        Instant::now(),
    );
    assert_eq!(
        session
            .terminal
            .colors
            .override_value(ColorSlot::Palette(1))
            .and_then(ColorValue::rgb),
        Some([0x12, 0x34, 0x56])
    );
    session.submit(
        Command::SetSystemAppearance(Appearance::Light),
        Origin::Platform,
    );
    session.pump(Instant::now());
    let active = session.active_theme().expect("resolved light theme");
    assert_eq!(active.name, "puzzle-light");
    for slot in colour_slots() {
        assert_eq!(
            session.terminal.colors.override_value(slot),
            None,
            "{slot:?}"
        );
    }
    assert_eq!(
        session
            .terminal
            .colors
            .get(ColorSlot::Dynamic(DynamicColor::Background))
            .rgb()
            .expect("validated colour fixture"),
        active.background
    );
    assert_eq!(
        writes.lock().expect("writes").last(),
        Some(&b"\x1b[?997;2n".to_vec())
    );
    // OSC 10 queries see the new theme instead of the old dynamic color.
    session.feed(b"\x1b]10;?\x07", Instant::now());
    let query = writes
        .lock()
        .expect("writes")
        .last()
        .cloned()
        .expect("query reply");
    assert!(query.starts_with(b"\x1b]10;rgb:"));
    assert!(wakes.load(Ordering::Relaxed) > 0);
}

/// A resolved setting change reports once; a reload with equal colors does not.
#[test]
fn settings_theme_change_reports_2031_once_per_resolved_change() {
    let (mut session, writes, _) = fixture();
    let start = Instant::now();
    session.apply_settings(&Settings::default(), start);
    session.feed(b"\x1b[?2031h", start);
    assert!(writes.lock().expect("writes").is_empty());

    // Config reload, CLI --set and profile selection all arrive through
    // apply_settings. Their cause does not change the mode-2031 contract.
    let light = Settings {
        theme_mode: ThemeMode::Light,
        ..Settings::default()
    };
    session.apply_settings(&light, start + Duration::from_millis(1));
    assert_eq!(
        *writes.lock().expect("writes"),
        vec![b"\x1b[?997;2n".to_vec()]
    );

    // Cursor policy may change on the next reload, but the same resolved
    // light palette must retain its OSC overrides and send no new report.
    session.feed(b"\x1b]10;#123456\x07", start + Duration::from_millis(1));
    let unrelated = Settings {
        cursor_blink: BlinkMode::Off,
        ..light
    };
    session.apply_settings(&unrelated, start + Duration::from_millis(2));
    session.apply_settings(&unrelated, start + Duration::from_millis(3));
    assert_eq!(
        session
            .terminal
            .colors
            .override_value(ColorSlot::Dynamic(DynamicColor::Foreground))
            .and_then(ColorValue::rgb),
        Some([0x12, 0x34, 0x56])
    );
    assert_eq!(writes.lock().expect("writes").len(), 1);

    let dark = Settings {
        theme_mode: ThemeMode::Dark,
        ..unrelated
    };
    session.apply_settings(&dark, start + Duration::from_millis(4));
    assert_eq!(
        *writes.lock().expect("writes"),
        vec![b"\x1b[?997;2n".to_vec(), b"\x1b[?997;1n".to_vec()]
    );
    assert_eq!(
        session
            .terminal
            .colors
            .override_value(ColorSlot::Dynamic(DynamicColor::Foreground))
            .and_then(ColorValue::rgb),
        None
    );
}

/// User-origin wire commands cannot impersonate an OS appearance change.
#[test]
fn appearance_command_requires_platform_origin() {
    let (mut session, writes, _) = fixture();
    session.apply_settings(&Settings::default(), Instant::now());
    session.submit(Command::SetSystemAppearance(Appearance::Light), Origin::Api);
    session.pump(Instant::now());
    assert_eq!(
        session.active_theme().expect("active theme").name,
        "puzzle-dark"
    );
    assert!(writes.lock().expect("writes").is_empty());
}

/// A configured fade changes render colors over injected time, while OSC
/// queries and the 2031 notification switch to the target immediately.
#[test]
fn configured_transition_samples_oklab_without_delaying_2031() {
    let (mut session, writes, _) = fixture();
    let start = Instant::now();
    let settings = Settings {
        theme_transition_ms: 100,
        ..Settings::default()
    };
    session.apply_settings(&settings, start);
    session.feed(b"\x1b[?2031h", start);
    let dark = Theme::builtin("puzzle-dark").expect("dark source");
    let light = Theme::builtin("puzzle-light").expect("light source");
    let switch = start + Duration::from_secs(1);
    session.submit(
        Command::SetSystemAppearance(Appearance::Light),
        Origin::Platform,
    );
    session.pump(switch);
    assert_eq!(
        session
            .terminal
            .colors
            .get(ColorSlot::Dynamic(DynamicColor::Background))
            .rgb()
            .expect("validated colour fixture"),
        light.background
    );
    assert_eq!(
        writes.lock().expect("writes").last(),
        Some(&b"\x1b[?997;2n".to_vec())
    );
    let first = session
        .theme_state
        .sample(switch)
        .expect("first fade sample");
    assert_eq!(first.background, dark.background);
    let middle = session
        .theme_state
        .sample(switch + Duration::from_millis(50))
        .expect("middle");
    assert_eq!(
        middle.background,
        color::mix(dark.background, light.background, 0.5)
    );
    assert_eq!(
        middle.palette[1],
        color::mix(dark.palette[1], light.palette[1], 0.5)
    );
    assert!(session.theme_state.transitioning());
    let final_colors = session
        .theme_state
        .sample(switch + Duration::from_millis(100))
        .expect("end");
    assert_eq!(final_colors, light);
    assert!(!session.theme_state.transitioning());
}

/// §14.7 clears even stored-only roles on appearance and configured-theme changes.
#[test]
fn every_colour_override_clears_on_both_theme_paths() {
    let (mut session, _, _) = fixture();
    let now = Instant::now();
    let mut settings = Settings::default();
    session.apply_settings(&settings, now);
    let slots = colour_slots();
    for appearance_path in [true, false] {
        for slot in &slots {
            session
                .terminal
                .colors
                .set(*slot, ColorValue::Rgb([1, 2, 3]));
        }
        if appearance_path {
            session.submit(
                Command::SetSystemAppearance(Appearance::Light),
                Origin::Platform,
            );
            session.pump(now);
        } else {
            settings.theme_mode = ThemeMode::Dark;
            session.apply_settings(&settings, now);
        }
        assert!(
            slots
                .iter()
                .all(|slot| session.terminal.colors.override_value(*slot).is_none())
        );
        assert_eq!(
            session
                .terminal
                .colors
                .get(ColorSlot::Dynamic(DynamicColor::SelectionBackground)),
            ColorValue::Rgb(
                session
                    .active_theme()
                    .expect("validated colour fixture")
                    .selection_background
            )
        );
    }
}

/// Enumerates every slot so a new stored-only role cannot escape theme-reset tests.
fn colour_slots() -> Vec<ColorSlot> {
    use puzzle_vt::color_table::SpecialColor;
    let roles = [
        DynamicColor::Foreground,
        DynamicColor::Background,
        DynamicColor::Cursor,
        DynamicColor::PointerForeground,
        DynamicColor::PointerBackground,
        DynamicColor::TektronixForeground,
        DynamicColor::TektronixBackground,
        DynamicColor::SelectionBackground,
        DynamicColor::TektronixCursor,
        DynamicColor::SelectionForeground,
        DynamicColor::CursorText,
        DynamicColor::VisualBell,
    ];
    (0..=255)
        .map(ColorSlot::Palette)
        .chain((0..5).map(|i| {
            ColorSlot::Special(SpecialColor::from_index(i).expect("validated colour fixture"))
        }))
        .chain(roles.into_iter().map(ColorSlot::Dynamic))
        .collect()
}
