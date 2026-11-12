//! Latency evidence must distinguish real appearances, redraws and censored work.
use super::*;
use puzzle_term::text::glyph::HintMode;
use std::time::Duration;

/// A scrolling row retains its original latency and is counted once on completion.
#[test]
fn presentations_count_ink_once_and_censor_unfinished_appearances() {
    let mut observer = Observer::default();
    observer.start();
    let start = Instant::now();
    observer.built.push(sample(1, false));
    observer.presented(start);
    let mut ready = sample(1, true);
    ready.row = 5;
    observer.built.push(ready);
    observer.built.push(sample(2, false));
    observer.presented(start + Duration::from_millis(20));
    observer.built.push(sample(1, true));
    observer.presented(start + Duration::from_millis(40));
    let report = observer.report(0.04);
    assert_eq!(report["covered_appearances"], 1);
    assert_eq!(report["max_ms"], 20.0);
    assert_eq!(report["p95_ms"], 20.0);
    assert_eq!(report["censored"], 1);
    assert_eq!(report["unresolved_visible"], 0);
}

/// No font or GPU work is involved: the fixture names already observed source requests.
fn sample(line: u64, ready: bool) -> InkSample {
    InkSample {
        line: puzzle_term::vt::tracked::LineId(line),
        row: 0,
        column: 0,
        span: 2,
        key: GlyphKey {
            face: 1,
            glyph_id: 20,
            instance: 0,
            hint_ppem: 0,
            hint_mode: HintMode::None,
        },
        ready,
    }
}
