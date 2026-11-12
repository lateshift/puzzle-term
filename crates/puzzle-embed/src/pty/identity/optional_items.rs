// Standard child creation contains only terminal identity policy.
/// No additional launch environment exists in the standard distribution.
fn strip_extra(_: &mut CommandBuilder) {}
/// Preserve the common cold constructor without installing any private handoff.
pub(super) fn apply_extra(_: &mut CommandBuilder, _: Option<&std::path::Path>) {}
