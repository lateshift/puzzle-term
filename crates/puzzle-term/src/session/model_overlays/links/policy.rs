//! Configured link policy is validated again after the OpenLink interceptor chain.
//!
//! Gesture detection is not authority: an interceptor may replace a target, and
//! direct API dispatch also uses the command lane. The fixed safe-scheme filter
//! therefore runs after policy hooks and before the host opener. A configured
//! scheme list can narrow this filter but cannot enable JavaScript or data URLs.
//! Native hosts may open file URLs; browsers never do, even if configured to.
//! Settings changes invalidate hover lookup without changing glyph or grid state.

/// Small Session-owned input policy, with no platform handles or OS queries.
pub(in crate::session) struct Policy {
    /// Plain URL detection is independent of explicit OSC 8 metadata.
    pub detect: bool,
    /// The disabled gesture policy suppresses both UI and direct opening requests.
    pub enabled: bool,
    /// Configured schemes can only reduce the hard safe-scheme allowlist.
    pub schemes: Vec<String>,
}
/// Bare Sessions follow the native default until a host installs its settings.
impl Default for Policy {
    /// Web opening still excludes file regardless of this default list.
    fn default() -> Self {
        Self {
            detect: true,
            enabled: true,
            schemes: ["http", "https", "mailto", "file"]
                .map(str::to_owned)
                .into(),
        }
    }
}
impl Policy {
    /// Checks limits and the immutable security filter before host-side URL I/O.
    pub(in crate::session) fn allows(&self, uri: &str, desktop: bool) -> bool {
        // Reject raw whitespace/control injection; valid URI paths encode such bytes.
        self.enabled
            && uri.len() <= 2048
            && !uri.chars().any(|ch| ch.is_control() || ch.is_whitespace())
            && crate::vt::links::LinkTable::openable(uri, desktop)
            && uri
                .split_once(':')
                .is_some_and(|(scheme, _)| self.schemes.iter().any(|s| s == scheme))
    }
}
