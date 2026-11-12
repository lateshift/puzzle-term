//! Manual-clock cases for the display deadline and nvim's two-update flash.
#![allow(
    clippy::expect_used,
    reason = "manual-clock tests assert a scheduled frame exists"
)]
use super::*;
use puzzle_term::vt::terminal::Terminal;

/// An already queued late reply must not erase its owner delay from D-33's lead.
#[test]
fn earlier_publication_preserves_actual_owner_cost_without_counting_wait_twice() {
    let zero = Instant::now();
    let mut pace = Pacer::new(60_000, Duration::ZERO);
    // This frame receives immediately, but its answer needed two ms last turn.
    pace.snapshot_received(zero, [Duration::ZERO, Duration::from_millis(2)]);
    pace.presented(zero, zero, Duration::ZERO, zero + Duration::from_millis(3));
    assert_eq!(pace.base_cost(), Duration::from_millis(5));
    assert_eq!(pace.lead(), Duration::from_millis(6));
    // A freshly awaited answer has the same complete cost, not seven ms.
    let mut fresh = Pacer::new(60_000, Duration::ZERO);
    fresh.snapshot_received(
        zero + Duration::from_millis(2),
        [Duration::from_millis(2); 2],
    );
    fresh.presented(zero, zero, Duration::ZERO, zero + Duration::from_millis(5));
    assert_eq!(fresh.base_cost(), pace.base_cost());
}

/// Ordinary wake jitter leaves time for the owner without consuming the margin twice.
#[test]
fn publication_can_arrive_after_wake_jitter_and_still_fit_the_reserved_render_work() {
    let zero = Instant::now();
    let mut pace = Pacer::new(120_000, Duration::ZERO);
    pace.first = false;
    pace.phase = Some(zero);
    let owner = Duration::from_micros(30);
    let work = Duration::from_millis(2);
    pace.snapshot_received(zero + owner, [owner; 2]);
    pace.presented(zero, zero, Duration::ZERO, zero + owner + work);
    pace.request_animation(zero + owner + work);
    let due = pace.due().expect("owner request opportunity");
    let target = pace.refresh.expect("reserved refresh").1;
    // The delay is smaller than D-33's existing margin, yet exceeds the owner cost.
    let woke = due + Duration::from_micros(100);
    let published = woke + owner;
    assert!(published <= pace.build_deadline(woke));
    assert!(published + work <= target);
    assert_eq!(pace.lead(), owner + work + Duration::from_millis(1));
}

/// Publication queue time affects D-33 lead; the build reserve excludes only FIFO wait.
#[test]
fn owner_publication_wait_enters_lead_but_not_the_render_work_reserve() {
    let zero = Instant::now();
    let mut pace = Pacer::new(120_000, Duration::ZERO);
    pace.first = false;
    pace.phase = Some(zero);
    pace.snapshot_received(
        zero + Duration::from_millis(2),
        [Duration::from_millis(2); 2],
    );
    pace.presented(
        zero,
        zero + Duration::from_millis(4),
        Duration::from_millis(1),
        zero + Duration::from_millis(5),
    );
    // All five milliseconds enter the existing half-period-clamped snapshot lead.
    assert_eq!(pace.lead(), pace.period / 2);
    pace.request_animation(zero + Duration::from_millis(5));
    let target = pace.refresh.expect("target").1;
    let due = pace.due().expect("snapshot opportunity");
    // Two milliseconds of post-receipt CPU are reserved. The wake margin already
    // advanced the request time and must remain available after an OS wake.
    assert_eq!(pace.build_deadline(due), target - Duration::from_millis(2));
    let expired = target + Duration::from_millis(1);
    assert_eq!(pace.build_deadline(expired), expired);
    assert!(pace.take(expired));
    pace.request_animation(expired);
    assert!(pace.refresh.expect("following target").1 >= target + pace.period);
}

/// Unknown phase can wait for at most its supplied period minus actual measured work.
#[test]
fn unknown_phase_publication_wait_is_bounded_without_a_second_timeout() {
    let zero = Instant::now();
    let mut pace = Pacer::new(60_000, Duration::ZERO);
    assert_eq!(pace.build_deadline(zero), zero + pace.period);
    pace.snapshot_received(zero, [Duration::ZERO; 2]);
    pace.presented(zero, zero, Duration::ZERO, zero + Duration::from_millis(3));
    assert_eq!(
        pace.build_deadline(zero),
        zero + pace.period - Duration::from_millis(3)
    );
}

/// One late snapshot includes both updates before a single refresh.
#[test]
fn two_updates_two_ms_apart_make_one_late_snapshot() {
    let zero = Instant::now();
    let mut pace = Pacer::new(60_000, Duration::from_millis(8));
    pace.request(zero, false);
    assert!(pace.take(zero));
    pace.presented(
        zero,
        zero + Duration::from_millis(2),
        Duration::from_millis(2),
        zero + Duration::from_millis(3),
    );
    let shown = zero + Duration::from_millis(3);
    pace.request(shown, false);
    let due = pace.due().expect("first update scheduled");
    pace.request(shown + Duration::from_millis(2), false);
    assert_eq!(pace.due(), Some(due));
    assert!(!pace.take(due - Duration::from_nanos(1)));
    assert!(pace.take(due));
    assert!(pace.due().is_none());
}

/// A transient nvim showcmd marker is erased before the paced snapshot.
#[test]
fn nvim_showcmd_erase_precedes_the_only_snapshot() {
    let zero = Instant::now();
    let mut pace = Pacer::new(60_000, Duration::from_millis(8));
    let mut terminal = Terminal::new(80, 24, 0, 0);
    pace.request(zero, false);
    assert!(pace.take(zero));
    pace.presented(
        zero,
        zero + Duration::from_millis(2),
        Duration::from_millis(2),
        zero + Duration::from_millis(3),
    );
    // The first read is nvim's synchronized 30-byte showcmd update;
    // the second read erases the same two cells two milliseconds later.
    let first = zero + Duration::from_millis(5);
    terminal.advance(b"\x1b[?2026h\x1b[24;70H^D\x1b[?2026l");
    pace.request(first, false);
    assert!(!pace.take(first));
    assert_eq!(
        terminal.screen().row(23).expect("last row").cells[69].ch,
        u32::from('^')
    );
    terminal.advance(b"\x1b[?2026h\x1b[24;70H  \x1b[?2026l");
    pace.request(first + Duration::from_millis(2), false);
    assert!(pace.take(pace.due().expect("deadline")));
    // Only the late snapshot becomes visible; neither showcmd cell
    // contains its transient caret or letter at presentation time.
    let row = terminal.screen().row(23).expect("last row");
    assert_ne!(row.cells[69].ch, u32::from('^'));
    assert_ne!(row.cells[70].ch, u32::from('D'));
}

/// A flood keeps one presentation slot per predicted refresh.
#[test]
fn continuous_damage_never_doubles_a_refresh() {
    let zero = Instant::now();
    let mut pace = Pacer::new(60_000, Duration::from_millis(8));
    pace.request(zero, false);
    assert!(pace.take(zero));
    pace.presented(
        zero,
        zero + Duration::from_millis(2),
        Duration::from_millis(2),
        zero + Duration::from_millis(3),
    );
    let mut previous = zero;
    for step in 1..10 {
        let update = zero + Duration::from_millis(step * 3);
        pace.request(update, false);
        let due = pace.due().expect("continuous request");
        // Even without a real display, successive snapshots cannot be
        // scheduled inside one predicted half-period of each other.
        assert!(due.duration_since(previous) >= pace.period / 2);
        assert!(!pace.take(due - Duration::from_nanos(1)));
        assert!(pace.take(due));
        pace.presented(
            due,
            due + Duration::from_millis(2),
            Duration::from_millis(2),
            due + Duration::from_millis(3),
        );
        previous = due;
    }
}

/// First and drag frames are available without the normal delay.
#[test]
fn first_and_resize_frames_are_immediate() {
    let zero = Instant::now();
    let mut pace = Pacer::new(60_000, Duration::from_millis(8));
    pace.request(zero, false);
    assert!(pace.take(zero));
    pace.request(zero + Duration::from_millis(1), false);
    assert!(!pace.take(zero + Duration::from_millis(1)));
    pace.request(zero + Duration::from_millis(2), true);
    assert!(pace.take(zero + Duration::from_millis(2)));
}

/// Every display phase waits past nvim's measured 2–7 ms erase gap.
#[test]
fn settle_window_phase_sweep() {
    let zero = Instant::now();
    let mut twelve_ms_flashes = 0;
    for phase_half_ms in 0..34 {
        for gap_ms in [2, 5, 7, 12] {
            let mut pace = Pacer::new(60_000, Duration::from_millis(8));
            pace.origin = zero;
            pace.first = false;
            pace.phase = Some(zero + Duration::from_micros(phase_half_ms * 500));
            pace.last_presented = Some(zero);
            pace.settle.presented(Duration::ZERO);
            let mark = zero + Duration::from_millis(20);
            pace.request(mark, false);
            let due = pace.due().expect("mark schedules a snapshot");
            let erase = mark + Duration::from_millis(gap_ms);
            if gap_ms < 8 {
                assert!(due > erase, "phase={phase_half_ms} gap={gap_ms}");
            } else if due < erase {
                // A twelve-millisecond gap exceeds the chosen eight-ms
                // window, so some display phases legitimately show it.
                twelve_ms_flashes += 1;
            }
            pace.request(erase, false);
            assert_eq!(pace.due(), Some(due));
        }
    }
    assert!(twelve_ms_flashes > 0);
    assert!(twelve_ms_flashes < 34);
}

/// Idle calibration is ineligible while hidden and throttled after a sample.
#[test]
fn idle_double_present_teaches_phase_only_when_visible() {
    let zero = Instant::now();
    let mut pace = Pacer::new(60_000, Duration::from_millis(8));
    pace.origin = zero;
    pace.first = false;
    pace.last_presented = Some(zero);
    pace.last_output = Some(zero);
    let idle = zero + Duration::from_millis(250);
    assert_eq!(pace.calibration_due(idle, false), None);
    assert_eq!(pace.calibration_due(idle, true), Some(idle));
    pace.calibrated(idle, Some(idle + Duration::from_millis(16)));
    assert_eq!(pace.phase, Some(idle + Duration::from_millis(16)));
    assert_eq!(
        pace.calibration_due(idle, true),
        Some(idle + Duration::from_secs(5))
    );
}

/// A transient startup FIFO cannot suppress further learning for five seconds.
#[test]
fn no_sample_retries_at_next_idle_opportunity_while_hidden_stays_asleep() {
    let zero = Instant::now();
    let mut pace = Pacer::new(60_000, Duration::from_millis(8));
    pace.first = false;
    pace.last_presented = Some(zero);
    pace.last_output = Some(zero);
    let attempt = zero + Duration::from_millis(250);
    pace.calibrated(attempt, None);
    let retry = attempt + Duration::from_millis(250);
    assert_eq!(pace.calibration_due(attempt, true), Some(retry));
    assert_eq!(pace.calibration_due(retry, false), None);
    // Fresh output must still postpone the retry until that activity is idle.
    pace.request(retry - Duration::from_millis(10), false);
    let due = pace.due().expect("output frame");
    assert!(pace.take(due));
    pace.presented(due, due, Duration::ZERO, due);
    assert_eq!(
        pace.calibration_due(retry, true),
        Some(retry + Duration::from_millis(240))
    );
    let learned = retry + Duration::from_millis(240);
    pace.calibrated(learned, Some(learned));
    assert_eq!(
        pace.calibration_due(learned, true),
        Some(learned + Duration::from_secs(5))
    );
}

/// Sparse samples use their own p90 rank, not a 32-slot rank of zeroes.
#[test]
fn p90_uses_the_actual_sample_count() {
    for (count, expected) in [(1, 2), (10, 10), (28, 27), (32, 30)] {
        let mut pace = Pacer::new(1_000, Duration::from_millis(8));
        for ms in 1..=count {
            pace.costs.push_back(Duration::from_millis(ms));
        }
        assert_eq!(pace.lead(), Duration::from_millis(expected));
    }
}

/// A fresh output burst cannot postpone a 120-Hz animation refresh.
#[test]
fn animation_keeps_refresh_despite_output_settle() {
    let zero = Instant::now();
    let mut pace = Pacer::new(120_000, Duration::from_millis(8));
    pace.origin = zero;
    pace.first = false;
    pace.phase = Some(zero);
    pace.last_presented = Some(zero);
    let output = zero + Duration::from_millis(6);
    pace.request(output, false);
    let settled = pace.due().expect("settled output");
    pace.request_animation(output);
    let animated = pace.due().expect("animation refresh");
    assert!(animated < settled);
    assert!(animated < output + Duration::from_millis(8));
    pace.request(output + Duration::from_micros(500), false);
    assert_eq!(pace.due(), Some(animated));
    assert!(pace.take(animated));
    pace.presented(animated, animated, Duration::ZERO, animated);
    assert!(!pace.animation);
    assert!(pace.due().is_none());
}

/// A shrinking p90 lead must not reserve the same refresh a second time (D-33).
#[test]
fn cheaper_animation_frame_does_not_reuse_its_refresh() {
    let zero = Instant::now();
    let mut pace = Pacer::new(60_000, Duration::ZERO);
    pace.first = false;
    pace.phase = Some(zero);
    pace.costs.push_back(Duration::from_millis(4));
    pace.request_animation(zero);
    let first = pace.due().expect("first deadline");
    let first_index = pace.refresh.expect("first refresh").0;
    assert!(pace.take(first));
    // The old lead was 5 ms; a cheap frame now changes it to 1.5 ms.
    pace.costs.clear();
    pace.presented(
        first,
        first,
        Duration::ZERO,
        first + Duration::from_micros(500),
    );
    pace.request_animation(first + Duration::from_micros(600));
    let next_index = pace.refresh.expect("next refresh").0;
    assert!(
        next_index > first_index,
        "lead changes must not reuse a refresh index"
    );
}

/// Unknown phase rate-limits submission starts without adding a period after completion.
#[test]
fn unknown_phase_uses_request_time_and_fifo_without_completion_drift() {
    let zero = Instant::now();
    let mut pace = Pacer::new(60_000, Duration::ZERO);
    pace.first = false;
    pace.request_animation(zero);
    assert_eq!(pace.due(), Some(zero));
    assert!(pace.take(zero));
    // A late completion already consumed more than a period. The next request
    // may submit now; waiting another full period would drift below refresh.
    let late = zero + Duration::from_millis(25);
    pace.presented(zero, late, Duration::ZERO, late);
    pace.request_animation(late);
    assert_eq!(pace.due(), Some(late));
}

/// Both modes reserve a full period despite changing lead and late OS wakes.
#[test]
fn every_mode_reserves_a_full_period_between_targeted_refreshes() {
    let zero = Instant::now();
    for calibrated in [false, true] {
        let mut pace = Pacer::new(60_000, Duration::ZERO);
        pace.first = false;
        pace.phase = calibrated.then_some(zero);
        let mut now = zero;
        let mut previous_target = None;
        let mut previous_start = None;
        for cost in [1, 8, 2, 20, 1, 3] {
            pace.request_animation(now);
            let due = pace.due().expect("animation deadline");
            let (_, target) = pace.refresh.expect("reserved slot");
            if let Some(previous) = previous_target {
                assert!(target.duration_since(previous) >= pace.period);
            }
            // An OS wake may be late. Unknown phase must rate-limit the actual
            // submission start, not an earlier predicted deadline.
            let start = due + Duration::from_millis(cost);
            assert!(pace.take(start));
            if !calibrated && let Some(previous) = previous_start {
                assert!(start.duration_since(previous) >= pace.period);
            }
            let end = start + Duration::from_millis(cost);
            pace.presented(start, start, Duration::ZERO, end);
            previous_target = Some(target);
            previous_start = Some(start);
            now = end;
        }
    }
}

/// Acquisition return jitter must not move the active clock backwards and skip a turn.
#[test]
fn early_acquisition_sample_keeps_the_next_refresh_adjacent() {
    let zero = Instant::now();
    let mut pace = Pacer::new(60_000, Duration::ZERO);
    pace.first = false;
    pace.phase = Some(zero);
    pace.costs.push_back(Duration::from_millis(3));
    pace.request_animation(zero);
    let due = pace.due().expect("first deadline");
    let (index, target) = pace.refresh.expect("first target");
    assert!(pace.take(due));
    // The recorded stalled runs returned 0.275 ms before their predicted edge.
    // Treating that noisy sample as a new grid made the next edge too early for
    // the full-period invariant, so the old scheduler skipped an entire refresh.
    let acquired = target - Duration::from_micros(275);
    let end = acquired + Duration::from_micros(450);
    pace.presented(due, acquired, Duration::from_millis(2), end);
    pace.request_animation(end);
    let (next_index, next_target) = pace.refresh.expect("next target");
    assert_eq!(next_index, index + 1);
    assert_eq!(next_target, target + pace.period);
}

/// Complete glyph preparation participates in D-33's measured snapshot lead.
#[test]
fn synchronous_glyph_work_contributes_to_actual_frame_lead() {
    let zero = Instant::now();
    let mut pace = Pacer::new(60_000, Duration::ZERO);
    pace.presented(zero, zero, Duration::ZERO, zero + Duration::from_millis(6));
    assert_eq!(pace.lead(), Duration::from_millis(7));
}

/// A late wake keeps its reservation; complete ink can finish after the targeted edge.
#[test]
fn late_wake_does_not_retarget_a_complete_frame() {
    let zero = Instant::now();
    let mut pace = Pacer::new(60_000, Duration::ZERO);
    pace.first = false;
    pace.phase = Some(zero);
    pace.request_animation(zero);
    let (index, target) = pace.refresh.expect("reserved refresh");
    assert!(pace.take(target + Duration::from_millis(5)));
    assert_eq!(pace.last_refresh_index, Some(index));
    assert_eq!(pace.last_refresh_target, Some(target));
    pace.request_animation(target + Duration::from_millis(25));
    assert!(pace.refresh.expect("next refresh").0 > index);
}

/// Host event frequency must not turn a continuous flood into isolated bursts.
#[test]
fn coalesced_writes_preserve_flood_and_isolated_key_settling() {
    use crate::render_thread::surface::DamageArrival;
    let mut pace = Pacer::new(120_000, Duration::from_millis(8));
    let zero = pace.origin;
    pace.request_output(
        DamageArrival {
            first: zero,
            previous: None,
        },
        zero,
    );
    assert!(pace.take(zero));
    pace.presented(zero, zero, Duration::ZERO, zero);
    // Wakes are over a refresh apart, but the actual last suppressed write is
    // only 100 us before the next write: output has never gone quiet.
    let first = zero + Duration::from_millis(20);
    pace.request_output(
        DamageArrival {
            first,
            previous: Some(first - Duration::from_micros(100)),
        },
        first,
    );
    assert_eq!(pace.settle.earliest(), Some(Duration::from_millis(20)));
    let due = pace.due().expect("coalesced frame");
    assert!(pace.take(due));
    pace.presented(due, due, Duration::ZERO, due);
    // After real silence an editor may draw ^D and erase it 2 ms later. Its
    // first write still gets all 8 ms even if the UI event arrives 3 ms late.
    let key = zero + Duration::from_millis(100);
    pace.request_output(
        DamageArrival {
            first: key,
            previous: Some(first),
        },
        key + Duration::from_millis(3),
    );
    assert_eq!(pace.settle.earliest(), Some(Duration::from_millis(108)));
    assert!(pace.due().expect("isolated frame") >= key + Duration::from_millis(8));
}
