//! Real-shell acceptance uses the native PTY seam and the shipped snippets.
//!
//! These tests compare parsed semantic ranges, not a substring of emitted OSC.
//! Success, failure and multiline output also preserve a framework-style prompt.
//! Sourcing twice must not duplicate boundaries. Fish's own mark-prompt feature
//! and the feature-disabled fallback are separate obligations.
use super::super::{m7::test, report};

/// Every supported shell must execute its exact witness, including bash 3.2.
pub(super) fn run(passed: &mut bool) {
    for name in [
        "bash_snippet_real_pty_zones",
        "zsh_snippet_real_pty_zones",
        "fish_snippet_real_pty_zones",
        "fish_legacy_snippet_real_pty_zones",
    ] {
        // Exact matching rejects a renamed or missing test instead of accepting zero tests.
        report(
            &format!("M9.6 {name}"),
            test(
                "puzzle-embed",
                &format!("pty::tests::shell_integration::{name}"),
            ),
            passed,
        );
    }
    // Framework packages are not installed by this gate; public hook seams are tested.
    // Human checks use the user's actual prompt setup and the documented source order.
    println!(
        "REVIEW M9.6 actual prompt-framework compatibility: pending; docs/shell-integration.md"
    );
}
