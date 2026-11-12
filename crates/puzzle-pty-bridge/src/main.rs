//! CLI for the local development PTY bridge (design §12.5).
//!
//! The warning and secret URL are intentional startup stdout, not library
//! logging. Subsequent lifecycle and failure events go to tracing on stderr.
#![allow(
    clippy::print_stdout,
    reason = "bridge prints its access URL once for the local user"
)]
#![allow(clippy::print_stderr, reason = "bridge CLI reports startup errors")]
//! Authentication is in the library so real-socket tests exercise the same
//! path as this CLI. Arguments are OS strings converted once into options;
//! no command line is constructed by concatenating shell fragments.
//! The listener address is fixed by Bridge, including when a caller changes
//! the port. --allow-origin explicitly grants a page access and is therefore
//! validated before bind.
//! PUZZLE_LOG can enable bridge.backpressure at debug for a slow client;
//! ordinary startup stays at info and never emits output bytes or tokens.
//! Invalid filter syntax falls back to info so diagnostics cannot prevent
//! the user from starting an otherwise valid development server.
//! --record is opt-in because output can contain private application data.
use puzzle_pty_bridge::{config::Config, server::Bridge, shutdown::Shutdown};
use std::process::ExitCode;

/// Starts the server only after all options and entropy have been validated.
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args == ["--help"] {
        println!(
            "puzzle-pty-bridge [--shell PATH] [--port 7681] [--record FILE.vtrec] [--allow-origin ORIGIN]…\nLocal development only. Anyone with the token and an allowed Origin has shell access. Recordings contain private output."
        );
        return ExitCode::SUCCESS;
    }
    // A startup failure is reported on stderr with a failing exit status.
    // The secret URL alone belongs on stdout; logs never contain that URL.
    // Targets parsing supplies opt-in backpressure diagnostics without the
    // regex/env-filter feature that the workspace deliberately avoids.
    let filter = std::env::var("PUZZLE_LOG")
        .unwrap_or_else(|_| "info".into())
        .parse::<tracing_subscriber::filter::Targets>()
        .unwrap_or_else(|_| {
            tracing_subscriber::filter::Targets::new().with_default(tracing::Level::INFO)
        });
    use tracing_subscriber::prelude::*;
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_filter(filter),
        )
        .init();
    match start(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// Keeps the token out of the logging layer and warns about its authority.
fn start(args: &[String]) -> Result<(), String> {
    let config = Config::parse(args)?;
    // Like the token-authority banner, this must remain visible even with PUZZLE_LOG=error.
    // It identifies the granted page origin, never the private access URL or token.
    for origin in config.non_loopback_origins() {
        println!(
            "WARNING: --allow-origin {origin} authorizes a non-loopback page to access a full local shell with the token."
        );
    }
    let bridge = Bridge::bind(config)?;
    println!(
        "WARNING: development PTY bridge grants full local shell access. Keep this token private. Loopback only; at most four sessions."
    );
    println!("{}", bridge.url()?);
    bridge.serve(Shutdown::default())
}
