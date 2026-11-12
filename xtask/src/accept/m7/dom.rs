//! Browser input contracts through the neutral Session before DOM attachment.
//!
//! These witnesses need no browser or GPU. Provisional composition must not
//! become shell bytes, and focus reports must retain their place in input FIFO.
//! The browser witnesses dispatch actual events through mounted listeners.
use super::super::report;

/// Rejects a missing witness, using the same exact-test guard as other M7 tasks.
pub(super) fn core(passed: &mut bool) {
    // Keep the two command contracts separate: a focus regression should
    // still report even when a composition regression failed first.
    for (criterion, name) in [
        (
            "preedit remains UI state until a committed Text command",
            "preedit_is_not_application_input",
        ),
        (
            "typed focus reports preserve input FIFO without a frame",
            "focus_reports_share_input_fifo",
        ),
    ] {
        report(
            criterion,
            super::test("puzzle-term", &format!("session::tests::dom::{name}")),
            passed,
        );
    }
}

/// Executes actual browser event constructors/listeners, not native simulations.
/// Capturing one run keeps Chrome startup out of individual criterion timings.
/// A missing exact name fails even when a renamed test leaves the suite green.
pub(super) fn browser(passed: &mut bool) {
    let output = crate::browser::suite();
    // The same run supplies budget assertions and actual frame/credit evidence.
    super::measurements::browser(&output, passed);
    for &(criterion, name) in BROWSER.iter().chain(RUNTIME) {
        // The output guard proves every listed contract executed in Chrome;
        // wasm-pack also exits successfully when a test filter matches nothing.
        let result = output.as_ref().map_err(Clone::clone).and_then(|output| {
            // The full suite runs at 1× and 2×; neither scale may omit a witness.
            if output.matches(&format!("test {name} ... ok")).count() == 2 {
                Ok(())
            } else {
                Err(format!("browser witness {name} did not run"))
            }
        });
        report(criterion, result, passed);
    }
}

/// Every row names a behavior witness run in the browser, not an empty filter.
const BROWSER: &[(&str, &str)] = &[
    // Physical US identity must remain distinct from German produced text.
    // Legacy keys, numeric-pad decimal and requested releases share the mapper.
    (
        "DOM key table, keypad and kitty alternates",
        "dom::keyboard::tests::key_table_and_kitty_alternates",
    ),
    // Both Option sides can have different policies within one held gesture.
    // Missing getLayoutMap cannot cause a configured Alt chord to disappear.
    (
        "DOM Option sides with exact layout map and physical-US fallback",
        "dom::keyboard::tests::option_sides_with_and_without_layout_map",
    ),
    // Browser chrome commands remain native unless fullscreen lock is granted.
    // A reserved user binding must be reported by the shared validation path.
    (
        "DOM browser-reserved shortcut validation",
        "dom::keyboard::tests::browser_reserved_shortcuts",
    ),
    // Browsers disagree on whether final input precedes or follows compositionend.
    // Both orders must deliver one committed string, never provisional stdin.
    (
        "DOM composition commit orders",
        "dom::composition::tests::composition_commit_orders",
    ),
    // Cancellation clears only UI state; a UTF-16 caret cannot split UTF-8.
    // Non-BMP candidates expose incorrect scalar/unit offset assumptions.
    (
        "DOM composition cancellation and checked Unicode caret",
        "dom::composition::tests::composition_cancel_and_unicode_caret",
    ),
    // Mounted listeners must honor the transaction state, not just pure helpers.
    // Retaining a detached textarea cannot keep its old command lane alive.
    (
        "mounted composition commits once and listeners dispose",
        "dom::tests::mounted_composition_and_disposal",
    ),
    // User paste reads event clipboardData without permission or async prompting.
    // Canceling default insertion avoids a second input event typing it twice.
    (
        "DOM paste reads clipboardData once",
        "dom::tests::mounted_paste_from_event",
    ),
    // Non-passive registration is required for an effective preventDefault.
    // Disabled tracking and local Shift leave page scrolling untouched.
    (
        "DOM wheel prevents default only when consumed",
        "dom::tests::mounted_wheel_default_ownership",
    ),
    // SGR cells and physical pixel reports originate from the same padded geometry.
    // DOM button bits must preserve middle-click identity and consecutive clicks.
    (
        "DOM pointer device geometry and click counts",
        "dom::tests::pointer_device_geometry",
    ),
    // High-rate movement schedules one lazy rAF report, with its latest position.
    // Disposal cancels pending work rather than calling a removed Session later.
    (
        "DOM motion coalesces once per rAF and cancels on dispose",
        "dom::tests::pointer_motion_is_coalesced",
    ),
];

/// The minimal API has independent lifecycle, parsing, input and resource witnesses.
const RUNTIME: &[(&str, &str)] = &[
    // Real HTTP font timing and per-cell ink must not be hidden by a whole-canvas count.
    (
        "public write renders every styled cell and the cursor with served assets",
        "runtime::tests::presentation::public_write_renders_every_cell_and_cursor_with_served_assets",
    ),
    // A schema report alone is insufficient: the public host must emit its warning.
    (
        "public create logs the browser-reserved binding warning",
        "runtime::tests::warnings::public_create_logs_reserved_shortcut",
    ),
    // A resolved Promise must mean the retained parser suffix is gone.
    // Reply callbacks must already have observed DA/DSR bytes before that completion.
    (
        "write/writeln resolve after parsing and reply flush",
        "runtime::tests::write_resolves_after_parsing_and_reply_flush",
    ),
    // Weak ownership does not cancel setTimeout; disposal must release the browser task too.
    (
        "disposal cancels queued parser tasks and rejects incomplete writes",
        "runtime::tests::dispose_cancels_queued_parser_work",
    ),
    // Local input must not paint the grid before application echo.
    // Paste framing follows negotiated modes rather than a browser heuristic.
    (
        "input/paste/sendKey/onData and subscription disposal",
        "runtime::tests::input::input_paste_send_key_and_disposable",
    ),
    // Host callbacks cannot carry mutable Session borrows across JavaScript.
    // Drained exit must follow the last output, preserving the web held-screen default.
    (
        "attachTransport/exited and title callbacks",
        "runtime::tests::input::callback_transport_and_events",
    ),
    // An onData callback may inject another key synchronously.
    // Its bytes must stay behind the original write to every attached transport.
    (
        "reentrant host input preserves transport FIFO",
        "runtime::tests::input::reentrant_input_preserves_transport_fifo",
    ),
    // Focus can synchronously dispatch a DOM callback into the same terminal.
    // Removing listeners and cancelling refreshes must make disposal idempotent.
    (
        "focus/blur/hasFocus/fit and idempotent disposal",
        "runtime::tests::dom::focus_blur_fit_and_dispose",
    ),
    // All public event names require delivery and registration removal witnesses.
    // A green title test alone would conceal missing bell or error forwarding.
    (
        "all six convenience events and removal",
        "runtime::tests::dom::all_convenience_events_and_subscription_lifetime",
    ),
    // External values include cycles and malformed optional properties.
    // Structured rejection is required instead of trapping or silently coercing a chord.
    (
        "invalid external API values produce errors",
        "runtime::tests::invalid_arguments_are_reported",
    ),
    // Browser canvas refusal must reject create, independent of any fixed canvas count.
    (
        "browser canvas resource refusal rejects create with a clear error",
        "runtime::tests::creation::refused_canvas_rejects_create_with_clear_error",
    ),
    // The approved registry stores weak resources, never another terminal Session.
    // Pointer equality distinguishes actual resource reuse from identical configuration.
    (
        "page terminals share one device and text context",
        "runtime::tests::resources::two_terminals_share_device_and_context",
    ),
    // Packs are caches, not the only trusted source of drawable font data.
    // Malformed pack bytes must retain a path to external runtime preparation.
    (
        "invalid startup pack falls back to external fonts",
        "runtime::tests::resources::corrupt_pack_falls_back_to_external_fonts",
    ),
    // A device allocation is insufficient evidence of a presented terminal.
    // Pixel ink with the cursor hidden rules out a clear-only first frame.
    (
        "rAF first frame has ink at exact device-pixel backing size",
        "runtime::tests::resources::rendered_canvas_backing_and_first_frame",
    ),
    // All three pipelines must pass browser validation, including unused raster paths.
    // Exact mask/color glyph pixels catch a rejected module concealed by valid Slug output.
    (
        "browser shaders validate and both raster atlas kinds draw glyph ink",
        "runtime::tests::shaders::pipelines_validate_and_raster_glyph_draws_ink",
    ),
    // Hinted ppem belongs to the text key, while the device remains page-wide.
    // Sharing an atlas under a different size would reuse incorrect grid metrics.
    (
        "different font sizes select separate text contexts",
        "runtime::tests::resources::different_font_sizes_have_separate_contexts",
    ),
    // Subscription and initialization closures must not pin disposed font/device owners.
    // Weak upgrade checks lifetime rather than trusting an empty registry entry.
    (
        "last disposal releases both weak resource kinds",
        "runtime::tests::resources::last_dispose_releases_resources",
    ),
    // WebGPU device destruction resolves the real browser loss Promise.
    // The next create must acquire a distinct device, including when the old owner survives.
    (
        "lost device is replaced for the next create",
        "runtime::tests::resources::device_loss_creates_fresh_device",
    ),
    // Literal little-endian grants verify wasm byte interpretation independently.
    // Native codec success cannot prove the client recognizes the new 0x84 message.
    (
        "bridge codec round trip in wasm",
        "runtime::tests::socket::codec_round_trip_in_wasm",
    ),
    // A real PTY reports its resized rows/columns via stty, not a fabricated event.
    // The final output and exit status cross the production WebSocket client.
    (
        "browser hello/input/output/resize/exit over a real bridge",
        "runtime::tests::socket::real_bridge_hello_echo_resize_exit",
    ),
    // Raw hosts own their protocol; development control tags must never reach them.
    // Both browser message payload forms use the same budgeted output parser.
    (
        "raw WebSocket text/binary output and unframed input",
        "runtime::tests::socket::raw_socket_text_binary_and_unframed_input",
    ),
    // Input credit bounds each send while retaining the whole unsent tail.
    // A later key or parser reply cannot jump ahead of that paste.
    (
        "credited paste/key/reply FIFO",
        "transport::credit::tests::paste_credit_and_reply_fifo",
    ),
    // Overgranting or zero credit must fail transactionally.
    // Malformed protocol input cannot manufacture capacity or corrupt the queued FIFO.
    (
        "invalid bridge input grants are rejected",
        "transport::credit::tests::invalid_grants_are_transactional",
    ),
    // Accepted packets are not necessarily parsed in the same feed turn.
    // Provenance keeps an old connection suffix from acknowledging a new socket.
    (
        "output ACKs follow parsed prefixes and write barriers",
        "transport::parsed::tests::barriers_and_ack_follow_parsed_prefixes",
    ),
];
