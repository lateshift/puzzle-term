//! Stable warning and error names emitted by the terminal composition layer.
//!
//! A diagnostic's `event` field is part of the observable API (design §5.6).
//! The bus cascade name is included here even though the bus emits it: this
//! list gives integrators one place to discover warning and error meanings.
//! Bridge names are listed as strings only: the standalone server has no
//! dependency on this catalogue or the terminal engine. Its authorization
//! failures deliberately report categories rather than HTTP request contents.
//! Browser clipboard failures likewise omit copied text, and callback failures
//! name their operation without serializing potentially private DOM objects.

/// One stable diagnostic name and its meaning.
pub struct Diagnostic {
    /// Value of the tracing `event` field.
    pub name: &'static str,
    /// Why the condition matters to an embedding application.
    pub meaning: &'static str,
}

/// Names currently emitted by implemented browser, desktop, bus and VT code.
pub const CATALOGUE: &[Diagnostic] = &[
    // A valid binding can remain stored while browser chrome owns its shortcut.
    Diagnostic {
        name: "settings.binding_unavailable",
        meaning: "A preserved binding is browser-reserved or requires a future feature",
    },
    // Failures are named without including private clipboard text or DOM objects.
    Diagnostic {
        name: "web.clipboard_write_failed",
        meaning: "The browser rejected a user-activated clipboard write",
    },
    // A failed removal matters because JS may still retain the wasm trampoline.
    Diagnostic {
        name: "web.listener_remove_failed",
        meaning: "A terminal's DOM listener could not be removed during disposal",
    },
    // Resolution is observed without a timer; rejection disables that query lane.
    Diagnostic {
        name: "web.dpr_watch_failed",
        meaning: "A browser resolution query could not be re-armed after a DPR change",
    },
    // Motion coalescing must never silently lose a failed scheduling request.
    Diagnostic {
        name: "web.motion_schedule_failed",
        meaning: "A requested mouse-motion report could not schedule its rAF turn",
    },
    // A rejected upgrade never logs the HTTP request or token-bearing URL.
    Diagnostic {
        name: "bridge.session_failed",
        meaning: "A bridge handshake, protocol request or PTY worker failed; its session closed",
    },
    // Disconnect cleanup owns the child until it is signaled and reaped.
    Diagnostic {
        name: "bridge.hangup_failed",
        meaning: "The safe PTY child API could not signal a still-running bridge shell",
    },
    // Input has one ordered writer; OS failure cannot count as successful delivery.
    Diagnostic {
        name: "bridge.input_failed",
        meaning: "An ordered bridge PTY input write failed",
    },
    // Missing bundled identity is recoverable, but applications lose Sync discovery.
    Diagnostic {
        name: "bridge.terminfo_missing",
        meaning: "The bridge could not find bundled xterm-puzzle and used xterm-256color",
    },
    // A truncated OSC may be harmless title text, but security-sensitive
    // OSC 8 and OSC 52 payloads are rejected rather than applied partially.
    Diagnostic {
        name: "vt.osc_truncated",
        meaning: "An OSC string reached the parser's 64 KiB bound",
    },
    // A long hyperlink is different from a truncated general OSC string:
    // it violates the stricter URI bound even when the parser completed it.
    Diagnostic {
        name: "vt.link_too_long",
        meaning: "An OSC 8 URI exceeded the 2 KiB hyperlink limit",
    },
    Diagnostic {
        name: "vt.osc52_invalid",
        meaning: "OSC 52 clipboard text was invalid base64 or UTF-8",
    },
    // Unknown DCS strings are streamed away; only a recognized but
    // oversized query gets this warning, then no partial reply is sent.
    Diagnostic {
        name: "vt.dcs_truncated",
        meaning: "A DCS query exceeded its bounded request buffer",
    },
    // A cascade is a hook bug or feedback loop, not a malformed PTY byte.
    Diagnostic {
        name: "bus.cascade_limit",
        meaning: "A hook queued work for more than sixteen rounds; remaining work was dropped",
    },
    // A trusted continuation name is a code invariant, not user-supplied data.
    Diagnostic {
        name: "bus.resume_unknown_target",
        meaning: "A resumed command named an absent policy; it was dropped and debug builds assert",
    },
    // Browser setup failure is visible in the demo as an error state.
    Diagnostic {
        name: "web.gpu_setup_failed",
        meaning: "The browser could not create or present its WebGPU clear frame",
    },
    // The next two names cover failures while presenting that error to a user.
    // They distinguish a GPU failure from a broken embedding page.
    Diagnostic {
        name: "web.canvas_state_failed",
        meaning: "The page could not mark its canvas as failed",
    },
    Diagnostic {
        name: "web.error_display_failed",
        meaning: "The page could not reveal the WebGPU setup error text",
    },
    // Native initialization is fatal to opening the harness window.
    Diagnostic {
        name: "desktop.window_init_failed",
        meaning: "The native window or GPU surface could not start",
    },
    // Resizing and drawing failures may recover on the next OS event.
    Diagnostic {
        name: "desktop.surface_config_failed",
        meaning: "A resized native surface could not be configured",
    },
    Diagnostic {
        name: "desktop.render_failed",
        meaning: "The native surface clear pass failed",
    },
];
