//! Finite invocation and isolated dependency decisions stay mechanically checked.
use super::*;
#[test]
fn durations_are_positive_finite_and_checked() {
    for (text, expected) in [("60", 60), ("3s", 3), ("2m", 120), ("1h", 3600)] {
        assert_eq!(seconds(text), Ok(expected));
    }
    for text in ["", "0", "0h", "-1", "+1", "1ms", "18446744073709551615h"] {
        assert!(seconds(text).is_err());
    }
}

#[test]
fn invocation_pins_sanitizer_and_workspace_paths_without_launching_engine() {
    let root = Path::new("/workspace");
    let command = invocation(
        root,
        "text_sizing",
        60,
        &root.join("target/fuzz/corpus/text_sizing"),
        &root.join("target/fuzz/artifacts/text_sizing"),
    );
    let args: Vec<_> = command
        .get_args()
        .map(|arg| arg.to_string_lossy())
        .collect();
    assert_eq!(&args[..4], &["run", NIGHTLY, "cargo", "fuzz"]);
    assert!(args.iter().any(|arg| arg == "--debug-assertions"));
    assert!(args.iter().any(|arg| arg == "address"));
    assert!(args.iter().any(|arg| arg == "-max_total_time=60"));
    assert!(args.iter().any(|arg| arg == "-timeout=60"));
    assert!(args.iter().any(|arg| arg == "/workspace/target/fuzz/build"));
    assert!(
        args.iter()
            .any(|arg| arg == "-artifact_prefix=/workspace/target/fuzz/artifacts/text_sizing/")
    );
}
