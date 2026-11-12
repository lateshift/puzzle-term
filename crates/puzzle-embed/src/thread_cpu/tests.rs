//! CLOCK_THREAD_CPUTIME_ID must exclude time when the caller is not executing.
use super::*;

#[test]
fn cpu_clock_charges_work_but_not_sleep() -> Result<(), String> {
    let started = now().ok_or("supported Unix CPU clock unavailable")?;
    let busy = std::time::Instant::now();
    while busy.elapsed() < Duration::from_millis(5) {
        std::hint::black_box((1_u64..128).sum::<u64>());
    }
    assert!(elapsed(Some(started)).ok_or("same-thread CPU delta unavailable")? > Duration::ZERO);
    let started = now();
    let wall = std::time::Instant::now();
    std::thread::sleep(Duration::from_millis(30));
    let cpu = elapsed(started).ok_or("same-thread CPU delta unavailable")?;
    // An elapsed wall clock accidentally used here charges all thirty milliseconds.
    assert!(cpu < wall.elapsed() / 2, "sleep charged CPU: {cpu:?}");
    Ok(())
}
