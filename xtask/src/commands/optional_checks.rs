//! The standard distribution has no additional feature-dependent protocols.
/// All shipped crates are already covered by the ordinary workspace gate.
pub(super) fn check() -> Result<(), String> {
    Ok(())
}
