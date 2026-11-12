//! Failed receipt evidence must survive another probe and a closed input timer.
use super::*;

/// Two windows can share the evidence root without deleting either run's files.
#[test]
fn independent_probes_preserve_failed_six_key_history() -> Result<(), String> {
    let root = std::env::temp_dir().join("puzzle-receipt-tests");
    let first = Receipts::new(&root)?;
    let second = Receipts::new(&root)?;
    assert_ne!(first.path, second.path);
    first.save_issued(&[1; 6])?;
    let issued = fs::read(first.path.with_extension("issued.json")).map_err(|e| e.to_string())?;
    let received = [2, 3, 4, 5, 6];
    let bytes = serde_json::to_vec(&received).map_err(|e| e.to_string())?;
    fs::write(first.path.with_extension("txt.samples"), &bytes).map_err(|e| e.to_string())?;
    fs::write(&second.path, b"second child").map_err(|e| e.to_string())?;
    let error = first
        .pair(&[1; 6], &received, 6)
        .err()
        .ok_or("five receipts passed an exact-six gate")?;
    assert!(error.contains("expected 6, sent 6, received 5"));
    drop(second);
    assert_eq!(
        fs::read(first.path.with_extension("issued.json")).map_err(|e| e.to_string())?,
        issued
    );
    assert_eq!(
        fs::read(first.path.with_extension("txt.samples")).map_err(|e| e.to_string())?,
        bytes
    );
    assert_eq!(
        first.pair(&[1, 2, 3, 4, 5, 6], &[2, 3, 4, 5, 6, 7], 6)?,
        vec![0.000001; 6]
    );
    assert!(first.pair(&[3], &[2], 1).is_err());
    Ok(())
}

/// Cancellation uses channel state, so no sleeps or scheduler timing are required.
#[test]
fn closed_probe_cancels_every_remaining_key() -> Result<(), String> {
    let mut receipts = Receipts::new(&std::env::temp_dir().join("puzzle-receipt-tests"))?;
    let timer = receipts.timer();
    assert!(wait(&timer, Duration::ZERO));
    drop(receipts);
    assert!(!wait(&timer, Duration::ZERO));
    Ok(())
}
