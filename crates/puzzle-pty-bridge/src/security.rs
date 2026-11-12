//! Authorization before WebSocket upgrade or PTY creation (design §12.5).
//!
//! Tokens are OS-random, not timestamps or counters. Comparison examines
//! every byte of a fixed-size token rather than stopping at the first mismatch.
//! Neither tokens nor request URLs are written to tracing logs. The startup
//! URL is printed once by the binary for the authorized local user.
use crate::config::Config;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::StatusCode;

/// Generates 128 bits using the approved OS entropy source (D-19).
/// Entropy failure prevents startup instead of weakening authentication.
pub(crate) fn token() -> Result<String, String> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| format!("bridge entropy: {error}"))?;
    Ok(bytes.iter().map(|value| format!("{value:02x}")).collect())
}

/// Fixed-length byte comparison with no mismatch-dependent early return.
fn token_matches(candidate: &str, expected: &str) -> bool {
    if candidate.len() != 32 || expected.len() != 32 {
        return false;
    }
    // Length is public. Only the accumulated difference depends on the
    // secret; all 32 hex bytes participate regardless of matching prefix.
    candidate
        .bytes()
        .zip(expected.bytes())
        .fold(0_u8, |different, (a, b)| different | (a ^ b))
        == 0
}

/// Denies missing, duplicate or malformed origin/token metadata.
/// One exact path/query form makes ambiguous parameter parsing impossible.
#[allow(
    clippy::result_large_err,
    reason = "Tungstenite Callback fixes the HTTP error response type"
)]
pub(crate) fn authorize(
    request: &Request,
    response: Response,
    config: &Config,
    token: &str,
) -> Result<Response, ErrorResponse> {
    // Two Origin headers are ambiguous across HTTP intermediaries: reject
    // them rather than trusting whichever a parser happens to return first.
    let origins: Vec<_> = request.headers().get_all("origin").iter().collect();
    // Exact equality excludes suffix domains, paths and the opaque "null"
    // origin. A token alone cannot grant a foreign browser page a shell.
    let origin_ok = origins.len() == 1
        && origins[0]
            .to_str()
            .ok()
            .is_some_and(|origin| config.origins.iter().any(|allowed| allowed == origin));
    // The query is a single opaque hex credential. Extra parameters,
    // duplicate token keys and percent-encoding all fail this exact form.
    let token_ok = request.uri().path() == "/pty"
        && request
            .uri()
            .query()
            .and_then(|query| query.strip_prefix("token="))
            .is_some_and(|candidate| token_matches(candidate, token));
    if !origin_ok || !token_ok {
        // The HTTP denial does not reveal which credential failed or
        // disclose a portion of the expected random token.
        let mut denied = ErrorResponse::new(Some("bridge authorization required".into()));
        *denied.status_mut() = StatusCode::FORBIDDEN;
        return Err(denied);
    }
    Ok(response)
}

#[cfg(test)]
/// Prefix/suffix and entropy tests exercise the authentication primitives.
mod tests;
