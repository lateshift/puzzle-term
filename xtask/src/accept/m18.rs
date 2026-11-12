//! M18 work-order acceptance, with origin organization preceding new behaviour.
//! Targeted tasks cannot certify the milestone; full acceptance additionally
//! runs the unchanged earlier budgets and the complete serial Chrome gates.
//!
//! D-58 has three separate obligations. The source inventory protects ownership:
//! interpreting an extension must not pull shared models into one origin family.
//! Existing tests protect wire semantics and retained state across the moves.
//! Existing goldens protect the renderer even though no rendering code changes.
//! All three are reported independently, preserving a failure from any stage.
//!
//! Origin names describe the defining project, not our implementation reference.
//! Kitty owns keyboard and graphics grammars, FinalTerm owns prompt boundaries,
//! iTerm2 owns plain OSC 9, ConEmu owns its progress subtype and rxvt owns OSC 777.
//! Contour defines the appearance/Unicode modes and synchronized-output CSI form;
//! Rockorager defines in-band resize. VTE defines hyperlink OSC 8. Apple's
//! installed shell script is the recorded primary evidence for directory OSC 7.
//! Rex remains an optional crate; the Session's OSC seam stays generic.
//!
//! Common ECMA-48, DEC and xterm families deliberately keep their old routes.
//! Their documentation is checked alongside extension pages so the index also
//! covers the base terminal, rather than presenting only the newly moved files.
//! The architecture test does not infer behavior from folder names: compiled
//! VT/Session tests must still observe the same replies, cells and host facts.
//!
//! No reference is blessed by this gate. Pixel tolerances remain those of §2.7.
//! A reference mismatch cannot be hidden by a successful protocol parser test.
//! Review of comments and final user-visible protocol behavior stays human.
//! Task selectors run independent criteria in work-order order. Full acceptance
//! includes every protocol task, previous performance contracts and Chrome.
//! It continues after failures without retrying or substituting old measurements.
//! Human review remains distinct from passing all mechanical lines.
//! Clipboard acceptance independently checks wire, Session and native C boundaries.
//! Human host behavior is printed as review work even when mechanical lines pass.
/// MIME clipboard state, host completions and single-use capabilities.
mod clipboard;
/// Complete unchanged native budgets and serial browser closure gates.
mod closure;
/// Unified colour-table conformance and GPU selection witnesses.
mod colours;
/// UAPI active contexts remain independent of screen/reset lifetime.
mod context;
/// Native drag gestures and MIME negotiation through synthetic and C hosts.
mod dnd;
/// Notifications share policy across origins and asynchronous native completion.
mod notifications;
/// Native startup injection and the explicit Apple bash exception.
mod shell;
/// Shared Shift ownership and backward-compatible settings.
mod shift;
/// Native swapchain format, drawable bytes and compositor colour contract.
mod surface_colour;
/// Rectangular grid text, sealed glyph geometry and unused-feature guarantees.
mod text_sizing;
use super::report;
use std::fs;

/// Selects task receipts or the complete milestone, with no browser skip in full mode.
pub(super) fn run(args: &[String]) -> bool {
    let mut passed = true;
    match args {
        [flag, task] if flag == "--task" && task == "origins" => origins(&mut passed),
        [flag, task] if flag == "--task" && task == "colours" => colours::run(&mut passed),
        [flag, task] if flag == "--task" && task == "shift" => shift::run(&mut passed),
        [flag, task] if flag == "--task" && task == "clipboard" => clipboard::run(&mut passed),
        [flag, task] if flag == "--task" && task == "notifications" => notifications::run(&mut passed),
        [flag, task] if flag == "--task" && task == "dnd" => dnd::run(&mut passed),
        [flag, task] if flag == "--task" && task == "context" => context::run(&mut passed),
        [flag, task] if flag == "--task" && task == "text-sizing" => text_sizing::run(&mut passed),
        [flag, task] if flag == "--task" && task == "shell" => shell::run(&mut passed),
        // The urgent native colour repair is independently reviewable before clipboard work.
        [flag, task] if flag == "--task" && task == "surface-colour" => surface_colour::run(&mut passed),
        // Reviewer-authorized environment follow-ups preserve the full gate unchanged.
        [flag, task] if flag == "--task" && task == "sanitizers" => {
            clipboard::sanitizers(&mut passed);
            notifications::sanitizers(&mut passed);
            dnd::sanitizers(&mut passed);
        }
        [flag, task] if flag == "--task" && task == "bridge" => {
            super::bridge::closure(super::BrowserGate::Full, &mut passed);
        }
        [] => {
            origins(&mut passed);
            colours::run(&mut passed);
            shift::run(&mut passed);
            // The full gate retains real-surface coverage alongside offscreen goldens.
            surface_colour::run(&mut passed);
            clipboard::run(&mut passed);
            notifications::run(&mut passed);
            dnd::run(&mut passed);
            context::run(&mut passed);
            text_sizing::run(&mut passed);
            shell::run(&mut passed);
            // Final regression gates reuse the original optimized budget code.
            closure::run(&mut passed);
        }
        _ => report(
            "M18 arguments",
            Err(
                "expected --task origins / colours / shift / surface-colour / clipboard / notifications / dnd / context / text-sizing / shell / sanitizers / bridge, or the full milestone with no flags"
                    .into(),
            ),
            &mut passed,
        ),
    }
    println!("REVIEW M18 comment quality: docs/history/M18.md; pending reviewer");
    passed
}

/// Retains all existing functional and pixel expectations across module moves.
fn origins(passed: &mut bool) {
    report(
        "M18.0 origin modules and support index",
        inventory(),
        passed,
    );
    report(
        "M18.0 existing VT and Session/optional-handler tests",
        origin_tests(),
        passed,
    );
    // No blessing: any changed pixel is a regression in this behaviour-free task.
    report(
        "M18.0 every existing golden unchanged",
        crate::golden::run_all(false, false),
        passed,
    );
}

/// Checks the architectural deliverable, not just the existence of empty folders.
fn family_pages() -> Result<(), String> {
    // The support index must link each family, with its primary reference on the page.
    // A renamed page without an updated index would strand the protocol contract.
    let index = fs::read_to_string("docs/protocols/support.md").map_err(|e| e.to_string())?;
    for origin in include_str!("../../data/protocol-origins.txt").split_whitespace() {
        // Reading the page catches missing or empty deliverables, not just directories.
        let path = format!("docs/protocols/{origin}.md");
        let page = fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
        // Reference URLs belong to origin pages; reference parity is a separate column.
        if !index.contains(&format!("{origin}.md")) || !page.contains("Reference:") {
            return Err(format!("{origin} lacks index entry or primary reference"));
        }
    }
    Ok(())
}

/// Origin interpretation moves while shared storage and policy keep their owners.
fn inventory() -> Result<(), String> {
    family_pages()?;
    // The common routes must no longer contain these unrelated extension bodies.
    for path in [
        "crates/puzzle-vt/src/perform/osc/host.rs",
        "crates/puzzle-vt/src/perform/osc/link.rs",
    ] {
        // Old generic OSC helpers must contain only common xterm interpretation now.
        let body = fs::read_to_string(path).map_err(|e| e.to_string())?;
        for old in [
            "fn shell_mark(",
            "fn notification_777(",
            "fn notification_or_progress(",
            "fn hyperlink(",
        ] {
            // An old extension body left here would create a second interpretation path.
            if body.contains(old) {
                return Err(format!(
                    "{path} still interprets an origin extension: {old}"
                ));
            }
        }
    }
    // Storage and host policy remain shared; origin organization must not duplicate them.
    for path in [
        "crates/puzzle-term/src/images/store.rs",
        "crates/puzzle-term/src/images/admission.rs",
        "crates/puzzle-vt/src/links.rs",
        "crates/puzzle-vt/src/zones.rs",
    ] {
        // The model locations are stable clients of more than one protocol family.
        if !std::path::Path::new(path).is_file() {
            return Err(format!("shared model was moved into an origin: {path}"));
        }
    }
    Ok(())
}

// Protocol-specific packages are checked only in distributions that ship them.
include!("m18/optional_items.rs");
