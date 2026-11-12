//! Refresh-anchored tasks must regain slack after every lost callback target.
use super::*;
/// Continuations spend existing slack and regain allowance at the next refresh.
#[wasm_bindgen_test::wasm_bindgen_test]
fn tasks_share_one_refresh_and_late_callbacks_recover() {
    let now = Instant::now();
    let mut host = Host::default();
    host.update(now, Duration::from_millis(16), Duration::from_millis(3));
    assert_eq!(host.window(now).0, Duration::from_millis(12));
    assert_eq!(
        host.window(now + Duration::from_millis(10)).0,
        Duration::from_millis(2)
    );
    assert_eq!(
        host.window(now + Duration::from_millis(13)).0,
        Duration::ZERO
    );
    // Neither idle priority nor a new glyph-count quantum is needed for progress.
    for n in 1..20 {
        let at = now + Duration::from_millis(n * 16 + 1);
        let (budget, edge, _) = host.window(at);
        assert_eq!(budget, Duration::from_millis(11));
        assert!(edge > at && edge - at <= Duration::from_millis(16));
    }
}
