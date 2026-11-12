//! Cohort discovery must not discard earlier, slow or malformed scroll records.
#![allow(
    clippy::unwrap_used,
    reason = "test fixture paths and filesystem setup must succeed"
)]
use super::*;

/// Filename discovery includes every run, regardless of contents or sort position.
#[test]
fn discovers_all_repeats_and_rejects_missing_cohorts() {
    let root = std::env::temp_dir().join(format!("puzzle-m8-cohort-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    for name in [
        "scroll-0.json",
        "scroll-9.json",
        "scroll-10.json",
        "report.json",
        "preflight.json",
        "scroll-1.log",
    ] {
        // Malformed content must reach validation, not disappear during discovery.
        fs::write(root.join(name), "malformed retained evidence").unwrap();
    }
    let names: Vec<_> = paths(root.to_str().unwrap(), 3)
        .unwrap()
        .into_iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["scroll-0.json", "scroll-10.json", "scroll-9.json"]);
    assert!(paths(root.to_str().unwrap(), 4).is_err());
    fs::remove_dir_all(&root).unwrap();
    assert!(paths(root.to_str().unwrap(), 1).is_err());
}
