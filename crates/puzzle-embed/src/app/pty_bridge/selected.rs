// Standard shell startup has no additional grant or lifecycle receipt.
/// An ordinary launch needs no extra state.
pub(super) type Launch = Option<std::convert::Infallible>;
/// A completed ordinary launch retains no private resource.
pub(super) type Provisioned = Option<std::convert::Infallible>;
/// No extra path is passed to a standard shell.
// This hook cannot introduce a path or environment value into an ordinary shell.
fn bootstrap_path(_: &Launch) -> Option<&std::path::Path> {
    None
}
/// Starts the configured login shell through the shared PTY constructor.
fn spawn_child(
    grid: PtySize,
    appearance: Appearance,
    options: &SurfaceOptions,
    shell_integration: &str,
    _: Option<&std::path::Path>,
) -> Result<pty::SpawnedPty, String> {
    // Terminal identity, cwd and integration still use the common constructor.
    pty::login::spawn_login_settings(
        grid,
        appearance,
        options.shell.as_deref(),
        options.cwd.as_deref(),
        &options.env,
        shell_integration,
    )
}
/// Standard sessions have nothing extra to install.
fn install(launch: Launch, _: &mut puzzle_term::Session) -> Result<Provisioned, String> {
    // Launch is an Option of an uninhabited type, so it preserves the empty state.
    Ok(launch)
}
/// Every ordinary event is delivered to its host.
// No host fact is intercepted by an absent extension lifecycle.
pub(super) fn observe(_: &Provisioned, _: &puzzle_term::HostEvent) -> bool {
    false
}
