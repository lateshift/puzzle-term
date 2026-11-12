//! Unicode discovery must yield to real Chrome submissions and real PTY input.
use super::*;
use wasm_bindgen_test::console_log;
/// Cold fallback uses the same 46,607-scalar producer as the native window gate.
#[wasm_bindgen_test(async)]
async fn unicode_flood_presents_and_key_reaches_child() {
    let _fonts = resources::Fonts::new();
    let (terminal, host) = resources::create(14).await;
    terminal.resize(80.into(), 40.into()).unwrap();
    let options = js_sys::Object::new();
    js_sys::Reflect::set(&options, &"protocol".into(), &"puzzle-bridge-v1".into()).unwrap();
    let connection = terminal
        .connect(
            option_env!("PUZZLE_TEST_BRIDGE_URL").unwrap().into(),
            Some(options.into()),
        )
        .unwrap();
    let script = option_env!("PUZZLE_TEST_UNICODE_SCRIPT")
        .unwrap()
        .replace('\'', "'\"'\"'");
    terminal
        .input(format!(
            "stty raw -echo; python3 -u '{script}'; stty sane\r"
        ))
        .unwrap();
    wait(&terminal, |r| r.session.buffer().title() == "unicode-ready").await;
    terminal.owner.borrow_mut().measurements = super::super::measurements::Measurements {
        active: true,
        ..Default::default()
    };
    let start = terminal.owner.borrow().started.elapsed();
    terminal.input("g".into()).unwrap();
    // Keep the first second cold: discovery and raster preparation happen during the measured flood.
    socket::delay(1000).await;
    let issued = issue_key(&terminal);
    wait(&terminal, |r| {
        r.session.buffer().title().starts_with("unicode-key-")
    })
    .await;
    let receipt = terminal
        .owner
        .borrow()
        .session
        .buffer()
        .title()
        .strip_prefix("unicode-key-")
        .unwrap()
        .parse::<f64>()
        .unwrap()
        / 1e6;
    let key_ms = receipt - issued;
    socket::delay(4800).await;
    let end = terminal.owner.borrow().started.elapsed();
    let result = check(&terminal, start, end, key_ms);
    connection.disconnect();
    terminal.dispose().unwrap();
    host.remove();
    assert!(result.is_ok(), "{}", result.unwrap_err());
}
/// Bounded observation fails on a stuck renderer rather than accepting a plausible final grid.
async fn wait(terminal: &PuzzleTerminal, predicate: impl Fn(&Runtime) -> bool) {
    let start = Instant::now();
    while !predicate(&terminal.owner.borrow()) {
        assert!(
            start.elapsed().as_secs() < 20,
            "Unicode child wait timed out"
        );
        socket::delay(5).await;
    }
}
/// Every submission and the complete interval contribute to the unchanged acceptance budgets.
fn check(
    terminal: &PuzzleTerminal,
    start: std::time::Duration,
    end: std::time::Duration,
    key_ms: f64,
) -> Result<(), String> {
    let r = terminal.owner.borrow();
    let frames: Vec<_> = r
        .measurements
        .frames
        .iter()
        .filter(|at| **at >= start && **at <= end)
        .collect();
    let fps = frames.len() as f64 / (end - start).as_secs_f64();
    let max_gap = frames
        .windows(2)
        .map(|p| (*p[1] - *p[0]).as_secs_f64() * 1000.0)
        .fold(0.0, f64::max);
    let text = r.text.as_ref().unwrap();
    let context = text.context.borrow();
    let raster_bytes = context.as_ref().unwrap().raster_bytes();
    let stats = text.fallback_stats.borrow();
    let index_us: usize = stats
        .iter()
        .map(|s| s.coverage_us.load(std::sync::atomic::Ordering::Relaxed))
        .sum();
    let slots: usize = stats
        .iter()
        .map(|s| s.slots.load(std::sync::atomic::Ordering::Relaxed))
        .sum();
    console_log!(
        "M11_UNICODE idle_index_ms={} scalar_slots={slots} index_bytes={}",
        index_us as f64 / 1000.0,
        stats.len() * 0x110000 * 8 / 4
    );
    console_log!(
        "M11_UNICODE fps={fps:.3} key_to_child_ms={key_ms:.3} max_gap_ms={max_gap:.3} bytes={} raster_bytes={raster_bytes}",
        r.measurements.received
    );
    // Return an error after dropping all borrows; cleanup must also run on a failed budget.
    if fps < 30.0 {
        return Err(format!("Unicode actual submissions {fps} fps < 30"));
    }
    if !(0.0..=50.0).contains(&key_ms) {
        return Err(format!("Unicode key to child {key_ms} ms > 50"));
    }
    if raster_bytes == 0 {
        return Err("covered emoji never replaced provisional tofu".into());
    }
    if r.measurements.pending_peak > 4 * 1024 * 1024 {
        return Err("Unicode pending bytes exceed 4 MiB".into());
    }
    Ok(())
}

/// A DOM Ctrl+A travels through keyboard mapping and the live bridge to the raw PTY child.
fn issue_key(terminal: &PuzzleTerminal) -> f64 {
    let textarea = terminal
        .owner
        .borrow()
        .dom
        .as_ref()
        .unwrap()
        .textarea()
        .clone();
    let init = web_sys::KeyboardEventInit::new();
    init.set_key("a");
    init.set_code("KeyA");
    init.set_ctrl_key(true);
    init.set_cancelable(true);
    let event =
        web_sys::KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init).unwrap();
    let issued = js_sys::Date::now();
    textarea.dispatch_event(&event).unwrap();
    issued
}
