//! This historical gate covers a component outside the standard distribution.
/// Never report an omitted component as a passing executable criterion.
pub(super) fn run(_: &[String]) -> bool {
    eprintln!("requested component is not part of this source distribution; use cargo xtask check");
    false
}
