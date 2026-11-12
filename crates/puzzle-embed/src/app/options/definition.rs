//! Shared process options; distribution-specific fields are supplied once.

/// Declares the options and defaults together so fields cannot drift.
// A struct field and its default are supplied by the same invocation.
// The common accessors still type-check in both distribution profiles.
macro_rules! define_options {
    ({$($fields:tt)*}, {$($defaults:tt)*}) => {
/// Per-surface process policy; omitted fields preserve the existing login-shell behavior.
pub struct SurfaceOptions {
    /// None selects the App's resolved settings snapshot.
    pub settings: Option<DesktopSettings>,
    /// Host appearance is distinct from an explicit theme setting.
    pub appearance: Appearance,
    /// BuiltIn by default; Host emits find intent without opening a query overlay.
    pub search_ui: puzzle_term::search::SearchUi,
    /// Default owns a PTY; Host accepts output through Surface::feed_output.
    pub transport: super::TransportMode,
    /// None selects SHELL (or /bin/sh); the chosen shell starts as a login shell on Unix.
    pub shell: Option<OsString>,
    /// None preserves the PTY library's inherited working directory.
    pub cwd: Option<PathBuf>,
    /// Additional environment values; Puzzle's terminal identity remains authoritative.
    pub env: Vec<(OsString, OsString)>,
    /// Optional host creation stamp keeps first-ink startup measurement inclusive of native UI work.
    pub window_created: Option<std::time::Instant>,
            $($fields)*
    /// Opt-in private diagnostic recording, shared with the native workers.
    #[cfg(feature = "test-support")]
    pub recorder: Option<Recorder>,
    /// Host source identity for the recording header, without invoking Git in the engine.
    #[cfg(feature = "test-support")]
    pub revision: Option<String>,
    /// Finite authored probes stay in the harness, attached through this observation seam.
    #[cfg(all(feature = "lock-metrics", feature = "test-support"))]
    pub observer: Option<Box<dyn crate::window::observer::SurfaceObserver>>,
}
impl Default for SurfaceOptions {
    /// Default policy opens a normal login shell and assumes dark when no host preference exists.
    fn default() -> Self {
        Self {
            settings: None,
            appearance: Appearance::Dark,
            search_ui: puzzle_term::search::SearchUi::BuiltIn,
            transport: super::TransportMode::Pty,
            shell: None,
            cwd: None,
            env: Vec::new(),
            window_created: None,
            $($defaults)*
            #[cfg(feature = "test-support")]
            recorder: None,
            #[cfg(feature = "test-support")]
            revision: None,
            #[cfg(all(feature = "lock-metrics", feature = "test-support"))]
            observer: None,
        }
    }
}

    };
}
