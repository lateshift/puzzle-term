//! Run dispatch and headless introspection of the effective shortcut table.
//!
//! These commands load the same validated config/profile/session layers as
//! the window. They deliberately avoid GPU and PTY startup, so inspecting a
//! broken binding does not spawn a shell or require a display (§14.9).
//! Provenance comes from the original layer maps, not the flattened settings.
//! An overridden trigger appears once with its winning source; defaults that
//! remain active keep the Defaults label even with an active named profile.
//! Unknown or malformed entries use the store's per-entry report contract.
use crate::{RunOptions, settings::DesktopSettings};

/// Loads once, then either prints the requested data or opens the window.
pub(crate) fn run(args: &[&str]) -> Result<(), String> {
    // Parse once: --help after a producer's -- delimiter belongs to its argv.
    let options = crate::run_options(args)?;
    if options.help {
        println!(
            "puzzle-desktop run [--config PATH] [--profile NAME] [--set key=value] [--list-keybindings] [--list-actions] [--log-file PATH] [--record [FILE.pzrec]] [--approved-producer PROGRAM --producer-capabilities LIST [--producer-cwd PATH] [-- ARG...]]\npuzzle-desktop --init-config [PATH] writes a starter config and editor schema. Recordings contain everything typed and shown.\nPUZZLE_VERIFY_FRAMES=1 checks cold redraws. A synthetic check took about 15 ms to build and 15 ms to render/read back; the live verify-mode gaps remain unexplained. Judge pacing in a normal run."
        );
        return Ok(());
    }
    // Share parsing with run so --set and --profile cannot resolve a different
    // effective table merely because this command has no native window.
    if options.lists.iter().any(|list| *list) {
        list(&options)
    } else {
        crate::window::run(options)
    }
}

/// Emits one stable row per binding or action with its original source.
fn list(options: &RunOptions) -> Result<(), String> {
    let (settings, warnings) = DesktopSettings::open(
        options.config.as_deref(),
        options.profile.as_deref(),
        &options.sets,
    )?;
    // Introspection has no window initialization to install stderr tracing.
    crate::logging::init(options.log.as_deref())?;
    // Config warnings use stderr; stdout remains machine-readable table data.
    for warning in warnings {
        tracing::warn!(event="settings.invalid", %warning);
    }
    let bindings = settings.store.keybindings();
    if options.lists[0] {
        for binding in bindings.list() {
            // A flattened effective setting cannot explain layer ownership.
            // This is the exact table that Session receives, not a CLI copy.
            println!(
                "{}\t{:?}\t{:?}",
                binding.canonical(),
                binding.action,
                binding.source
            );
        }
    }
    if options.lists[1] {
        for info in bindings.actions() {
            // Future features stay discoverable with their availability.
            println!(
                "{}\t{}\t{}\t{}",
                info.name, info.argument, info.available, info.description
            );
        }
    }
    Ok(())
}
