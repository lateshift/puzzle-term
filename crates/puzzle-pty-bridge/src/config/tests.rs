//! CLI rejects options that could broaden authorization by accident.
use super::*;

/// A bind address is deliberately absent; origins must be explicit authorities.
#[test]
fn options_are_explicit() {
    for args in [
        vec!["--bind", "0.0.0.0"],
        vec!["--port", "65536"],
        vec!["--allow-origin", "*"],
        vec!["--allow-origin", "http://evil.test/"],
        vec!["--allow-origin", "http://user@evil.test"],
        vec!["--shell"],
    ] {
        assert!(Config::parse(&args.into_iter().map(String::from).collect::<Vec<_>>()).is_err());
    }
    let args = [
        "--allow-origin",
        "https://example.test:443",
        "--allow-origin",
        "http://localhost:9000",
        "--port",
        "0",
    ];
    let Ok(config) = Config::parse(&args.map(String::from)) else {
        panic!("valid explicit options rejected")
    };
    assert_eq!(config.origins.len(), 4);
    assert_eq!(config.port, 0);
}

/// Warn on remote page grants without confusing local hosts with suffix lookalikes.
#[test]
fn non_loopback_origin_grants_are_identified() {
    let origins = [
        "http://localhost:8080",
        "https://LOCALHOST:9443",
        "http://127.0.0.2:9000",
        "http://[::1]:9000",
        "https://example.test",
        "http://localhost.evil.test:8080",
        "http://127.0.0.1.evil.test",
        "http://[2001:db8::1]:9000",
    ];
    let config = Config {
        origins: origins.map(String::from).to_vec(),
        ..Config::default()
    };
    assert_eq!(
        config.non_loopback_origins().collect::<Vec<_>>(),
        origins[4..]
    );
}
