//! Token comparison and uniqueness without persisting generated secrets.
#![allow(
    clippy::unwrap_used,
    reason = "assertions expose setup or contract failures in tests"
)]
use super::*;

/// Wrong bytes at either end and ambiguous query syntax cannot authenticate.
#[test]
fn fixed_token_comparison() {
    let expected = "0123456789abcdef0123456789abcdef";
    assert!(token_matches(expected, expected));
    for candidate in [
        "",
        "0123456789abcdef0123456789abcdee",
        "1123456789abcdef0123456789abcdef",
        "0123456789abcdef0123456789abcdef&x=1",
    ] {
        assert!(!token_matches(candidate, expected));
    }
}

/// OS randomness supplies the full 16-byte token, not a predictable run counter.
#[test]
fn random_128_bit_tokens() {
    let tokens: std::collections::BTreeSet<_> = (0..128).map(|_| token().unwrap()).collect();
    assert_eq!(tokens.len(), 128);
    assert!(
        tokens
            .iter()
            .all(|token| token.len() == 32 && token.bytes().all(|b| b.is_ascii_hexdigit()))
    );
}

/// Duplicate origins and ambiguous credentials cannot be authorized by parser choice.
#[test]
fn ambiguous_metadata_rejected() {
    let config = Config::default();
    let token = "0123456789abcdef0123456789abcdef";
    for origin_count in [0, 2] {
        let mut request = Request::builder()
            .uri(format!("/pty?token={token}"))
            .body(())
            .unwrap();
        for _ in 0..origin_count {
            request
                .headers_mut()
                .append("Origin", "http://localhost:8080".parse().unwrap());
        }
        assert!(authorize(&request, Response::new(()), &config, token).is_err());
    }
    for uri in [
        format!("/pty?token={token}&token={token}"),
        format!("/other?token={token}"),
        format!("/pty?token=%30{}", &token[1..]),
    ] {
        let request = Request::builder()
            .uri(uri)
            .header("Origin", "http://localhost:8080")
            .body(())
            .unwrap();
        assert!(authorize(&request, Response::new(()), &config, token).is_err());
    }
}
