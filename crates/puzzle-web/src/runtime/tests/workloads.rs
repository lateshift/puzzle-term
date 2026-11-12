//! Release Chrome observations run real local commands through the authenticated PTY.
//!
//! Generated Unicode content and the local mandoc source stay under target/.
//! No fetch mock, synthetic glyph completion or diagnostic redraw is used.
use super::*;
use std::sync::atomic::Ordering;
use wasm_bindgen_test::console_log;

/// Separate tests keep the existing per-test timeout while retaining every workload.
macro_rules! workload {
    ($name:ident, $mode:literal) => {
        #[wasm_bindgen_test(async)]
        async fn $name() {
            let Some(script) = option_env!("PUZZLE_TEST_WORKLOAD_SCRIPT") else {
                return;
            };
            assert!(
                observe($mode, script).await,
                "workload failed; raw report retained"
            );
        }
    };
}
workload!(real_workloads_chinese_cat, "cat-zh");
workload!(real_workloads_japanese_cat, "cat-ja");
workload!(real_workloads_korean_cat, "cat-ko");
workload!(real_workloads_directory_names, "ls");
workload!(real_workloads_nvim_scrolling, "nvim");
workload!(real_workloads_tmux_scripts, "tmux");
workload!(real_workloads_chat_log, "chat");
workload!(real_workloads_man_scrolling, "man");

/// Each workload gets a fresh text context, preserving its first-family cold call.
async fn observe(mode: &str, script: &str) -> bool {
    let (terminal, host) = resources::create(10).await;
    terminal.resize(100.into(), 36.into()).unwrap();
    let options = js_sys::Object::new();
    js_sys::Reflect::set(&options, &"protocol".into(), &"puzzle-bridge-v1".into()).unwrap();
    let connection = terminal
        .connect(
            option_env!("PUZZLE_TEST_BRIDGE_URL").unwrap().into(),
            Some(options.into()),
        )
        .unwrap();
    let key = format!("{script}.{mode}.key");
    // Absolute paths work from the bridge's home-directory shell too.
    let quote = |s: &str| s.replace('\'', "'\"'\"'");
    terminal
        .input(format!(
            "stty raw -echo; PUZZLE_WORKLOAD='{mode}' python3 -u '{}' '{}'; stty sane\r",
            quote(script),
            quote(&key)
        ))
        .unwrap();
    wait(&terminal, |r| r.session.buffer().title() == "unicode-ready").await;
    activate(&terminal);
    let start = terminal.owner.borrow().started.elapsed();
    terminal.input("g".into()).unwrap();
    socket::delay(1000).await;
    let issued = key_press(&terminal);
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
        .trim_start_matches("unicode-key-")
        .parse::<f64>()
        .unwrap()
        / 1e6;
    socket::delay(5000).await;
    let end = terminal.owner.borrow().started.elapsed();
    let passed = report(&terminal, mode, end - start, receipt - issued);
    connection.disconnect();
    terminal.dispose().unwrap();
    host.remove();
    passed
}

/// Test observers are inactive in normal browser sessions and allocate only on opt-in.
fn activate(terminal: &PuzzleTerminal) {
    let mut r = terminal.owner.borrow_mut();
    r.measurements = super::super::measurements::Measurements {
        active: true,
        ..Default::default()
    };
    let text = r.text.as_ref().unwrap();
    text.context
        .borrow_mut()
        .as_mut()
        .unwrap()
        .measure_completions();
    for stats in text.fallback_stats.borrow().iter() {
        stats.active.store(true, Ordering::Relaxed);
    }
}

/// Ctrl+D travels through the ordinary DOM adapter and is acknowledged by the real child.
fn key_press(terminal: &PuzzleTerminal) -> f64 {
    let textarea = terminal
        .owner
        .borrow()
        .dom
        .as_ref()
        .unwrap()
        .textarea()
        .clone();
    let init = web_sys::KeyboardEventInit::new();
    init.set_key("d");
    init.set_code("KeyD");
    init.set_ctrl_key(true);
    init.set_cancelable(true);
    let event =
        web_sys::KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init).unwrap();
    let at = js_sys::Date::now();
    textarea.dispatch_event(&event).unwrap();
    at
}

/// Bounded child supervision is a test timeout, never a glyph-work tuning parameter.
async fn wait(terminal: &PuzzleTerminal, predicate: impl Fn(&Runtime) -> bool) {
    let start = Instant::now();
    while !predicate(&terminal.owner.borrow()) {
        assert!(
            start.elapsed().as_secs() < 20,
            "real workload child wait timed out"
        );
        socket::delay(5).await;
    }
}

/// JSON evidence includes raw samples, so slow calls and removed glyphs stay visible.
fn report(
    terminal: &PuzzleTerminal,
    mode: &str,
    elapsed: std::time::Duration,
    key_ms: f64,
) -> bool {
    let r = terminal.owner.borrow();
    let m = &r.measurements;
    let mut latency: Vec<_> = m.ink.samples.iter().map(|s| s.0).collect();
    latency.sort_unstable_by(f64::total_cmp);
    let p95 = latency
        .get((latency.len() * 95).div_ceil(100).saturating_sub(1))
        .copied()
        .unwrap_or_default();
    let max = latency.last().copied().unwrap_or_default();
    let calls: Vec<_> = r
        .text
        .as_ref()
        .unwrap()
        .fallback_stats
        .borrow()
        .iter()
        .flat_map(|s| s.calls.lock().unwrap().clone())
        .collect();
    let events: Vec<_> = r
        .text
        .as_ref()
        .unwrap()
        .fallback_stats
        .borrow()
        .iter()
        .flat_map(|s| s.events.lock().unwrap().clone())
        .collect();
    console_log!(
        "D39_SOURCE {{\"workload\":\"{mode}\",\"events\":{events:?},\"ink\":{:?}}}",
        m.ink.details
    );
    let missed: Vec<_> = m.turns.iter().filter(|t| t[6] > t[5]).collect();
    let admission_missed = missed.iter().filter(|t| t[3] > 0.0).count();
    let frames: Vec<_> = m.frames.iter().map(|t| t.as_secs_f64() * 1000.0).collect();
    let delays: Vec<_> = m.ink.samples.iter().map(|s| s.1).collect();
    console_log!(
        "D39_CHROME {{\"workload\":\"{mode}\",\"max_ms\":{max},\"p95_ms\":{p95},\"key_ms\":{key_ms},\"fps\":{},\"unresolved\":{},\"censored\":{},\"admission_missed\":{admission_missed},\"missed\":{},\"frames\":{frames:?},\"latency_ms\":{latency:?},\"latency_frames\":{delays:?},\"turns\":{:?},\"canvas_calls\":{calls:?}}}",
        frames.len() as f64 / elapsed.as_secs_f64(),
        m.ink.unresolved(),
        m.ink.censored,
        missed.len(),
        m.turns
    );
    // Keep failed evidence intact; observations do not weaken any acceptance bound.
    m.ink.unresolved() == 0 && admission_missed == 0 && (0.0..=50.0).contains(&key_ms)
}
