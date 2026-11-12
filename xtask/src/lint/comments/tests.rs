//! Regression cases for the design §2.4 comment quality gate.
//!
//! The two failure modes would otherwise let the workspace pass R1 while
//! repeating explanations or counting URL text as a comment.

use super::{QualityGuard, scan_line};
use std::path::Path;

/// A slash pair in a string does not start a Rust comment.
#[test]
fn url_does_not_raise_comment_density() {
    let (code, comment) = scan_line("let url = \"https://example.com\";");
    assert!(code.contains("let url"));
    assert!(comment.is_none());
}

/// The fourth copy of an explanation fails even across different paths.
#[test]
fn repeated_explanation_fails_workspace_gate() {
    let mut guard = QualityGuard::default();
    for index in 0..3 {
        let path = format!("crate_{index}.rs");
        assert!(
            guard
                .observe(
                    Path::new(&path),
                    "Clear after resize because the surface changed."
                )
                .is_ok()
        );
    }
    assert!(
        guard
            .observe(
                Path::new("fourth.rs"),
                "clear after  resize because the surface changed."
            )
            .is_err()
    );
}

/// Generic prose fails before it can contribute to the repetition count.
#[test]
fn filler_phrase_fails_immediately() {
    let mut guard = QualityGuard::default();
    assert!(
        guard
            .observe(
                Path::new("source.rs"),
                "Keep this operation explicit for M0."
            )
            .is_err()
    );
}
