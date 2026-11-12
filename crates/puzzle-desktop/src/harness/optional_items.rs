// Standard scenarios need no extra replay state.
/// Uninhabited state preserves a single scenario construction path.
type ExtraScene = Option<std::convert::Infallible>;
/// No extra command family intercepts standard script input.
fn extra_command(state: &mut Scenario, _: &str) -> Option<Result<(), String>> {
    let _ = &state.extra_scene;
    None
}
/// Plain VT replay requires no additional observations.
fn extra_replay(state: &mut Scenario, _: &str, _: &[u8]) -> Result<(), String> {
    let _ = &state.extra_scene;
    Ok(())
}
