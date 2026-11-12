//! Classifies explicitly allowed origins for the startup authority warning.
//!
//! Authorization still compares the complete origin exactly. This classification
//! only suppresses the extra warning for literal loopback IPs and localhost;
//! it never resolves DNS or adds an origin to the allowlist.
use std::net::IpAddr;

/// Identifies literal loopback hosts; an unrecognized host keeps the warning enabled.
pub(super) fn is_loopback(origin: &str) -> bool {
    let Some(authority) = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
    else {
        return false;
    };
    // IPv6 brackets separate the host from an optional port without DNS lookup.
    let host = if let Some(ipv6) = authority.strip_prefix('[') {
        let Some((host, tail)) = ipv6.split_once(']') else {
            return false;
        };
        if !tail.is_empty() && !tail.starts_with(':') {
            return false;
        }
        host
    } else {
        authority
            .split_once(':')
            .map_or(authority, |(host, _)| host)
    };
    // localhost is the one explicit local hostname; suffix lookalikes still warn.
    // IP parsing recognizes the entire 127/8 range and IPv6 ::1 without a URL crate.
    // Classification does not normalize or rewrite the exact authorized origin.
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}
