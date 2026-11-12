//! Small functional traces validate labels without running the timed benchmark.
use super::*;

#[test]
fn outside_writes_keep_the_watch_live_and_inside_writes_invalidate() -> Result<(), String> {
    for scalar in ['a', 'é'] {
        for mode in [Mode::None, Mode::Outside, Mode::Inside] {
            let (mut terminal, watch) = terminal(mode)?;
            let bytes = unit(mode, scalar);
            // Repetition proves the outside case stays live; the first write
            // must not silently retire its watch and measure the no-watch loop.
            for _ in 0..3 {
                terminal.advance(bytes.as_bytes());
                if let Some(watch) = watch {
                    let state = terminal.source_watch(watch).ok_or("watch disappeared")?;
                    assert_eq!(state.changed, matches!(mode, Mode::Inside));
                }
            }
        }
    }
    Ok(())
}
