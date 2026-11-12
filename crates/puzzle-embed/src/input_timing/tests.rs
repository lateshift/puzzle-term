//! Host clock validation and exact owner/channel stage boundaries, without budgets.
use super::*;

#[test]
fn host_epoch_is_arbitrary_but_invalid_pairs_never_become_fast_samples()
-> Result<(), crate::ApiError> {
    let timing = InputTiming::new(10_000.0, 10_000.125)?;
    assert_eq!(timing.residence, Some(Duration::from_millis(125)));
    for (event, now) in [
        (-1.0, 0.0),
        (2.0, 1.0),
        (f64::NAN, 1.0),
        (0.0, f64::NAN),
        (0.0, f64::INFINITY),
        (f64::INFINITY, f64::INFINITY),
    ] {
        assert!(InputTiming::new(event, now).is_err());
    }
    Ok(())
}

/// The harness's release-logs feature retains DEBUG but compiles out TRACE.
#[test]
fn diagnostic_capture_works_with_debug_only_filter() {
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(std::io::sink)
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        assert!(
            InputTiming::capture_api().is_some(),
            "input diagnostic must not require TRACE"
        );
    });
}
