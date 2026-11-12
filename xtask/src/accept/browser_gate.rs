//! D-46 schedules Chrome only at desktop milestone closure, with explicit task skips.
//!
//! The default executes every browser criterion. Opting out never emits PASS for
//! an unexecuted witness: each original label prints SKIP with the exact flag.
//! Wasm compilation and Clippy remain in commands::check regardless of this gate.
//! Only milestones with a desktop task inventory currently accept the opt-out.
//! Unknown arguments fail before expensive checks or a native window can start.
//! Adding a milestone requires passing this value through its browser criteria.
/// Browser execution policy selected once by the acceptance CLI.
#[derive(Clone, Copy)]
pub(super) enum BrowserGate {
    /// Default milestone acceptance executes all Chrome and real-bridge witnesses.
    Full,
    /// Explicit desktop task invocation defers each browser criterion to closure.
    TaskSkip,
}

/// The gate wraps only browser work; native tests and wasm checks never see it.
impl BrowserGate {
    /// Runs the expensive shared suite once, or leaves its dependent labels skipped.
    pub(super) fn suite(self) -> Option<Result<String, String>> {
        // None means deliberately unexecuted, never an empty successful suite receipt.
        match self {
            Self::Full => Some(crate::browser::suite()),
            Self::TaskSkip => None,
        }
    }

    /// Defers the closure itself, so skipped criteria cannot launch browser children.
    pub(super) fn report(
        self,
        name: &str,
        result: impl FnOnce() -> Result<(), String>,
        passed: &mut bool,
    ) {
        // Accept the computation as a closure so policy is decided before child startup.
        match self {
            Self::Full => super::report(name, result(), passed),
            Self::TaskSkip => println!(
                "SKIP {name}: --skip-browser (D-46 task run; required at final acceptance)"
            ),
        }
    }

    /// Shares one suite receipt while preserving each dependent criterion's original name.
    /// A failed suite fails every dependent witness; skipped suites never evaluate it.
    pub(super) fn report_suite(
        self,
        name: &str,
        output: &Option<Result<String, String>>,
        validate: impl FnOnce(&str) -> Result<(), String>,
        passed: &mut bool,
    ) {
        // Retain the shared failure for each label rather than testing partial stdout.
        self.report(
            name,
            || {
                // A missing receipt is an error if a full gate ever reaches this closure.
                output
                    .as_ref()
                    .ok_or("browser suite skipped")?
                    .as_ref()
                    .map_err(Clone::clone)
                    .and_then(|text| validate(text))
            },
            passed,
        );
    }
}

/// Parses the milestone and explicit scheduling flag, rejecting silent ignored options.
pub(super) fn parse(args: &[String]) -> Result<(Option<&str>, BrowserGate), String> {
    // The first positional argument remains the milestone; options cannot hide its absence.
    let milestone = args.first().map(String::as_str);
    // Only one exact flag is accepted, so typos cannot silently select a partial run.
    let browser = match &args[usize::from(!args.is_empty())..] {
        [] => BrowserGate::Full,
        [flag] if flag == "--skip-browser" => BrowserGate::TaskSkip,
        _ => return Err("usage: cargo xtask accept M<n> [--skip-browser]".into()),
    };
    // Earlier web milestones must not accidentally be certified with desktop scheduling.
    if matches!(browser, BrowserGate::TaskSkip) && !matches!(milestone, Some("M9" | "M11" | "M16"))
    {
        return Err("--skip-browser is supported for desktop tasks M9, M11 and M16".into());
    }
    // Borrow argv only for dispatch; no ambient environment can change this policy.
    Ok((milestone, browser))
}

#[cfg(test)]
/// CLI mistakes and accidental eager evaluation could silently bypass final gates.
mod tests;
