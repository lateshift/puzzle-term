//! Native callback boundary rejects owner aliases without rejecting observations.
use super::*;

#[test]
fn ordinary_callbacks_cannot_import_owners_or_diagnostic_modules() {
    let path = Path::new("crates/puzzle-desktop/src/window/ui/new_callback.rs");
    for code in [
        "use puzzle_term::Session as Terminal;",
        "use crate::session_thread as endpoint;",
        "use puzzle_embed::diagnostics as internal;",
        "surface.send(SessionMessage::ForceRecapture);",
        "surface.update(SurfaceUpdate::Frame);",
    ] {
        assert!(source(path, code).is_err(), "accepted {code}");
    }
}

#[test]
fn typed_surface_callbacks_and_observers_stay_available() {
    let path = Path::new("crates/puzzle-desktop/src/window/ui/input.rs");
    assert!(
        source(
            path,
            "use puzzle_embed::{Surface, RawInput}; surface.input(raw)?;"
        )
        .is_ok()
    );
    assert!(source(path, "// no Session lock\nlet label = \"SessionMessage\";").is_ok());
    assert!(source(path, "let stage = crate::resize_timing::Stage::new(label);").is_ok());
    let probe = Path::new("crates/puzzle-desktop/src/window/scroll_probe/timing.rs");
    assert!(
        source(
            probe,
            "use puzzle_embed::diagnostics::session_thread::Handle;"
        )
        .is_ok()
    );
}
