//! Startup injection acceptance separates environment policy from real-shell behavior.
//!
//! The same puzzle-embed constructor serves the harness and the C AppKit host.
//! Environment tests protect detection, opt-out and exact restoration of values.
//! Real PTYs prove semantic boundaries and cwd without any user startup sourcing.
//! Existing manual snippets must coexist without duplicated command boundaries.
//! Startup fixtures also prove that prompt frameworks and vendor files still run.
//! No absolute timing is inferred from these functional shell tests.
//! The full M18.9 gate retains unchanged optimized performance criteria.
use super::report;

/// Reports each shell independently so one failure does not hide later results.
pub(super) fn run(passed: &mut bool) {
    for (criterion, package, filter) in [
        // Pure construction covers unknown shells and explicit wrapper choices
        // without relying on the developer's login shell or environment.
        (
            "M18.8 executable detection, opt-out and environment restoration",
            "puzzle-embed",
            "pty::integration::tests::",
        ),
        (
            "M18.8 bash ENV policy and Apple login/manual exception",
            "puzzle-embed",
            "integration_bash_real_login",
        ),
        // ZDOTDIR restoration must occur before later startup files, and hooks
        // must attach after frameworks without duplicating manual boundaries.
        (
            "M18.8 zsh automatic login, startup and manual coexistence",
            "puzzle-embed",
            "integration_zsh_real_login",
        ),
        (
            "M18.8 fish automatic login, vendor startup and manual coexistence",
            "puzzle-embed",
            "integration_fish_real_login",
        ),
        // This separate witness inspects true login state and unchanged ENV;
        // merely seeing marks from a manually sourced snippet cannot prove it.
        (
            "M18.8 unsupported Apple bash retains true login, ENV and startup",
            "puzzle-embed",
            "integration_apple_bash_preserves_login_startup",
        ),
        // Empty homes establish automatic behavior independently of any user's
        // existing rc file. Apple bash's documented exception is tested above.
        (
            "M18.8 unconfigured login shells emit OSC 133 and OSC 7",
            "puzzle-embed",
            "integration_unconfigured_shells_real_login",
        ),
        // Host startup reads the resolved setting once. Live writes must report
        // next-session application rather than mutating an already-running shell.
        (
            "M18.8 schema validates choices and next-session application",
            "puzzle-term",
            "shell_integration_setting",
        ),
    ] {
        // Behavioral selectors include real parsing of marks and exact command outputs.
        report(criterion, checked_tests(package, filter), passed);
    }
    println!(
        "REVIEW M18.8 fresh shells in app and harness, prompt frameworks: docs/shell-integration.md; docs/protocol-review.md; pending M18.9"
    );
}

/// Rejects a renamed filter that Cargo would otherwise treat as zero-test success.
fn checked_tests(package: &str, filter: &str) -> Result<(), String> {
    // capture preserves failure diagnostics and rejects a failing subprocess
    // before its successful-summary text can be considered by this parser.
    let output = crate::commands::capture("cargo", &["test", "-p", package, "--lib", filter])?;
    // These selectors intentionally cover multiple fixtures. Require a nonempty
    // passing test summary rather than hard-coding their evolving test count.
    let executed = output.lines().any(|line| {
        line.strip_prefix("test result: ok. ")
            .and_then(|summary| summary.split_whitespace().next())
            .and_then(|count| count.parse::<usize>().ok())
            .is_some_and(|count| count > 0)
    });
    if !executed {
        return Err(format!(
            "{package} shell selector {filter} executed no tests"
        ));
    }
    Ok(())
}
