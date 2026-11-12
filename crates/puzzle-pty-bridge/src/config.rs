//! Explicit development-server options; no configurable bind address (§12.5).
//!
//! Additional origins are exact page origins, not URL prefixes or wildcards.
//! Shell arguments are absent from the CLI: PATH names an executable, not
//! shell-interpreted text. Tests may construct argv for deterministic children.
use std::path::PathBuf;

/// Loopback classification affects only the startup warning, never authorization.
mod origins;

/// Choices fixed before the listener accepts a connection.
#[derive(Clone, Debug)]
pub struct Config {
    /// Executable and argv; by default the user's login shell with -l.
    pub shell: Vec<String>,
    /// TCP port only; the address is always IPv4 loopback.
    pub port: u16,
    /// Exact authorized origins, including the two local Trunk defaults.
    pub origins: Vec<String>,
    /// Optional output recording base; extra sessions get numbered siblings.
    pub record: Option<PathBuf>,
}

/// Defaults authorize local development pages, never arbitrary websites.
impl Default for Config {
    /// Uses SHELL without interpreting it through another shell process.
    fn default() -> Self {
        // SHELL is trusted process configuration, not a WebSocket field.
        // Passing an executable and -l separately preserves paths with spaces.
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        Self {
            shell: vec![shell, "-l".into()],
            port: 7681,
            origins: vec![
                "http://127.0.0.1:8080".into(),
                "http://localhost:8080".into(),
            ],
            record: None,
        }
    }
}

/// CLI validation prevents accidental empty allowlist entries or invalid ports.
impl Config {
    /// Names explicit non-loopback grants so the CLI can warn without exposing a token.
    pub fn non_loopback_origins(&self) -> impl Iterator<Item = &str> {
        // Defaults are local; additional origins remain exact-match grants even after warning.
        self.origins
            .iter()
            .map(String::as_str)
            .filter(|origin| !origins::is_loopback(origin))
    }

    /// Parses native bridge options, returning a report before binding on error.
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let mut config = Self::default();
        // Every supported option takes exactly one value; refusing incomplete
        // pairs keeps a trailing --allow-origin from silently changing defaults.
        if !args.len().is_multiple_of(2) {
            return Err("bridge options require values".into());
        }
        // All options are resolved before entropy, bind or child creation.
        // In particular, a failed port parse cannot start a partial server.
        for pair in args.chunks_exact(2) {
            match pair[0].as_str() {
                "--shell" if !pair[1].is_empty() => {
                    config.shell = vec![pair[1].clone(), "-l".into()]
                }
                "--port" => config.port = pair[1].parse().map_err(|_| "invalid port")?,
                "--record" => config.record = Some(PathBuf::from(&pair[1])),
                "--allow-origin" if valid_origin(&pair[1]) => config.origins.push(pair[1].clone()),
                _ => return Err(format!("invalid bridge option: {}", pair[0])),
            }
        }
        Ok(config)
    }
}

/// Accepts only an HTTP(S) origin authority with no path, query or credentials.
fn valid_origin(origin: &str) -> bool {
    let authority = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"));
    // Exact matching makes explicit nonlocal approvals possible without
    // opening the default allowlist to sibling paths or suffix domains.
    authority.is_some_and(|value| {
        !value.is_empty()
            && value.is_ascii()
            && !value
                .chars()
                .any(|c| c.is_ascii_whitespace() || "/?#@*".contains(c))
    })
}

/// Invalid options fail before opening any listener.
#[cfg(test)]
mod tests;
