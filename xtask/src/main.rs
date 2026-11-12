//! Repository commands for the M0 build and acceptance gates (design §2.3).
//!
//! Theme and settings generators use the approved native-only JSON parser;
//! no generated palette requires JSON parsing in a shipped terminal crate.
//! Each subcommand returns a `Result`; the dispatcher is the only place that
//! converts a failure into a process exit code. That keeps `check` and
//! `accept` composable without parsing human-oriented output.
// Command output is the xtask CLI protocol (project rule R10 exception).
#![allow(clippy::print_stdout, reason = "xtask prints command results")]
#![allow(clippy::print_stderr, reason = "xtask reports failures to stderr")]
/// Per-criterion acceptance report and its pending human review evidence.
mod accept;
/// Native example build and host-owned test orchestration (D-56).
mod app;
/// Headless Chrome witnesses against an explicit local development bridge.
mod browser;
/// Bus catalogue reference generated without a crate-graph cycle.
mod bus_docs;
/// Build, shell, and milestone acceptance commands.
mod commands;
/// Native parse-and-grid throughput corpus for M5.2.
mod corpus;
/// Curated immutable source export, verified before a maintainer publishes it.
mod export;
/// Checked native C header, strict caller build and sanitizer receipts.
mod ffi;
/// Prebuilds four approved font faces as validated Slug texel packs.
mod font_pack;
/// User-approved isolated nightly fuzz targets, run only on explicit request.
mod fuzz;
/// Cross-process exclusion keeps native and browser acceptance from sharing the GPU.
mod gate;
/// Native Slug scene references with the fixed §2.7 comparison tolerance.
mod golden;
/// Source and manifest rules beyond Rust's compiler lints.
mod lint;
/// Native M4 shaping, pack decoding and first-use hinting measurements.
mod m4_bench;
/// Cargo-derived third-party license summary.
mod notices;
/// Optional tool variants are removable independently of the shared checks.
mod optional_cli;
/// Rejects development bridge artifacts in the distributed web package.
mod package;
/// Stored-deflate PNG reader for dependency-free golden comparisons.
mod png;
/// Real-process ANSI chart and box fixtures captured by macOS script.
mod preview_fixture;
/// Generated settings guide, JSON Schema and TypeScript key map.
mod settings_docs;
/// Slug CPU-oracle diagnostics and preparation benchmark.
mod slug;
/// Compile and verify the bundled D-32 terminfo entry.
mod terminfo;
/// Generates pinned palette constants from reviewed JSON source assets.
mod themes;
/// Unicode fallback flood through real PTYs and the production desktop window.
mod unicode_flood;
/// Generates declarations matching the shipped hashed JS module.
mod web_types;
use std::process::ExitCode;
/// Dispatches the requested repository task and reports a concise failure.
/// The first argument selects the operation; a second argument carries only
/// the milestone name or scenario path needed by that operation.
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Acquire before spawning any child; nested build/harness helpers do not relock.
    let _gate = match gate::acquire(&args) {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!("gate serialization: {error}");
            return ExitCode::FAILURE;
        }
    };
    if args.first().map(String::as_str) == Some("accept") {
        // Acceptance owns its exit code so failures need no extra stderr summary.
        return if accept::run(&args[1..]) {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    let result = match args.first().map(String::as_str) {
        Some("browser-workloads") => args.get(1).ok_or("workload producer path missing".into()).and_then(|path| browser::workloads(path)),
        Some("unicode-flood") => unicode_flood::native(),
        Some("check") => commands::check(),
        Some("export") => export::run(&args[1..]),
        Some("ffi") => optional_cli::run(args.get(1).map(String::as_str)),
        Some("fuzz") => fuzz::run(&args[1..]),
        Some("app") => app::run(&args[1..]),
        Some("lint") => lint::run(),
        Some("run") => commands::desktop_run(&args[1..]),
        Some("bridge") => commands::bridge(&args[1..]),
        Some("harness") => commands::harness::run(&args[1..]),
        Some("golden") => match args.get(1).map(String::as_str) {
            None => golden::run_all(false, true),
            Some("--bless") => golden::run_all(true, true),
            Some("images") => golden::images::run(args.iter().any(|arg| arg == "--bless"), true),
            _ => Err("usage: cargo xtask golden [--bless]".into()),
        },
        Some("web") => commands::web(args.get(1).map(String::as_str)),
        Some("notices") => notices::run(),
        Some("settings-docs") => settings_docs::run(),
        Some("slug-dump") => slug::run(&args[1..]),
        Some("slug-bench") => slug::bench(),
        Some("font-pack") => font_pack::run(),
        Some("m4-bench") => m4_bench::run(),
        Some("corpus") => corpus::run(),
        Some("terminfo") => terminfo::run(),
        Some("themes") => themes::run(),
        Some("preview-chart") => preview_fixture::chart(),
        Some("preview-box") => preview_fixture::box_grid(),
        _ => Err("usage: cargo xtask <check|lint|golden|accept M<n>|export DIR|run|harness FILE|web serve|web build|font-pack|m4-bench|corpus|settings-docs|notices>".into()),
    };
    match result {
        // A failed child command propagates to Cargo as a nonzero status.
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
