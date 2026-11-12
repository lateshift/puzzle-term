// Only standard C calls are distributed in this source tree.
/// Never silently run a base archive in place of an unavailable variant.
fn extra_flags(_: &mut Command, status: bool, extra: bool) -> Result<(), String> {
    // A requested non-shipped variant cannot be mistaken for a successful base run.
    if status || extra {
        return Err("requested C variant is not distributed".into());
    }
    Ok(())
}
/// Strict compilation still covers every declaration which this distribution ships.
fn extra_defines(command: &mut Command, status: bool, extra: bool) -> Result<(), String> {
    extra_flags(command, status, extra)
}
