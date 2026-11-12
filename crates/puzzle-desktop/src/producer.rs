//! Standard harness launch policy uses the configured login shell.
/// The standard CLI has no additional child-launch request.
#[derive(Debug)]
pub(crate) enum Request {}
/// Extra options are deliberately absent from this distribution.
#[derive(Default)]
pub(crate) struct Options {
    /// Keep the standard parser zero-sized while preserving its construction API.
    _reserved: (),
}
// The shared CLI asks this parser before its ordinary flag catalogue.
impl Options {
    /// Leave ordinary CLI flags to the main parser.
    pub(crate) fn option(&mut self, _: &str, _: &[&str]) -> Result<Option<usize>, String> {
        Ok(None)
    }
    /// Standard parsing never creates an extra launch request.
    pub(crate) fn finish(self) -> Result<Option<Request>, String> {
        // No executable request can be manufactured by standard option parsing.
        Ok(None)
    }
}
/// Standard surface creation preserves login-shell options.
pub(crate) fn configure(
    _: Option<Request>,
    _: &mut puzzle_embed::SurfaceOptions,
) -> Result<bool, String> {
    Ok(false)
}
/// Standard runtime startup requires no extra protocol registration.
pub(crate) fn configure_surface(_: &puzzle_embed::Surface, _: bool) -> Result<(), String> {
    Ok(())
}
