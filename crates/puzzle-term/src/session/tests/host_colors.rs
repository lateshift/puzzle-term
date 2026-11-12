//! Host chrome observes defaults, never the current SGR cell or a theme name guess.
use super::*;

/// Drain only the copied colour events; other owner facts remain independently tested.
fn colors(host: &mut TestHost) -> Vec<([u8; 3], [u8; 3], bool)> {
    std::iter::from_fn(|| host.take_event())
        .filter_map(|event| {
            if let crate::HostEvent::Colors {
                background,
                foreground,
                dark,
            } = event
            {
                Some((background, foreground, dark))
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn default_colors_emit_once_then_follow_osc_changes_and_resets() {
    let (mut session, _, _) = fixture();
    session.enable_host_events().expect("events");
    let initial = colors(&mut session);
    assert_eq!(initial.len(), 1);
    assert!(colors(&mut session).is_empty());
    let now = Instant::now();
    session.feed(b"\x1b]10;#112233\x07\x1b]11;#fefefe\x07", now);
    assert_eq!(colors(&mut session), [([254; 3], [17, 34, 51], false)]);
    session.feed(b"\x1b[31mordinary text", now);
    assert!(
        colors(&mut session).is_empty(),
        "SGR is not a default colour"
    );
    session.feed(b"\x1b]110\x07\x1b]111\x07", now);
    assert_eq!(colors(&mut session), initial);
}
