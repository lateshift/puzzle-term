//! A hidden cursor phase may not survive re-arming or the idle stop.
//!
//! The frame builder owns a blink phase while Session owns its deadline.
//! A settings change can cancel that deadline. If it later re-enables
//! blinking after the 15-second idle cutoff, no new timer is scheduled;
//! the old hidden phase would then remain invisible indefinitely. These
//! cases exercise the actual frame rectangles at injected times, including
//! host changes that rebuild geometry without touching VT cursor position.

use super::*;
use crate::{
    frame_builder::context::TextContext,
    settings::{Settings, ThemeMode},
    theme::Appearance,
};
use puzzle_text::{FontData, font_set::FontSet};

/// Operations that can land while the caret is in its 530-ms off phase.
#[derive(Clone, Copy, Debug)]
enum Change {
    /// The operating system selects the other theme.
    Appearance,
    /// VT and PTY acquire new cell geometry.
    Resize,
    /// A user edits the cursor color or blink choice.
    CursorReload,
    /// A user forces the other member of the theme pair.
    ThemeReload,
    /// A focus round trip changes filled and hollow geometry.
    Focus,
    /// A monitor scale change selects a different text context key.
    Dpr,
    /// The finite blink window reaches its idle cutoff while hidden.
    IdleStop,
    /// Blinking is disabled while hidden, then re-enabled after idle.
    RearmAfterIdle,
}

/// Loads only the approved test font outside the neutral Session.
fn fixture_with_context() -> (TestHost, TextContext) {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/JetBrainsMono-Variable.ttf"
    ))
    .expect("bundled font fixture");
    let fonts = FontSet::new(FontData::new(bytes, 0), None, None).expect("font set");
    let context = TextContext::new(fonts, 14.0, 1.0).expect("context");
    let (mut session, _, _) = fixture();
    session.set_text_context(&context);
    (session, context)
}

/// Reports whether a submitted frame contains the cursor rectangle.
fn cursor_drawn(session: &mut TestHost, context: &mut TextContext, at: Instant) -> Option<bool> {
    session
        .draw(at, context)
        .map(|frame| !frame.rects.is_empty())
}

/// Every listed change recovers a hidden caret within one blink period.
#[test]
fn host_changes_and_idle_stop_never_strand_a_hidden_cursor() {
    let cases = [
        Change::Appearance,
        Change::Resize,
        Change::CursorReload,
        Change::ThemeReload,
        Change::Focus,
        Change::Dpr,
        Change::IdleStop,
        Change::RearmAfterIdle,
    ];
    for case in cases {
        let (mut session, mut context) = fixture_with_context();
        let start = Instant::now();
        session.apply_settings(&Settings::default(), start);
        assert_eq!(
            cursor_drawn(&mut session, &mut context, start),
            Some(true),
            "{case:?}"
        );
        let hidden_at = start + Duration::from_millis(530);
        assert_eq!(
            cursor_drawn(&mut session, &mut context, hidden_at),
            Some(false),
            "{case:?}"
        );
        let changed = hidden_at + Duration::from_millis(70);
        apply_change(case, &mut session, &mut context, changed);
        verify_recovery(case, &mut session, &mut context, changed, start);
    }
}

/// Mutates one host-controlled input while the saved phase is off.
fn apply_change(case: Change, session: &mut TestHost, context: &mut TextContext, changed: Instant) {
    match case {
        Change::Appearance => {
            session.submit(
                Command::SetSystemAppearance(Appearance::Light),
                Origin::Platform,
            );
            session.pump(changed);
        }
        Change::Resize => session.resize_grid(5, 1, [100, 64]).expect("resize"),
        Change::CursorReload => session.apply_settings(
            &Settings {
                cursor_color: "inverse".into(),
                ..Settings::default()
            },
            changed,
        ),
        Change::ThemeReload => session.apply_settings(
            &Settings {
                theme_mode: ThemeMode::Light,
                ..Settings::default()
            },
            changed,
        ),
        Change::Focus => {
            session.set_focus(false);
            assert_eq!(cursor_drawn(session, context, changed), Some(true));
            session.set_focus(true);
        }
        Change::Dpr => {
            context.reconfigure(14.0, 2.0).expect("DPR");
            session.set_text_context(context);
        }
        Change::IdleStop | Change::RearmAfterIdle => {}
    }
}

/// Separates ordinary host recovery from the two idle-cutoff invariants.
fn verify_recovery(
    case: Change,
    session: &mut TestHost,
    context: &mut TextContext,
    changed: Instant,
    start: Instant,
) {
    match case {
        Change::IdleStop => verify_idle_stop(session, context, start),
        Change::RearmAfterIdle => verify_rearm(session, context, changed, start),
        _ => {
            let immediate = cursor_drawn(session, context, changed);
            let recovered = immediate == Some(true)
                || cursor_drawn(session, context, changed + Duration::from_millis(500))
                    == Some(true);
            assert!(recovered, "{case:?} left cursor hidden for a blink period");
            // A host change may re-arm blinking without changing the last
            // input time. Its eventual idle cutoff must still paint the
            // caret solid, even if the intermediate phase was invisible.
            let stopped = changed + Duration::from_secs(16);
            let _ = cursor_drawn(session, context, stopped);
            assert!(
                session.builder.as_ref().expect("builder").blink_phase(),
                "{case:?} remained hidden after idle stop"
            );
            assert!(
                session.blink_deadline.is_none(),
                "{case:?} re-armed after idle"
            );
            assert_eq!(
                cursor_drawn(session, context, stopped + Duration::from_secs(1)),
                None,
                "{case:?} requested an idle frame"
            );
        }
    }
}

/// The final timer makes the phase visible and schedules no more frames.
fn verify_idle_stop(session: &mut TestHost, context: &mut TextContext, start: Instant) {
    let stopped = start + Duration::from_secs(15);
    assert_eq!(cursor_drawn(session, context, stopped), Some(true));
    assert!(
        session.blink_deadline.is_none(),
        "idle stop must not re-arm"
    );
    assert_eq!(
        cursor_drawn(session, context, stopped + Duration::from_secs(1)),
        None
    );
    assert!(session.builder.as_ref().expect("builder").blink_phase());
}

/// Disabling blink during an off phase must not strand a later Auto setting.
fn verify_rearm(
    session: &mut TestHost,
    context: &mut TextContext,
    changed: Instant,
    start: Instant,
) {
    use crate::session::cursor_settings::{CursorBlink, CursorSettings};
    session.set_cursor_settings(
        CursorSettings {
            blink: CursorBlink::Off,
            ..Default::default()
        },
        changed,
    );
    assert_eq!(cursor_drawn(session, context, changed), Some(true));
    let late = start + Duration::from_secs(20);
    session.set_cursor_settings(CursorSettings::default(), late);
    assert_eq!(cursor_drawn(session, context, late), Some(true));
    assert!(
        session.blink_deadline.is_none(),
        "idle re-arm must stay solid"
    );
    assert_eq!(
        cursor_drawn(session, context, late + Duration::from_secs(1)),
        None
    );
    assert!(session.builder.as_ref().expect("builder").blink_phase());
}
