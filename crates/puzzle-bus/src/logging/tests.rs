//! Filter strings are user input and must fail visibly when malformed.
use super::parse_filter;

/// A default and two target overrides use the spec's syntax.
#[test]
fn parses_target_overrides() {
    assert!(parse_filter("warn,puzzle_vt=debug,puzzle_render=info").is_ok());
}

/// Empty text is not silently interpreted as a permissive level.
#[test]
fn rejects_empty_filter() {
    assert!(parse_filter(" ").is_err());
}
