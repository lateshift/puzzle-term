//! D-50 source gates catch ownership and diagnostic-path regressions.
use super::*;

/// A nested Arc or qualified/aliased Session must not recreate the old lock.
#[test]
fn shared_guards_cannot_hide_behind_aliases_or_whitespace() {
    for code in [
        "let x: Arc<Mutex<Session>>;",
        "let x: RwLock<puzzle_term::Session>;",
        "use puzzle_term::Session as State; type Alias = State; let x: Mutex<Alias>;",
        "let x: Mutex < \n Session >;",
        "type Stored = Box<puzzle_term::Session>; let x: Mutex<Stored>;",
        "use std::sync::Mutex as Guard; let x: Guard<Session>;",
    ] {
        assert!(source(Path::new("crates/puzzle-desktop/src/test.rs"), code).is_err());
    }
}

/// A render resource must not gain direct parser access or a blocking diagnostic query.
#[test]
fn live_reachability_and_probe_calls_are_rejected() {
    let path = Path::new("crates/puzzle-desktop/src/window/live.rs");
    for name in [
        "crates/puzzle-desktop/src/window/live.rs",
        "crates/puzzle-desktop/src/window.rs",
        "crates/puzzle-desktop/src/render_thread.rs",
        // M17 moves ownership, not the live-render prohibition on direct Session access.
        "crates/puzzle-embed/src/render_thread.rs",
        "crates/puzzle-embed/src/window/live.rs",
    ] {
        assert!(
            source(
                Path::new(name),
                "use puzzle_term::Session as Source; let source: Source;"
            )
            .is_err()
        );
    }
    assert!(source(path, "self.owner.probe(deadline, callback);").is_err());
    assert!(source(path, "let x = Control::Probe(callback);").is_err());
    assert!(source(path, "// Mutex<Session>\nlet note = \".probe(\";").is_ok());
    assert!(
        source(
            Path::new("crates/puzzle-desktop/src/harness/fixture.rs"),
            "owner.probe(deadline, callback);"
        )
        .is_ok()
    );
}

/// The owner-neutral core cannot add its own synchronization when the web stays single-threaded.
#[test]
fn neutral_core_rejects_synchronization() {
    assert!(
        source(
            Path::new("crates/puzzle-term/src/session.rs"),
            "use std::sync::atomic::AtomicBool;"
        )
        .is_err()
    );
}
