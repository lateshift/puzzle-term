//! Browser measurements cross mounted DOM, the production transport and rAF.
//!
//! Synthetic key construction is outside timing; the timer ends inside the
//! attached transport's write callback, after mapping, policy and encoding.
//! The flood witness uses an exact 100 MB local file and actual bridge output.
use super::*;
use wasm_bindgen_test::console_log;

/// A finite wait never prints a private bridge URL or silently skips a witness.
async fn wait(terminal: &PuzzleTerminal, predicate: impl Fn(&Runtime) -> bool) {
    let start = Instant::now();
    while !predicate(&terminal.owner.borrow()) {
        assert!(start.elapsed().as_secs() < 20, "measurement timed out");
        socket::delay(5).await;
    }
}

/// Mapping, keybindings, encoding and host delivery all precede the timed endpoint.
#[wasm_bindgen_test(async)]
async fn dom_key_to_transport_budget() {
    let _fonts = resources::Fonts::new();
    let (terminal, parent) = resources::create(14).await;
    let start = Rc::new(RefCell::new(Instant::now()));
    let samples = Rc::new(RefCell::new(Vec::<f64>::new()));
    let clock = start.clone();
    let times = samples.clone();
    let callback = Closure::wrap(Box::new(move |bytes: js_sys::Uint8Array| {
        let elapsed = clock.borrow().elapsed().as_secs_f64() * 1000.0;
        // ArrowUp must be real encoded input, never an empty shortcut outcome.
        assert_eq!(bytes.to_vec(), b"\x1b[A");
        times.borrow_mut().push(elapsed);
    }) as Box<dyn FnMut(js_sys::Uint8Array)>);
    let transport = js_sys::Object::new();
    js_sys::Reflect::set(&transport, &"write".into(), callback.as_ref()).unwrap();
    let handle = terminal.attach_transport(transport.into()).unwrap();
    let textarea = terminal
        .owner
        .borrow()
        .dom
        .as_ref()
        .unwrap()
        .textarea()
        .clone();
    let init = web_sys::KeyboardEventInit::new();
    init.set_key("ArrowUp");
    init.set_code("ArrowUp");
    init.set_cancelable(true);
    let key = web_sys::KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init).unwrap();
    for _ in 0..351 {
        // Browser event construction and measurement formatting are outside this span.
        *start.borrow_mut() = Instant::now();
        textarea.dispatch_event(&key).unwrap();
    }
    let metrics = latency(&samples.borrow()[50..]);
    console_log!(
        "M7_METRIC web_key_median_ms={:.6} web_key_p99_ms={:.6} web_key_max_ms={:.6}",
        metrics[0],
        metrics[1],
        metrics[2]
    );
    assert_eq!(
        samples.borrow().len(),
        351,
        "every press must reach the transport"
    );
    assert!(metrics[2] <= 0.5, "key maximum {} ms > 0.5", metrics[2]);
    handle.dispose();
    terminal.dispose().unwrap();
    parent.remove();
}

/// Keep the whole measured range: report median/p99 and enforce the observed maximum.
fn latency(samples: &[f64]) -> [f64; 3] {
    let mut samples = samples.to_vec();
    samples.sort_by(f64::total_cmp);
    [
        samples[samples.len() / 2],
        samples[(samples.len() - 1) * 99 / 100],
        samples[samples.len() - 1],
    ]
}

/// A real cat cannot conceal dropped output behind a plausible final screen.
#[wasm_bindgen_test(async)]
async fn bridge_100mb_keeps_presenting_and_input_flows() {
    let Some(url) = option_env!("PUZZLE_TEST_BRIDGE_URL") else {
        panic!("run cargo xtask web test");
    };
    let Some(path) = option_env!("PUZZLE_TEST_FLOOD_PATH") else {
        panic!("flood fixture missing");
    };
    let _fonts = resources::Fonts::new();
    let (terminal, parent) = resources::create(14).await;
    parent.style().set_property("height", "720px").unwrap();
    terminal.fit().unwrap();
    terminal.resize(80.into(), 40.into()).unwrap();
    let options = js_sys::Object::new();
    js_sys::Reflect::set(&options, &"protocol".into(), &"puzzle-bridge-v1".into()).unwrap();
    let connection = terminal.connect(url.into(), Some(options.into())).unwrap();
    terminal
        .input("stty -echo; PS1=; PS2=; printf '\\033]2;flood-ready\\007'\r".into())
        .unwrap();
    wait(&terminal, |r| r.session.buffer().title() == "flood-ready").await;
    // Start checksum and submissions after initialization/prompt output is fully parsed.
    terminal.owner.borrow_mut().measurements = super::super::measurements::Measurements {
        active: true,
        ..Default::default()
    };
    let started = terminal.owner.borrow().started.elapsed();
    // Quote the generated path even if a checkout directory contains an apostrophe.
    let path = path.replace('\'', "'\"'\"'");
    terminal.input(format!("cat '{path}'; printf '\\033]2;flood-done\\007'; IFS= read -r key; printf '\\033]2;flood-%s\\007' \"$key\"\r").into()).unwrap();
    wait(&terminal, |r| r.measurements.received >= 1_000_000).await;
    // Queue during cat: the shell reads it after the burst, preserving normal FIFO.
    let key_start = Instant::now();
    terminal.input("key\r".into()).unwrap();
    let key_ms = key_start.elapsed().as_secs_f64() * 1000.0;
    wait(&terminal, |r| r.session.buffer().title() == "flood-key").await;
    let ended = terminal.owner.borrow().started.elapsed();
    flood_assertions(&terminal, started, ended, key_ms);
    assert_eq!(terminal.pending_bytes().unwrap(), 0.0);
    connection.disconnect();
    terminal.dispose().unwrap();
    parent.remove();
}

/// Exact byte/checksum and submitted-frame budgets use the same measured interval.
fn flood_assertions(
    terminal: &PuzzleTerminal,
    started: std::time::Duration,
    ended: std::time::Duration,
    key_ms: f64,
) {
    let runtime = terminal.owner.borrow();
    let m = &runtime.measurements;
    let suffix = b"\x1b]2;flood-done\x07\x1b]2;flood-key\x07";
    let source = include_bytes!("../../../../../assets/fixtures/corpus/flood.vt");
    let expected = puzzle_term::bridge::checksum::fnv1a_update(
        puzzle_term::bridge::checksum::FNV1A_BASIS,
        source
            .iter()
            .cycle()
            .take(100_000_000)
            .chain(suffix)
            .copied(),
    );
    let frames: Vec<_> = m
        .frames
        .iter()
        .copied()
        .filter(|at| *at >= started && *at <= ended)
        .collect();
    let seconds = (ended - started).as_secs_f64();
    let fps = frames.len() as f64 / seconds;
    let max_gap = frames
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).as_secs_f64() * 1000.0)
        .fold(0.0, f64::max);
    console_log!(
        "M7_METRIC flood_bytes={} flood_checksum={} flood_seconds={:.3} flood_frames={} flood_fps={:.3} flood_max_gap_ms={:.3} flood_key_ms={:.6} flood_pending_peak={}",
        m.received,
        m.checksum,
        seconds,
        frames.len(),
        fps,
        max_gap,
        key_ms,
        m.pending_peak
    );
    assert_eq!(
        m.received,
        100_000_000 + suffix.len() as u64,
        "100 MB plus exact completion markers"
    );
    assert_eq!(m.checksum, expected, "lossless ordered browser stream");
    assert!(
        frames.len() >= 2 && fps >= 30.0,
        "actual submissions {fps} fps < 30"
    );
    assert!(key_ms <= 50.0, "input enqueue {key_ms} ms > 50");
    assert!(
        m.pending_peak <= 4 * 1024 * 1024,
        "unbounded Session suffix"
    );
}
