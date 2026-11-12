//! Missing or inconsistent D-50 observations cannot become an acceptance pass.
use super::*;

/// Late answers remain valid evidence; their lateness is reported without a new floor.
#[test]
fn completed_pairs_are_required_even_for_late_answers() {
    let mut row = serde_json::json!({
        "request_to_publish_wall_ms": 3.0,
        "control_wait_wall_ms": 2.0,
        "applied_to_publish_wall_ms": 1.0,
        "applied_to_publish_thread_cpu_ms": 0.1,
        "published_after_build_deadline": true,
    });
    assert_eq!(observation(&row), Ok((3.0, Some(0.1), true)));
    row["control_wait_wall_ms"] = 4.0.into();
    assert!(observation(&row).is_err());
    assert!(observation(&serde_json::json!({})).is_err());
    assert!(summarize(&[]).is_err());
}
