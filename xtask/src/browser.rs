//! One real local bridge supplies the browser transport witnesses.
//! The token is passed privately through the test environment, never printed
//! or committed. Chrome's test origin is fixed to match the bridge allowlist.
//!
//! The production client must reach a real PTY: mocked socket packets cannot
//! prove that stty receives a resize or that output precedes a child's exit.
//! This test driver therefore launches the already approved native bridge,
//! then hands its private token URL only to the compiled browser witnesses.
//! A server guard reaps that process even if wasm compilation or Chrome fails.
//!
//! The bridge chooses a free loopback port, but the test page has a fixed
//! origin. Origin authorization depends on the page, not the WebSocket port.
//! The fixed page address is supplied to wasm-bindgen-test as well as the
//! bridge allowlist; accepting any Origin would conceal a security defect.
//!
//! Browser tests use the release profile because M7 measures parser and input
//! latency in the shipped optimized engine. Test-only font byte fixtures and
//! the private socket URL never enter Trunk's non-test distribution build.
//! Capturing both output streams preserves assertion diagnostics while
//! successful compile chatter stays out of each acceptance criterion line.
//! The saved log can be inspected without publishing a connection credential.
//!
//! Real-workload measurements select release wasm and the installed Chrome driver.
//! Each DPR run gets fresh receipt files so a prior child cannot supply its key timing.
//! The workload producer uses only local generated fixtures and already installed tools.
//! One test per workload retains failures without exceeding the browser test harness timeout.
//! The timeout bounds a broken fixture; no latency budget is inferred from it.
//! Raw per-call and presentation evidence stays under target/ for later review.
//! The shipped package remains independent of this local PTY bridge measurement.

use std::{
    fs::{self, File},
    io::Write,
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
};

/// Test-runner HTTP fixtures use the same external bytes as the demo.
mod assets;

/// Reaps the development child even when compilation or a browser test fails.
struct Server(Child);
/// Explicit lifetime keeps a failed browser gate from leaving a shell server running.
impl Drop for Server {
    /// Tests own this process, so terminating it never targets a user session.
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Runs the full browser suite with a real authenticated development transport.
pub(crate) fn suite() -> Result<String, String> {
    run(None)
}

/// An explicitly requested real-workload run uses the same release browser and bridge.
pub(crate) fn workloads(script: &str) -> Result<(), String> {
    let script = fs::canonicalize(script).map_err(|e| e.to_string())?;
    let script = script.to_str().ok_or("workload path is not UTF-8")?;
    println!("{}", run(Some(script))?);
    Ok(())
}

/// Receipt files are per scale, so a preceding Chrome process cannot answer its successor.
fn clear_workload_receipts(script: &str) -> Result<(), String> {
    // Key acknowledgments belong to this run; stale files cannot satisfy receipt.
    for mode in [
        "cat-zh", "cat-ja", "cat-ko", "ls", "nvim", "tmux", "chat", "man",
    ] {
        let path = format!("{script}.{mode}.key");
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}

/// Shared launch ownership prevents diagnostic runs diverging from the acceptance setup.
fn run(workload: Option<&str>) -> Result<String, String> {
    // wasm-bindgen's server serves the crate directory alongside generated JS.
    let _assets = assets::Served::new()?;
    // Build first so the private URL is valid for this checkout's codec and server.
    crate::commands::quiet("cargo", &["build", "-p", "puzzle-pty-bridge"])?;
    // Generate local neutral bytes once; this is not a shipped or vendored asset.
    let flood = flood_fixture()?;
    // Unicode uses the same producer as the native window, including child receipt clocks.
    // Regenerate it before Chrome starts so a stale corpus cannot satisfy the flood guard.
    let _unicode = crate::unicode_flood::fixture()?;
    // Tracing output contains lifecycle/credit decisions, never a token or payload bytes.
    let log = File::create("target/m7-browser-bridge.log").map_err(|e| e.to_string())?;
    // /bin/sh supplies deterministic command behavior without a user's login configuration.
    let child = Command::new("target/debug/puzzle-pty-bridge")
        .args([
            "--port",
            "0",
            "--shell",
            "/bin/sh",
            "--allow-origin",
            "http://127.0.0.1:8088",
        ])
        .stdout(Stdio::piped())
        .env("PUZZLE_LOG", "debug")
        .stderr(Stdio::from(log))
        .spawn()
        .map_err(|e| format!("browser bridge start failed: {e}"))?;
    let mut server = Server(child);
    let url = private_url(&mut server)?;
    // 1× uses hinted runtime outlines; 2× retains unhinted pack glyphs.
    // Both complete suites are required so one backend path cannot hide the other.
    let mut text = String::new();
    let mut success = true;
    for dpr in [1, 2] {
        if let Some(script) = workload {
            // Both scales run every producer anew with independent receipt timestamps.
            clear_workload_receipts(script)?;
        }
        let (output, passed) = chrome(&url, &flood, dpr, workload)?;
        // Continue through the second scale even after failure to retain both diagnoses.
        text.push_str(&format!("Chrome DPR {dpr}\n{output}"));
        success &= passed;
    }
    // Preserve failed assertion evidence as well as successful budget numbers.
    std::fs::write("target/m7-browser.log", &text).map_err(|e| e.to_string())?;
    if !success {
        return Err(text);
    }
    Ok(text)
}

/// A separate browser process gives each display scale a fresh resource registry.
fn chrome(
    url: &str,
    flood: &str,
    dpr: u32,
    workload: Option<&str>,
) -> Result<(String, bool), String> {
    // wasm-bindgen's watchdog covers the entire invocation, not each test.
    // Each six-second real workload therefore gets its own unchanged watchdog;
    // ordinary acceptance still runs the existing complete suite together.
    // Fresh processes give each cold-font observation independent source readiness.
    // The same bridge remains authenticated for every invocation at this scale.
    // Exact workload filters avoid rerunning unrelated acceptance witnesses.
    // Aggregation below retains stdout and stderr for both success and failure.
    // The outer caller still completes both DPRs after any individual failure.
    // Performance numbers are never inferred from the watchdog duration.
    let filters: &[&str] = if workload.is_some() {
        &[
            "real_workloads_chinese_cat",
            "real_workloads_japanese_cat",
            "real_workloads_korean_cat",
            "real_workloads_directory_names",
            "real_workloads_nvim_scrolling",
            "real_workloads_tmux_scripts",
            "real_workloads_chat_log",
            "real_workloads_man_scrolling",
            // Pure clock and real Canvas readiness witnesses exercise this same lane.
            "tasks_share_one_refresh_and_late_callbacks_recover",
            "background_draw_enables_later_first_frame_canvas_ink",
            // Validation preserves every scalar while reusing publication pixels.
            "device_sized_validation_reuses_ink_and_scalar_facts",
            "mixed_cluster_rejects_missing_scalar_and_remembers_negative",
            "shared_readback_clears_old_ink_and_transform",
            // Paired reusable surfaces isolate the requested readback hint cost.
            "readback_hint_cost_on_reused_canvas",
        ]
    } else {
        &[""]
    };
    let mut text = String::new();
    let mut passed = true;
    for filter in filters {
        // Keep every failed workload's output and continue collecting the others.
        let (output, success) = chrome_one(url, flood, dpr, workload, filter)?;
        text.push_str(&output);
        passed &= success;
    }
    std::fs::write(format!("target/m7-browser-dpr-{dpr}.log"), &text).map_err(|e| e.to_string())?;
    Ok((text, passed))
}

/// One runner invocation retains the existing watchdog and owns one Chrome session.
fn chrome_one(
    url: &str,
    flood: &str,
    dpr: u32,
    workload: Option<&str>,
    filter: &str,
) -> Result<(String, bool), String> {
    // Capabilities are runner input, not a shipped browser setting or asset.
    let capabilities = format!("target/m7-browser-dpr-{dpr}.json");
    std::fs::write(
        &capabilities,
        format!(r#"{{"goog:chromeOptions":{{"args":["--force-device-scale-factor={dpr}"]}}}}"#),
    )
    .map_err(|e| e.to_string())?;
    let capabilities = std::fs::canonicalize(capabilities).map_err(|e| e.to_string())?;
    // Release mode measures the actual optimized wasm paths rather than debug instruction cost.
    let mut command = Command::new("python3");
    command
        // The proxy supplies trusted W3C presses to this same runner-owned session.
        .arg("xtask/scripts/browser-actions.py")
        .args([
            "test",
            "--release",
            "--headless",
            "--chrome",
            "crates/puzzle-web",
            "--",
            "--nocapture",
        ])
        .env("WASM_BINDGEN_TEST_ADDRESS", "127.0.0.1:8088")
        .env("WASM_BINDGEN_TEST_WEBDRIVER_JSON", capabilities)
        .env("PUZZLE_TEST_BRIDGE_URL", url)
        .env("PUZZLE_TEST_FLOOD_PATH", flood)
        .env(
            "PUZZLE_TEST_UNICODE_SCRIPT",
            std::fs::canonicalize("target/m11-unicode/producer.py").map_err(|e| e.to_string())?,
        );
    if let Some(script) = workload {
        // The optional fixture is test-only; normal package builds know no local paths.
        command
            .env("PUZZLE_TEST_WORKLOAD_SCRIPT", script)
            .arg(filter);
    } else {
        // Clearing inherited diagnostics keeps an ordinary acceptance run reproducible.
        command.env_remove("PUZZLE_TEST_WORKLOAD_SCRIPT");
    }
    let output = command.output().map_err(|e| e.to_string())?;
    // Strip the private token if a driver exception ever includes the connection URL.
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .replace(url, "[private bridge URL]");
    // Separate files keep scale-specific failures and measurements easy to inspect.
    std::fs::write(format!("target/m7-browser-dpr-{dpr}.log"), &text).map_err(|e| e.to_string())?;
    Ok((text, output.status.success()))
}

/// Reads only the private startup protocol; neither failure messages nor logs include the token.
fn private_url(server: &mut Server) -> Result<String, String> {
    // Only the test process receives the secret startup line. Errors never include it.
    let stdout = server
        .0
        .stdout
        .take()
        .ok_or("bridge startup pipe missing")?;
    let mut lines = BufReader::new(stdout).lines();
    let _warning = lines
        .next()
        .ok_or("bridge warning missing")?
        .map_err(|e| e.to_string())?;
    let url = lines
        .next()
        .ok_or("bridge startup URL missing")?
        .map_err(|e| e.to_string())?;
    if !url.starts_with("ws://127.0.0.1:") {
        return Err("bridge test address is not loopback".into());
    }
    Ok(url)
}

/// Exactly 100 MB of truecolor animation lets cat outrun real browser parsing.
fn flood_fixture() -> Result<String, String> {
    // The ignored target directory keeps the large generated file out of commits.
    let path = std::path::Path::new("target/m7-browser-flood.vt");
    // Reuse the deterministic truecolor corpus already approved for native flood tests.
    let source = fs::read("assets/fixtures/corpus/flood.vt").map_err(|e| e.to_string())?;
    // A zero-length record would leave the repetition loop unable to make progress.
    if source.is_empty() {
        return Err("empty browser flood corpus".into());
    }
    // Replace older generated input so this run never hashes an unrelated file.
    let mut file = File::create(path).map_err(|e| e.to_string())?;
    let mut left = 100_000_000;
    while left > 0 {
        // A partial last record is intentional: byte-for-byte checks include it.
        let count = left.min(source.len());
        file.write_all(&source[..count])
            .map_err(|e| e.to_string())?;
        left -= count;
    }
    // The bridge shell starts at home, so cat must receive an absolute local path.
    path.canonicalize()
        .map_err(|e| e.to_string())?
        .into_os_string()
        .into_string()
        .map_err(|_| "flood path is not UTF-8".into())
}
