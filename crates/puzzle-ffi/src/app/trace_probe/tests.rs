//! The C sink must enable the same input target that arms the shared observer.
use super::*;

/// Real tracing dispatch verifies target admission rather than a duplicate predicate.
#[test]
fn c_host_sink_enables_private_input_and_excludes_program_output() -> std::io::Result<()> {
    let path = std::env::temp_dir().join(format!("puzzle-input-sink-{}.log", std::process::id()));
    let file = File::create(&path)?;
    tracing::subscriber::with_default(Probe(Mutex::new(file)), || {
        assert!(tracing::enabled!(target: "puzzle_desktop::input", tracing::Level::DEBUG));
        assert!(tracing::enabled!(target: "puzzle_desktop::caret", tracing::Level::DEBUG));
        tracing::debug!(target: "puzzle_desktop::caret", event="embed.caret_delivered", emitted=2, delivered=1);
        assert!(!tracing::enabled!(target: "puzzle_vt::parser", tracing::Level::DEBUG));
        tracing::debug!(target: "puzzle_desktop::input", event="desktop.input_latency", api_to_writer_ms=0.125);
    });
    let text = std::fs::read_to_string(&path)?;
    assert!(text.contains("desktop.input_latency"));
    assert!(text.contains("api_to_writer_ms=0.125"));
    assert!(text.contains("embed.caret_delivered"));
    assert!(text.contains("emitted=2 delivered=1"));
    std::fs::remove_file(path)?;
    Ok(())
}
