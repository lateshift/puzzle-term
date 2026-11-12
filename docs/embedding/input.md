# Measure native input delivery

Existing key/text calls and ABI-v1 structures are unchanged. C hosts may call
`puzzle_surface_key_timed` or `puzzle_surface_text_timed` with the original OS
event timestamp and a current reading of **the same monotonic clock**, in
seconds, immediately before the call. For AppKit these are `NSEvent.timestamp`
and `ProcessInfo.processInfo.systemUptime`; do not use wall-clock dates or Rust's
clock epoch. Rust hosts construct `InputTiming::new(event_seconds, clock_seconds)`
at their API boundary, before native input conversion, and pass it to
`Surface::input_timed`, `key_timed` or `text_timed`. Invalid or reversed clock
pairs reject the input. Enable the private `puzzle_desktop::input=debug` target
before creating surfaces to record each encoded key's host-event→API residence,
API→channel send, channel→owner wait, owner→writer wall/CPU, and total delivery.
Untimed calls keep host residence **unknown**, while still measuring API delivery
when the target is enabled. A consumed binding/query edit produces no writer
sample and cannot label a later parser reply. Normal logging emits no per-key
timing. The local PTY endpoint is successful ordered-writer queue admission;
for a host transport it is input-event enqueue, so the host measures any later
tick/OS-write/child delay separately. The diagnostic target may also expose
existing private input logs; keep its output private. Rust hosts install their
usual tracing subscriber. C hosts set `PUZZLE_C_HOST_TRACE` to a private file
before `puzzle_app_new`; this opt-in sink enables the input target in release
builds as well. A caller-installed Rust subscriber takes precedence.

The once-per-second main-callback summary pairs the longest callback wall time
with that same callback’s CPU time (`max_main_busy_cpu_ms`, absent when
unavailable), rather than measuring gaps between events. Per-callback records
stay TRACE and require a build without the default release TRACE cap. Winit has no original OS key timestamp: harness-authored
keys use their pre-UI-post clock to observe event-loop delivery, and real untimed
callbacks cannot prove OS residence. Session 2 supplies AppKit event clocks in
the native host; this contract requires no app-specific API or synchronous
Session query.

Apple references: [event timestamp](https://developer.apple.com/documentation/appkit/nsevent/timestamp)
and [system uptime](https://developer.apple.com/documentation/foundation/processinfo/systemuptime).

For native IME/caret diagnosis, the same private C trace sink also enables
`puzzle_desktop::caret=debug` in release builds. `embed.caret_emitted` records
actual-contents image serial, rectangle and cumulative queued/delivered/failed
counts; `embed.caret_delivered` records handoff through the Rust tick sink;
`ffi.caret_callback` confirms the actual C callback returned. Counts belong to
one surface lifetime. Unchanged caret rectangles, coalesced images and D-60
timeouts create no notification. These observations do not request extra frames
or replay facts. Keep the trace private: this sink also enables input diagnostics.
