//! Standard C interface checks need no optional build variants.
/// Compile the header/caller witness without opening a native window.
pub(crate) fn run(mode: Option<&str>) -> Result<(), String> {
    // Reject unavailable variants instead of quietly substituting a default test.
    if mode.is_some() {
        return Err("usage: cargo xtask ffi".into());
    }
    crate::ffi::verify()
}
