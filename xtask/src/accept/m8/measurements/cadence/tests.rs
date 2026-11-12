//! Acceptance cannot turn a sparse or malformed recording into 120-Hz evidence.
#![allow(clippy::unwrap_used, reason = "test controls have known array shapes")]
use super::*;
use serde_json::json;

/// A full two-second, 120-Hz sequence defines the gate's positive control.
fn full() -> Value {
    json!({"elapsed_ms":2000.0, "submission_ms":(0..=240).map(|n| f64::from(n) * 1000.0 / 120.0).collect::<Vec<_>>()})
}

/// Checking only the median would conceal one complete missed display refresh.
#[test]
fn full_recording_and_single_missing_refresh() {
    let mut value = full();
    assert!(submissions(&value, 120.0).is_ok());
    value["submission_ms"].as_array_mut().unwrap().remove(120);
    assert!(submissions(&value, 120.0).is_err());
}

/// Short, absent and regularly slow sequences cannot borrow a PASS boolean.
#[test]
fn shortened_empty_slow_and_invalid_times() {
    let mut value = full();
    value["pass"] = json!(true);
    assert!(submissions(&value, 60.0).is_err());
    value["elapsed_ms"] = json!(1000.0);
    assert!(submissions(&value, 120.0).is_err());
    value = full();
    value["submission_ms"] = json!([]);
    assert!(submissions(&value, 120.0).is_err());
    value = full();
    value["submission_ms"][100] = value["submission_ms"][99].clone();
    assert!(submissions(&value, 120.0).is_err());
    value["submission_ms"][100] = json!(null);
    assert!(submissions(&value, 120.0).is_err());
}

/// Median-sized interior gaps must not conceal missing submissions at either edge.
#[test]
fn both_edges_and_full_sample_count() {
    let mut value = full();
    value["submission_ms"].as_array_mut().unwrap().drain(0..3);
    assert!(submissions(&value, 120.0).is_err());
    value = full();
    value["submission_ms"].as_array_mut().unwrap().truncate(237);
    assert!(submissions(&value, 120.0).is_err());
    // A regular 10-ms clock passes the individual-gap allowance but misses 39 slots.
    value = json!({"elapsed_ms":2000.0, "submission_ms":(0..=200).map(|n| f64::from(n)*10.0).collect::<Vec<_>>()});
    assert!(submissions(&value, 120.0).is_err());
}

/// A clean last repeat must never conceal an earlier missing refresh (D-36).
#[test]
fn every_repeat_contributes_to_misses_and_normal_failure() {
    let mut earlier = full();
    earlier["submission_ms"].as_array_mut().unwrap().remove(120);
    let summary = runs::Summary {
        runs: vec![
            (
                "scroll-0.json".into(),
                analysis::evaluate(&earlier, 120.0, 120.0).unwrap(),
            ),
            (
                "scroll-1.json".into(),
                analysis::evaluate(&full(), 120.0, 120.0).unwrap(),
            ),
            (
                "scroll-2.json".into(),
                analysis::evaluate(&full(), 120.0, 120.0).unwrap(),
            ),
        ],
    };
    let description = summary.describe();
    assert!(description.contains("runs=3"));
    assert!(description.contains("expected_refreshes=720"));
    assert!(description.contains("missed_refreshes=1"));
    assert!(description.contains("missed_rate_pct=0.138889"));
    assert!(description.contains("failed_runs=1"));
    assert!(
        summary
            .require_clean()
            .unwrap_err()
            .contains("scroll-0.json")
    );
}

/// A three-period stall misses two refreshes, and is retained despite deferral.
#[test]
fn multi_refresh_gap_and_edges_remain_distinct() {
    let mut value = full();
    value["submission_ms"]
        .as_array_mut()
        .unwrap()
        .drain(120..122);
    let run = analysis::evaluate(&value, 120.0, 120.0).unwrap();
    assert_eq!(run.missed, 2);
    assert!(!run.edge_failure);
    assert!(run.require_clean().is_err());
    value = full();
    value["submission_ms"].as_array_mut().unwrap().truncate(237);
    let run = analysis::evaluate(&value, 120.0, 120.0).unwrap();
    assert_eq!(run.missed, 0);
    assert!(run.edge_failure);
    assert!(run.require_clean().is_err());
}

/// LG's 60-Hz check scales the same period rule; it cannot certify 120 Hz.
#[test]
fn sixty_hz_bounds_and_independent_clock() {
    let mut value = json!({"elapsed_ms":2000.0, "submission_ms":(0..=120).map(|n| f64::from(n)*1000.0/60.0).collect::<Vec<_>>()});
    assert!(
        analysis::evaluate(&value, 60.0, 60.0)
            .unwrap()
            .require_clean()
            .is_ok()
    );
    assert!(
        analysis::evaluate(&value, 60.0, 120.0)
            .unwrap()
            .require_clean()
            .is_err()
    );
    value["submission_ms"].as_array_mut().unwrap().remove(60);
    let run = analysis::evaluate(&value, 60.0, 60.0).unwrap();
    assert_eq!(run.missed, 1);
    assert!(run.require_clean().is_err());
    assert!(analysis::evaluate(&full(), f64::NAN, 120.0).is_err());
    // Chrome's rate is independently measured; a duplicate tick is malformed.
    let mut browser = json!({"userAgent":"Chrome/154", "m8":{"scroll":{"raf_ms":[0.0,8.3,16.6]}}});
    assert!(chrome_rate(&browser).unwrap() > 120.0);
    browser["m8"]["scroll"]["raf_ms"][2] = json!(8.3);
    assert!(chrome_rate(&browser).is_err());
}
