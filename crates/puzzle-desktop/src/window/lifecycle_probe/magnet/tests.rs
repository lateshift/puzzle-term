//! User-authored Magnet sequences protect fractions, restore and cross-display geometry.
use super::*;

#[test]
fn exact_grid_and_restore_preserve_runtime_workarea() -> Result<(), Box<dyn std::error::Error>> {
    let original = [100.0, 200.0, 700.0, 500.0];
    // Nonzero/negative origins catch accidental use of NSScreen.frame or hardcoded menu heights.
    let visible = [1728.0, -323.0, 2560.0, 1407.0];
    assert_eq!(
        resolve("left", original, visible, original)?,
        [1728.0, -323.0, 1280.0, 1407.0]
    );
    assert_eq!(
        resolve("right", original, visible, original)?,
        [3008.0, -323.0, 1280.0, 1407.0]
    );
    assert_eq!(resolve("maximize", original, visible, original)?, visible);
    let boss = resolve("boss", original, visible, original)?;
    assert!((boss[0] - 1941.3333333333333).abs() < 1e-9);
    assert!((boss[2] - 2133.3333333333335).abs() < 1e-9);
    assert_eq!(resolve("restore", boss, visible, original)?, original);
    assert_eq!(
        resolve("center", original, visible, original)?,
        [2658.0, 130.5, 700.0, 500.0]
    );
    Ok(())
}

#[test]
fn all_user_sequences_replay_both_directions_and_move_variants()
-> Result<(), Box<dyn std::error::Error>> {
    let sequences: Vec<Vec<String>> = serde_json::from_str(include_str!("sequences.json"))?;
    assert_eq!(
        sequences.iter().map(Vec::len).collect::<Vec<_>>(),
        [5, 5, 6, 11]
    );
    for initial in 0..2 {
        for same_size in [true, false] {
            for sequence in &sequences {
                let screens = [
                    [0.0, 33.0, 1728.0, 1084.0],
                    [1728.0, -323.0, 2560.0, 1407.0],
                ];
                let original = [
                    screens[initial][0] + 100.0,
                    screens[initial][1] + 100.0,
                    700.0,
                    500.0,
                ];
                let mut current = original;
                let mut screen = initial;
                for command in sequence {
                    if matches!(command.as_str(), "next" | "previous") {
                        let source = screens[screen];
                        screen = 1 - screen;
                        if same_size {
                            let old_size = [current[2], current[3]];
                            current = resolve("next-size", current, screens[screen], original)?;
                            assert_eq!([current[2], current[3]], old_size);
                        } else {
                            // Cross-display relative mapping must be reversible for every size.
                            let before = current;
                            let target = screens[screen];
                            current = relative(before, source, target);
                            assert!((current[2] / target[2] - before[2] / source[2]).abs() < 1e-12);
                            let back = relative(current, target, source);
                            for (actual, expected) in back.iter().zip(before) {
                                assert!((actual - expected).abs() < 1e-9);
                            }
                        }
                    } else {
                        current = resolve(command, current, screens[screen], original)?;
                    }
                    assert!(current.iter().all(|n| n.is_finite()));
                    assert!(current[2] > 0.0 && current[3] > 0.0);
                }
            }
        }
    }
    Ok(())
}
