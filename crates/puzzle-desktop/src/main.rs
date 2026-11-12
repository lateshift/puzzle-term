//! Native M0 window and headless scenario entry points (design §13.1).
//!
//! Later milestones connect terminal state and PTY bytes to these two modes.
//! Both modes use `puzzle-render`; this binary is the native owner of winit
//! handles and of scenario files, keeping platform concerns out of the core.
//!
//! `run --record` is deliberately opt-in because it captures terminal
//! output and every typed key. A pathless request writes into ignored
//! target data; the native option parser preserves a user-supplied path as
//! one argument, including spaces, without invoking a shell. The recorder
//! itself is created before the first PTY read so startup negotiation is
//! present in the evidence as well as later user input.
// The binary reports command results to its human caller (R10 exception).
#![allow(
    clippy::print_stdout,
    reason = "command-line output is the binary interface"
)]
#![allow(clippy::print_stderr, reason = "command-line errors must reach stderr")]
/// Desktop-only PNG decoder for embedded color-font strikes.
mod bitmap;
/// Installed color-emoji readback is separate from deterministic harness references.
mod bitmap_probe;
/// Std-only executor for the native adapter request.
mod block_on;
/// Headless shortcut introspection and native run option dispatch.
mod cli;
/// System text clipboard used only by the live desktop shell.
mod clipboard;
/// Approved COLR test-font oracle and gradient reference crops.
mod colr_probe;
/// Reads the vendored font at the native platform boundary.
mod font_data;
/// M6 render-contract scene and reviewed offscreen image.
mod frame_probe;
/// Serializes native GPU fixtures independently of the parallel CPU test harness.
#[cfg(test)]
mod gpu_test;
/// Native adapter batches Slug grid coverage on the existing render device.
mod grid_rasterizer;
/// Scenario parser and deterministic headless evidence writer.
mod harness;
/// Winit logical keys translated into platform-neutral terminal keys.
mod keyboard;
/// Full-screen PTY witness for recorded key sequences and frame timing.
mod keyecho;
/// Native tracing target selection for both execution modes.
mod logging;
/// Physical pointer coalescing shared by native callbacks and table tests.
mod mouse;
/// Fresh primary calibration and dense overlay preparation evidence.
mod overlay_probe;
/// Installed-font affine evidence stays outside the golden catalogue.
mod overlay_review;
/// M4 proof that a font pack uploads and draws without loading a TTF.
mod pack_probe;
/// Stored-deflate PNG encoder used by screenshots.
mod png;
/// Portable local PTY used by both the live window and smoke probe.
mod pty;
/// Fixed 4 MiB PTY read-buffer pool with blocking handoff.
mod pty_pool;
/// Optional JSON Lines capture of live native input, PTY bytes and frames.
mod recording;
/// Process rendering actor and native-host surface seam (D-41, D-43).
mod render_thread;
/// Single-owner Session messages and rotating snapshot publication (D-50).
mod session_thread;
/// Native JSON config adapter; neutral crates see only bus values.
mod settings;
/// Native Naga validation includes every shader and control-flow uniformity.
#[cfg(test)]
mod shader_validation;
/// One synthetic Slug GPU scene for early shader validation.
mod slug_probe;
/// Native fontique discovery is installed only in live windows.
mod system_fonts;
/// Native M11.7 full-grid, bitmap and raster-memory measurements.
mod system_measure;
/// Opt-in production-frame readback with installed fonts at both display scales.
mod system_probe;
/// Compares tmux's synchronized redraws with both TERM identities.
mod terminfo_probe;
/// Captures privacy-neutral tmux redraw streams at a fixed PTY size.
mod tmux_corpus;
/// Interactive winit window and Metal presentation path.
mod window;
/// Dispatches between a presentable window and a headless scenario.
/// Unknown modes fail before creating a GPU device, making CLI mistakes cheap.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("harness") => harness::run(args.get(1).map(String::as_str)),
        Some("keyecho") => keyecho::run(&args[1..]),
        Some("slug-probe") => slug_probe::run(args.get(1).map(String::as_str)),
        Some("grid-raster-bench") => grid_rasterizer::bench::run(),
        Some("slug-bench") => slug_probe::benchmark(),
        Some("slug-store") => slug_probe::store_growth(),
        Some("slug-paint") => slug_probe::paint_table(),
        Some("colr-probe") => colr_probe::run(),
        Some("bitmap-probe") => bitmap_probe::run(),
        Some("system-font-probe") => system_probe::run(),
        Some("coverage-measure") => system_measure::run(),
        Some("system-font-bench") => system_fonts::bench::run(),
        Some("overlay-bench") => overlay_probe::run(),
        Some("overlay-review") => overlay_review::run(),
        Some("pack-probe") => pack_probe::run(),
        Some("frame-probe") => frame_probe::run(),
        Some("pty-smoke") => pty::smoke(),
        Some("exit-probe") => pty::exit_probe(args.get(1).map(String::as_str)),
        Some("tmux-corpus") => tmux_corpus::run(args.get(1).map(String::as_str)),
        Some("terminfo-probe") => terminfo_probe::run(args.get(1).map(String::as_str)),
        Some("--init-config") => {
            settings::DesktopSettings::init_config(args.get(1).map(String::as_str)).map(|path| {
                println!("created {}", path.display());
            })
        }
        Some("run") | None => {
            cli::run(&args.iter().skip(1).map(String::as_str).collect::<Vec<_>>())
        }
        Some("--list-keybindings") | Some("--list-actions") => {
            cli::run(&args.iter().map(String::as_str).collect::<Vec<_>>())
        }
        Some("--help") | Some("help") => cli::run(&["--help"]),
        Some(other) => Err(format!("unknown desktop mode: {other}")),
    };
    if let Err(error) = result {
        // A nonzero status lets `cargo xtask accept` reject a failed scenario.
        eprintln!("{error}");
        std::process::exit(1);
    }
}

/// Parses native options without confusing an optional record path with a flag.
fn run_options(options: &[&str]) -> Result<RunOptions, String> {
    // Paths stay owned as single arguments all the way to the window;
    // interpreting shell syntax here would corrupt paths with spaces.
    let mut parsed = RunOptions::default();
    let mut producer = producer::Options::default();
    let mut index = 0;
    while index < options.len() {
        match options[index] {
            "--help" => {
                parsed.help = true;
                index += 1;
            }
            "--list-keybindings" | "--list-actions" => {
                parsed.lists[usize::from(options[index] == "--list-actions")] = true;
                index += 1;
            }
            "--log-file" => {
                parsed.log =
                    Some((*options.get(index + 1).ok_or("--log-file needs a path")?).into());
                index += 2;
            }
            "--record" => {
                index += parsed.record_option(options.get(index + 1).copied());
            }
            "--config" => {
                parsed.config =
                    Some((*options.get(index + 1).ok_or("--config needs a path")?).into());
                index += 2;
            }
            "--profile" => {
                parsed.profile =
                    Some((*options.get(index + 1).ok_or("--profile needs a name")?).into());
                index += 2;
            }
            "--set" => {
                // Repeated --set flags retain CLI order. A later value
                // for the same key wins within the session layer (§14.2).
                parsed
                    .sets
                    .push((*options.get(index + 1).ok_or("--set needs key=value")?).into());
                index += 2;
            }
            value => {
                index += producer
                    .option(value, &options[index + 1..])?
                    .ok_or_else(|| format!("unknown run option: {value}"))?;
            }
        }
    }
    parsed.producer = if parsed.help {
        None
    } else {
        producer.finish()?
    };
    Ok(parsed)
}

/// Parsed native flags, with paths kept as single OS arguments.
#[derive(Default)]
#[cfg_attr(test, derive(Debug))]
struct RunOptions {
    /// Only outer --help flags reach this branch; child arguments stay literal.
    help: bool,
    /// Explicit command/subset approval; absent for ordinary shells.
    producer: Option<producer::Request>,
    /// Headless introspection requires no PTY, fonts or window.
    lists: [bool; 2],
    /// Optional explicit log destination, taking priority over logging.file.
    log: Option<String>,
    /// Optional recording request and its optional private path.
    record: Option<Option<String>>,
    /// Optional config.json override.
    config: Option<String>,
    /// Optional active profile override.
    profile: Option<String>,
    /// Session-layer key=value overrides in CLI order.
    sets: Vec<String>,
}

/// Paired observer clocks for live resize stages.
mod resize_timing;

/// Explicit approved-producer flags are public policy, never credential handling.
mod producer;

/// Optional recording paths cannot consume the next run flag.
impl RunOptions {
    /// Records the explicit/default destination and returns the consumed argv count.
    fn record_option(&mut self, next: Option<&str>) -> usize {
        // Missing paths select the ignored target directory; flags stay in the parser.
        let path = next.filter(|value| !value.starts_with("--"));
        self.record = Some(path.map(str::to_owned));
        if path.is_some() { 2 } else { 1 }
    }
}
