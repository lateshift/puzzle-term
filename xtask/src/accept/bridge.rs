//! Real-socket bridge contracts, scheduled with browser closure gates (M16.6a/U2).
//!
//! The server's library and CLI use the same listener and session workers.
//! Exact test-name evidence prevents Cargo's successful empty filter from
//! masquerading as security or byte-integrity verification. Each criterion
//! reports independently, including failures that follow another failure.
//!
//! No test reads a user's startup file or home-directory content. Synthetic
//! shell commands cover PTY behavior and temporary files hold the byte corpus.
//! The credential/Origin negatives need successful authenticated peers too:
//! otherwise a server rejecting every client would satisfy only the negatives.
//! Transport witnesses establish that positive half of the contract.
//! The 100 MB stream is binary and includes control bytes, so an accidental
//! cooked PTY or text conversion fails its checksum as well as byte count.
//! Keeping no-ACK and resume evidence in one transfer rules out a restart
//! that merely produces the same final byte count after losing earlier bytes.
//! Exit ordering is checked on the wire, not inferred from child status:
//! a successful wait alone says nothing about whether its last read was sent.
use super::{BrowserGate, m7::test, report};

/// Runs a named integration test through the production loopback listener.
fn socket(target: &str, name: &str) -> Result<(), String> {
    let output = crate::commands::capture(
        "cargo",
        &[
            "test",
            "-p",
            "puzzle-pty-bridge",
            "--test",
            target,
            name,
            "--",
            "--exact",
            // These tests stay listed by Cargo, but U2 runs their real sockets
            // only here. Without this flag an exact ignored match would do no work.
            "--ignored",
            // Preserve successful connect/upgrade clocks, normally hidden by libtest.
            "--show-output",
        ],
    )?;
    // Unlike mocked credentials, these witnesses make actual HTTP upgrades.
    // They also create real PTYs, so sandbox denial remains a visible failure.
    if !output.contains(&format!("test {name} ... ok")) {
        return Err(format!("bridge witness {target}::{name} did not run"));
    }
    // These observer records explain firewall/startup delays without changing
    // the handshake deadline or any security/flow assertion.
    for line in output
        .lines()
        .filter(|line| line.contains("test client upgrade timing"))
    {
        println!("{line}");
    }
    Ok(())
}

/// Every security row is independent: authorization is a conjunction of guards.
const SECURITY: &[(&str, &str)] = &[
    // Checking the OS address prevents a CLI default from concealing a wildcard bind.
    ("bridge binds only 127.0.0.1", "listener_is_loopback"),
    // An allowed page still needs the token; knowing the token still needs the Origin.
    ("bridge rejects missing token", "missing_token_rejected"),
    // Equal-length wrong credentials protect against prefix comparison mistakes.
    ("bridge rejects wrong token", "wrong_token_rejected"),
    // Duplicate/missing headers get a separate pure callback witness in codec().
    (
        "bridge requires an exact allowed Origin",
        "bad_origin_rejected",
    ),
    // Positive peers protect against falsely passing every negative by denying all upgrades.
    (
        "bridge accepts both development origins",
        "default_origins_succeed",
    ),
    // Idle upgrades count toward the quota, and HTTP alone must not spawn a child.
    (
        "bridge limits sessions to four and waits for hello",
        "fifth_session_and_prehello",
    ),
    // Wrong versions and forged ACK credit cannot reach native PTY state.
    (
        "bridge rejects malformed controls and forged ACKs",
        "malformed_protocol_closes",
    ),
    // Cleanup must signal a real child, not merely drop a WebSocket wrapper.
    ("bridge disconnect sends SIGHUP", "disconnect_sends_sighup"),
];

/// The shared native/wasm codec is exercised without opening sockets or files.
fn codec(passed: &mut bool) {
    for (label, name) in [
        ("bridge v1 literal codec round trips", "wire_round_trip"),
        // Every possible u32 grant must survive the new exact-length LE tag.
        (
            "bridge input credit round trip property",
            "input_credit_round_trip",
        ),
        // Missing fields and hidden suffixes must not become partial native controls.
        (
            "bridge v1 rejects malformed lengths and dimensions",
            "malformed_controls_rejected",
        ),
        // Hostile payloads exercise both directions without launching native services.
        (
            "bridge arbitrary frame decoder never panics",
            "arbitrary_frames_never_panic",
        ),
    ] {
        // puzzle-term is also compiled for wasm by the common native/wasm gate.
        // The standalone server compiles this exact pure source, without core deps.
        report(
            label,
            test("puzzle-term", &format!("bridge::tests::{name}")),
            passed,
        );
    }
    // Generated tokens must carry 16 OS-random bytes and never use a run counter.
    // Entropy failure is propagated before listener creation by the server.
    report(
        "bridge rejects missing/duplicate Origin and ambiguous URLs",
        test(
            "puzzle-pty-bridge",
            "security::tests::ambiguous_metadata_rejected",
        ),
        passed,
    );
    report(
        "bridge uses random 128-bit tokens",
        test(
            "puzzle-pty-bridge",
            "security::tests::random_128_bit_tokens",
        ),
        passed,
    );
}

/// Wire order, native state and recordings stay independently inspectable.
fn transport(browser: BrowserGate, passed: &mut bool) {
    for (label, name) in [
        (
            "bridge drains done before exit code 3",
            "output_before_exit",
        ),
        // Identity and initial appearance influence ncurses and vim before any prompt.
        (
            "bridge child sees xterm-puzzle, Sync and appearance",
            "identity_and_sync",
        ),
        // Wire row/column order differs from stty output; reversal is easy to miss.
        (
            "bridge input and physical resize reach the PTY",
            "input_and_resize",
        ),
        // Replay must retain the raw control stream and the intentional child delay.
        (
            "bridge records LE delay/length/raw output",
            "recording_raw_output",
        ),
    ] {
        // A fresh listener and synthetic shell isolate these from user dotfiles.
        // Temporary captures are removed and never become committed user data.
        browser.report(label, || socket("transport", name), passed);
    }
}

/// Adds only M7.4 contracts; DOM and JS transport tests belong to later tasks.
pub(super) fn run(passed: &mut bool) {
    codec(passed);
    closure(BrowserGate::Full, passed);
}

/// Closure-only real sockets/PTYs; task skips name every deferred witness (U2).
pub(super) fn closure(browser: BrowserGate, passed: &mut bool) {
    for (label, name) in SECURITY {
        browser.report(label, || socket("security", name), passed);
    }
    // One complete transfer supplies both observations: cat remains blocked
    // without ACK, then all 100 MB resume with the position-sensitive checksum.
    // Report both criteria even if the shared witness fails at an earlier step.
    let flow = match browser {
        BrowserGate::Full => socket("flow", "no_ack_blocks_and_100mb_resumes"),
        // No listener or child is created by an explicit task-level skip.
        BrowserGate::TaskSkip => Ok(()),
    };
    browser.report(
        "bridge no-ACK backpressure blocks cat near 1 MiB",
        || flow.clone(),
        passed,
    );
    browser.report(
        "bridge ACK resumes 100 MB without byte loss",
        || flow,
        passed,
    );
    // Socket controls remain eligible even when every granted input byte is
    // stuck behind the child. Echoing cat fills both directions, unlike a
    // redirected cat, so it exposes the original circular-wait defect.
    for (label, name) in [
        // A periodic idle timer can appear responsive while violating P2.
        (
            "bridge idle session and listener have zero periodic turns",
            "idle_session_has_zero_periodic_turns",
        ),
        // Binary equality detects translation as well as deadlock and loss.
        (
            "bridge credited 10 MB echo has exact FIFO bytes and no stall",
            "echoing_10mb_paste_completes_in_order",
        ),
        // Canonical input and line editing exercise a real shell, not raw cat.
        (
            "bridge credited paste reaches interactive shell line editor",
            "interactive_shell_line_paste_completes",
        ),
        // A key queued after a paste must never overtake its accepted prefix.
        (
            "bridge stalled input preserves 10 MB plus key in FIFO order",
            "unread_10mb_input_backpressures_without_disconnect",
        ),
        // Native counters witness the controls before the child starts reading.
        (
            "bridge stalled input leaves ACK and resize immediately readable",
            "unread_10mb_input_backpressures_without_disconnect",
        ),
        // A protocol error alone is insufficient if its child is left running.
        (
            "bridge excess input credit reports error, closes and cleans child",
            "overspending_credit_errors_and_cleans_child",
        ),
    ] {
        // Each witness uses a new listener and neutral temporary evidence;
        // controls and FIFO integrity remain separately reported criteria.
        browser.report(label, || socket("input", name), passed);
    }
    transport(browser, passed);
}

#[cfg(test)]
/// Explicit task skips must leave every real witness visibly deferred.
mod tests;
