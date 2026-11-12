//! Real-window regression for cached glyph uploads during startup occlusion.
//!
//! The probe's existing alphabet readback uses the live atlas without repairing
//! uploads. This opt-in test needs a visible macOS desktop and runs alone: it
//! owns each window and closes it before checking the next setup combination.
#![cfg(feature = "lock-metrics")]
#![allow(
    clippy::unwrap_used,
    reason = "test subprocess and JSON shape are assertion boundaries"
)]
use std::{fs, path::Path, process::Command};

/// Every alphabet cell must retain ink regardless of the three startup controls.
#[test]
#[ignore = "requires a visible native GPU window; run separately from timing recordings"]
fn startup_alphabet_preserves_every_cached_glyph() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let report = root.join("target/m8-glyph-bisect.json");
    let config = root.join("target/m8-glyph-empty-config.json");
    fs::write(&config, "{}\n").unwrap();
    for placement in [false, true] {
        for empty in [false, true] {
            for history in [false, true] {
                // A stale previous result must never make a failed process pass.
                let _ = fs::remove_file(&report);
                let mut command = Command::new(env!("CARGO_BIN_EXE_puzzle-desktop"));
                command
                    .current_dir(&root)
                    .arg("run")
                    .env("SHELL", "/bin/sh")
                    .env("PUZZLE_SCROLL_PROBE", "1")
                    .env("PUZZLE_GLYPH_BISECT", "1")
                    .env("PUZZLE_GLYPH_HISTORY", if history { "1" } else { "0" })
                    .env("PUZZLE_LOG", "info,puzzle_desktop::glyphs=trace");
                if placement {
                    command.env("PUZZLE_SCROLL_PROBE_MILLIHZ", "60000");
                } else {
                    command.env_remove("PUZZLE_SCROLL_PROBE_MILLIHZ");
                }
                if empty {
                    command.arg("--config").arg(&config);
                }
                let output = command.output().unwrap();
                let label = format!("placement-{placement}-empty-{empty}-history-{history}");
                fs::write(
                    root.join(format!("target/m8-startup-glyph-{label}.log")),
                    &output.stdout,
                )
                .unwrap();
                assert!(output.status.success(), "{label}: {:?}", output.status);
                let value: serde_json::Value =
                    serde_json::from_str(&fs::read_to_string(&report).unwrap()).unwrap();
                // Counting each written cell catches partial glyph loss; aggregate
                // nonblank screenshots could pass with only capitals remaining.
                let cells = value["cells"].as_array().unwrap();
                assert!(cells.len() >= 58, "{label}: missing authored alphabet");
                assert!(
                    value["missing"].as_array().unwrap().is_empty(),
                    "{label}: {}",
                    value["missing"]
                );
            }
        }
    }
}
