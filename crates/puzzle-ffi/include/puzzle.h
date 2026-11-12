/* Puzzle Terminal native embedding ABI v1 (D-43, D-50).
 *
 * Call every operation on the thread which created the app. Wrong-thread calls,
 * stale IDs and null/invalid argument shapes return errors in debug and release.
 * IDs contain slot+generation, never Rust pointers. Zero is never a live ID.
 *
 * The sole worker-callable function is callbacks.wakeup: it must be nonblocking,
 * thread-safe, and must NOT call Puzzle recursively. Post a host event-loop token;
 * call puzzle_app_tick on the host thread to receive events. Event callbacks may
 * enqueue surface operations. Recursive tick, app-free and surface creation are
 * Busy during event delivery. Do not unwind/throw across either callback.
 *
 * Bytes are pointer+length, not NUL-terminated strings. Empty buffers may be null.
 * Nonempty inputs must address one live readable allocation for the call; output
 * pointers must be aligned writable objects. Arbitrary invalid nonnull pointers
 * are undefined behavior, as in any C API. UTF-8, null, length overflow and numeric
 * enum inputs are checked before forwarding. Calls copy input they retain.
 *
 * Keep native view/connection objects alive until surface_free or app_free
 * returns. surface_free cancels its session and waits for native render teardown
 * (D-56); this cold lifecycle barrier may wait for an in-flight frame. app_free
 * cancels and joins all workers. PTY teardown cancels queued input, signals the
 * tty process group (HUP, then TERM/KILL after 250-ms grace windows), and discards
 * output while closing. The final master read closes after EOF/EIO or at most
 * one second of discard even when a descendant retains the slave. No worker is
 * detached; those shutdown allowances are not frame/input speed budgets.
 * Keep callback functions and userdata alive until
 * app_free returns. Do not compile this header with -fshort-enums.
 *
 * Most success results mean queued, not executed. Session owns terminal policy;
 * results arrive through tick. No input operation waits for a frame or Session.
 */
#ifndef PUZZLE_H
#define PUZZLE_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif

/* Separate namespaces; never cast an app ID into a surface ID. */
typedef uint64_t puzzle_app_t;
typedef uint64_t puzzle_surface_t;
/* Every function returns one of these stable ABI values. */
typedef enum puzzle_result {
    PUZZLE_OK = 0, PUZZLE_INVALID_ARGUMENT = 1, PUZZLE_INVALID_HANDLE = 2,
    PUZZLE_WRONG_THREAD = 3, PUZZLE_CLOSED = 4, PUZZLE_PLATFORM = 5,
    PUZZLE_UNSUPPORTED = 6, PUZZLE_BUSY = 7, PUZZLE_EXHAUSTED = 8
} puzzle_result_t;
/* Borrowed memory: see the allocation and UTF-8 contracts above. */
typedef struct puzzle_bytes { const uint8_t *data; size_t len; } puzzle_bytes_t;
/* name is UTF-8. payload is JSON except transport/input (raw user input) and transport/reply.
 * transport/reply is an 8-byte little-endian receipt ID followed by raw program
 * reply bytes. Preserve one FIFO across input and reply events. Copy call-scoped
 * bytes if needed, but call puzzle_surface_reply_complete ONLY after the entire
 * reply payload (excluding its ID prefix) is consumed by the actual writer.
 * Callback delivery or enqueue is not completion. Complete IDs once, in order;
 * duplicate/out-of-order IDs are ignored by the owner and return no credit.
 * Withheld completion throttles only this surface's output; keys and close remain
 * serviceable. On writer failure, close/free the surface; never acknowledge lost
 * bytes as written. Puzzle-owned PTY surfaces complete internally.
 * Both buffers last only for the callback invocation. Stable fact names come
 * from docs/BUS.md.
 */
/* Notifications: show has {token:string,id:string|null,title,body,urgency:0|1|2,
 * occasion:"always"|"unfocused"|"invisible",actions:{focus:bool,report:bool},
 * closeReport:bool}. token is an opaque generation, copied before callback return.
 * notification/close {token} removes a native delivery; notification/focus null
 * activates the creating surface. Hosts implement OS posting/removal only: no OSC
 * parsing or reply bytes. After user activation/dismissal or delivery failure,
 * return feedback to that same surface. Closed surfaces receive no callbacks.
 */
/* Tab/pane facts need no host OSC parser: shell/commandStarted carries JSON
 * {"command":string|null}; the string is at most 1024 UTF-8 bytes including ellipsis.
 * Missing metadata stays null. terminal/cwdChanged, terminal/progress,
 * shell/commandFinished, terminal/bell and terminal/titleChanged are forwarded
 * through this same tick-only callback. See docs/embedding/tabs.md.
 */
typedef struct puzzle_event { puzzle_bytes_t name, payload; } puzzle_event_t;
/* Wakeup is required; event may be NULL. Only wakeup runs on worker threads. */
typedef struct puzzle_callbacks {
    void *userdata;
    void (*wakeup)(void *userdata);
    void (*event)(void *userdata, puzzle_surface_t surface, const puzzle_event_t *event);
} puzzle_callbacks_t;
/* Empty path selects the usual native config; abi_version must equal 1. */
typedef struct puzzle_app_options {
    uint32_t abi_version;
    puzzle_bytes_t config_path;
    puzzle_callbacks_t callbacks;
} puzzle_app_options_t;
/* Backing device pixels, logical scale, display identity and rate in millihertz.
 * Zero width/height denotes minimized. Scale is finite and strictly positive.
 * macOS display is CGDirectDisplayID. A size-only change preserves glyph caches.
 */
typedef struct puzzle_geometry {
    uint32_t width, height;
    double scale;
    uint64_t display;
    uint32_t millihertz;
} puzzle_geometry_t;
/* View-local logical top-left points, in the surface/cursorRect coordinate space. */
typedef struct puzzle_rect {
    double x, y, width, height;
} puzzle_rect_t;
/* native_kind: 1 AppKit NSView, 2 Win32 HWND, 3 Xlib Window, 4 wl_surface, 5 CAMetalLayer (macOS).
 * Kind 5 borrows the exact host CAMetalLayer as a stable container. On macOS,
 * Puzzle installs a host-sized CALayer child and presents sRGB-tagged IOSurface
 * contents there; it no longer acquires/presents CAMetalLayer drawables. Puzzle
 * owns that child, its contents and backing-pixel allocation; hosts never resize
 * drawableSize. Retain the container through synchronous surface_free. Moving it
 * between views/windows does not require creating another Puzzle surface.
 * The host owns container bounds, forwards bounds/backing-scale changes through
 * surface_resize before the CA transaction commits, and continues calling tick
 * on wakeup (including during native event tracking). Initial geometry and
 * surface_resize physical pixels/scale are authoritative, preserving fractional
 * points (991 px / 2 = 495.5 pt). Puzzle sizes its child directly; stale CA bounds
 * cannot veto a matching image. CALayer.display may wait
 * at most one current display period ONLY when bounds/scale differ from the last
 * committed image (D-60); it covers edge drags, Magnet jumps and display moves.
 * Unchanged geometry never waits. Timeout keeps the previous image unscaled at
 * the visual top left; the asynchronous, display-link-paced path supplies the
 * replacement. Free cancels callbacks before joining workers or releasing layers.
 * native_display: Xlib Display/wl_display (unused on macOS/Windows).
 * native_screen: Xlib screen index, otherwise zero.
 * transport: 0 Puzzle-owned PTY, 1 host-fed transport. Empty shell/cwd inherits
 * the native default. Shell is an executable path, not a command string.
 */
typedef struct puzzle_surface_options {
    uint32_t native_kind;
    void *native_view, *native_display;
    int32_t native_screen;
    uint32_t transport;
    puzzle_bytes_t shell, cwd;
    puzzle_geometry_t geometry;
} puzzle_surface_options_t;
/* kind: 0 character, 1 W3C named key, 2 dead, 3 unidentified.
 * base is layout text without Shift/Option. physical is W3C code (KeyA, etc).
 * location: 0 standard, 1 left, 2 right, 3 numpad. pressed/repeat: exactly 0/1.
 * modifiers: Shift 1, Alt 2, Control 4, Super 8, left/right Option 16/32.
 */
typedef struct puzzle_key {
    uint32_t kind;
    puzzle_bytes_t logical, base, physical, text;
    uint32_t location, modifiers, pressed, repeat;
} puzzle_key_t;
/* kind: 0 move, 1 press, 2 release, 3 leave. Button uses xterm numbering.
 * x/y: signed view-local logical pixels (AppKit points), including padding.
 * Origin is the view's top left; +x points right and +y points down.
 * Puzzle applies the rendered inset, scale and scroll offset; hosts NEVER subtract
 * a grid origin. The same coordinates apply to pointer, wheel and selection.
 * cell_width/cell_height are reserved legacy fields, ignored; initialize to 0.
 * held is 0/1. clicks fits uint8.
 */
typedef struct puzzle_pointer {
    uint32_t kind, button;
    double x, y;
    uint32_t cell_width, cell_height, modifiers, held, clicks;
} puzzle_pointer_t;
/* All seven M9 declaration kinds use the same core validation and rendering.
 * kind: 0 highlight,1 underline,2 box,3 badge,4 inline text,5 SVG,6 glyph effect.
 * anchor_kind: 0 cursor,1 viewport rect,2 cell marker,3 marker range,4 line marker,
 * 5 OSC 133 zone,6 pointer. start/end are local marker IDs or zone ID.
 * rect: logical x,y,w,h. slot: 0 background,1 below text,2 text,3 raster text,
 * 4 cursor,5 above text,6 top. color: straight 0xRRGGBBAA.
 * style: underline 0 none/1 single/2 double/3 curly/4 dotted/5 dashed;
 * inline text 0 regular/1 bold/2 italic/3 bold italic. size/radius: logical px.
 * placement: 0 after/1 before/2 above/3 below/4 end-of-line.
 * SVG fit: 0 cell/1 range/2 transform. transform: a,b,c,d,e,f; use identity
 * {1,0,0,1,0,0} for unchanged glyph effects. opacity finite (core clamps 0..1).
 * auto_remove/replace_color/hidden: 0/1. text: label or SVG path. Empty tooltip
 * creates no hit region. Add results are asynchronous; invalid declarations
 * produce surface/decorationAdded error and leave installed definitions intact.
 */
typedef struct puzzle_decoration {
    uint32_t kind, anchor_kind;
    uint64_t start, end;
    float rect[4];
    uint32_t slot, auto_remove, color;
    float radius, size;
    uint32_t style, placement, fit;
    double transform[6];
    float opacity;
    uint32_t replace_color, hidden;
    puzzle_bytes_t text, tooltip;
} puzzle_decoration_t;

/* Creates the App on its owner thread; out is initialized only on success. */
puzzle_result_t puzzle_app_new(const puzzle_app_options_t *options, puzzle_app_t *out);
/* Cancels, joins and releases every native resource before returning. */
puzzle_result_t puzzle_app_free(puzzle_app_t app);
/* Delivers queued events on the owner thread, without borrowing a Session. */
puzzle_result_t puzzle_app_tick(puzzle_app_t app);
/* Session-layer JSON patch; valid fields apply, rejected fields return InvalidArgument.
 * render.padding is logical pixels per edge. It updates the next frame, input
 * mapping, cursorRect and child grid together, including existing surfaces. */
puzzle_result_t puzzle_app_set_config(puzzle_app_t app, puzzle_bytes_t patch);
/* Cold attachment may initialize GPU/fonts/PTY; input methods only enqueue. */
puzzle_result_t puzzle_surface_new(puzzle_app_t app, const puzzle_surface_options_t *options, puzzle_surface_t *out);
/* Cancels and waits for native release; the view may be destroyed after return. */
puzzle_result_t puzzle_surface_free(puzzle_surface_t surface);
/* Physical/layout callback through the shared native input adapter. */
puzzle_result_t puzzle_surface_key(puzzle_surface_t surface, const puzzle_key_t *key);
/* Committed UTF-8 text, routed through the active Session input mode. */
puzzle_result_t puzzle_surface_text(puzzle_surface_t surface, puzzle_bytes_t text);
/* Optional event clocks, ABI-v1 layouts unchanged. Both readings are seconds of
 * the SAME host monotonic clock: event_seconds is the original OS event time;
 * clock_seconds is a current reading immediately before this call (macOS:
 * NSEvent.timestamp and ProcessInfo.systemUptime). No cross-clock epoch is assumed.
 * Negative, nonfinite or reversed pairs return INVALID_ARGUMENT without input.
 * Enable puzzle_desktop::input=debug before creation for host residence,
 * API/marshalling, control-queue and owner-to-writer wall/CPU diagnostics.
 * C hosts: set PUZZLE_C_HOST_TRACE to a private log path before puzzle_app_new
 * (release supported; existing Rust subscribers win). The input target may
 * expose private key/text facts; normal hosts install no diagnostic sink.
 * Untimed calls report host residence as unknown. No ordinary per-key logging.
 * Local PTY endpoint is writer-queue admission; host transport is input-event
 * enqueue, not the later host OS write or child consumption. */
puzzle_result_t puzzle_surface_key_timed(puzzle_surface_t surface, const puzzle_key_t *key, double event_seconds, double clock_seconds);
puzzle_result_t puzzle_surface_text_timed(puzzle_surface_t surface, puzzle_bytes_t text, double event_seconds, double clock_seconds);
/* Provisional text; cursor UTF-8 byte offset or SIZE_MAX if unknown. */
puzzle_result_t puzzle_surface_preedit(puzzle_surface_t surface, puzzle_bytes_t text, size_t cursor);
/* Gesture sample; presented overlay regions decide consumption before mouse reporting. */
puzzle_result_t puzzle_surface_pointer(puzzle_surface_t surface, const puzzle_pointer_t *pointer);
/* unit: 0 rows, 1 precise logical pixels (including host OS inertia).
 * pointer uses the same view-local logical position as puzzle_surface_pointer. */
puzzle_result_t puzzle_surface_scroll(puzzle_surface_t surface, double delta, uint32_t unit, const puzzle_pointer_t *pointer);
/* Acknowledge seen output on this surface. Fresh subsequent program output
 * emits surface/activity with JSON null once, until acknowledged again. No
 * quiet timer or per-chunk events; input/resize/repaint do not count as output.
 * Call on showing a pane and on activity while it remains visible. This queues
 * an owner message and never waits for Session or rendering. */
puzzle_result_t puzzle_surface_activity_seen(puzzle_surface_t surface);

/* Copy the last actually presented image; no rendering, worker or GPU wait.
 * Call on the creating thread, including inside a tick callback. max_width_px
 * must be positive. Output is top-left, tightly packed premultiplied encoded
 * sRGB RGBA8 (four bytes/pixel), with width <= max_width_px, never enlarged.
 * Height preserves aspect ratio rounded down, minimum one pixel. DPR is not
 * applied again. Bytes/dimensions describe the retained frame even if a newer
 * geometry is pending. No host chrome is included.
 * Buffer is host-owned and exclusively writable for capacity bytes; it must
 * not overlap either required writable uint32_t output. NULL is valid only
 * with capacity 0. Exhausted returns required width/height without touching
 * pixels, allowing a NULL/0 size query; allocate width*height*4 and call again.
 * Unsupported before the first presentation (or on a backend without readable
 * presented storage); Busy if native image access is unavailable.
 * On other errors dimensions are zero and pixels unchanged. ABI v1 is unchanged.
 * See docs/embedding/snapshot.md. */
puzzle_result_t puzzle_surface_snapshot(puzzle_surface_t surface, uint32_t max_width_px,
    uint8_t *buffer, size_t capacity, uint32_t *width, uint32_t *height);
/* Focus report; focused must be 0 or 1. */
puzzle_result_t puzzle_surface_focus(puzzle_surface_t surface, uint32_t focused);
/* Visibility and occlusion must be 0/1; idle/hidden links stop. */
puzzle_result_t puzzle_surface_visibility(puzzle_surface_t surface, uint32_t visible, uint32_t occluded);
/* Coalesced backing extent, scale and display observation. */
puzzle_result_t puzzle_surface_resize(puzzle_surface_t surface, puzzle_geometry_t geometry);
/* Selects host pacing until close; Puzzle's own link detaches for this surface.
 * Call on the creating thread. The callback only queues work on the render worker.
 * display matches the latest geometry; index increases on that display across
 * source restarts. Duplicate indices and old-display callbacks are ignored.
 * output_delay_ns is the remaining monotonic time until scanout, measured from
 * this call (subtract any forwarding delay); period_ns must be nonzero.
 * Continue callbacks while visible output/animation needs frames; hidden surfaces
 * do not present. Input never waits for callbacks. Startup/resize exceptions and
 * D-33 settling remain unchanged. This call retains no callback/window pointer.
 */
puzzle_result_t puzzle_surface_refresh(puzzle_surface_t surface, uint64_t display, uint64_t index, uint64_t output_delay_ns, uint64_t period_ns);
/* Sanitized paste, bracketed according to the application's current mode. */
puzzle_result_t puzzle_surface_paste(puzzle_surface_t surface, puzzle_bytes_t text);
/* outcome: 0 activated, 1 closed, 2 close tracking unavailable, 3 delivery failed.
 * Unknown/stale tokens are ignored; malformed tags/ranges return INVALID_ARGUMENT.
 * This queues an owner message and never waits for OS notification work.
 */
puzzle_result_t puzzle_surface_notification_feedback(puzzle_surface_t surface, puzzle_bytes_t token, uint32_t outcome);

typedef struct puzzle_mime_item { puzzle_bytes_t mime, data; } puzzle_mime_item_t;

/* Native OSC 72 drop target. drop/registration delivers {generation: decimal
 * string, mimes: [names] or null}; null unregisters and releases native state.
 * drop/acceptance delivers {generation, operation:0 reject/1 copy/2 move, mimes};
 * drop/concluded delivers {generation, operation}. Callbacks arrive via app_tick.
 * Until registration, keep the host's ordinary paste-the-path behavior. On hover
 * offer names only, initially reject until acceptance. Capture bytes eagerly at
 * the actual user drop; no user data is readable from hover. Keep OS logic only
 * in the host: the library implements all OSC negotiation and replies.
 * kind:0 move,1 leave,2 drop. x/y are view-local top-left logical pixels (AppKit
 * points), including padding: never subtract a grid origin. operations:1 copy,
 * 2 move,3 either (ignored for leave). Echo this surface's registration generation.
 * At most 16 MIME alternatives, 256 bytes/name and 64 MiB total data. Move data
 * must be empty; Leave count must be zero. Borrowed arrays/bytes live for this
 * call only; all retained data is copied. Never forward a synthetic program drag
 * as a user drop; same-window program-originated drags must not grant file reads.
 * Stale generations are inert, including after RIS, child exit or re-registration.
 * MIME data is never treated as pasted text, and no supplied path is opened. */
puzzle_result_t puzzle_surface_drop(puzzle_surface_t surface, uint64_t generation,
    uint32_t kind, double x, double y, uint32_t operations,
    const puzzle_mime_item_t *items, size_t count);

/* Clipboard read result follows the same core paste policy. */
puzzle_result_t puzzle_surface_clipboard_answer(puzzle_surface_t surface, puzzle_bytes_t text);

/* OSC 5522 produces clipboard/mime at app_tick, with JSON
 * {token:"decimal",operation:"read"|"write",primary:bool,revision:"decimal"|null,items:[...]};
 * read items are MIME names; write items are {type:string,data:[byte,...]}.
 * Primary maps to the standard clipboard, as for OSC 52. Copy callback data
 * before returning. Finish the OS operation asynchronously, then answer below.
 * Never acknowledge a write before it actually succeeds. For an unsupported
 * MIME or operation return UNAVAILABLE; an unavailable prompt returns DENIED.
 * Token "0" denotes a public API write without an OSC acknowledgement: do not answer it.
 * One pending operation per surface is allowed; overload or a host that never
 * completes gets EBUSY. Closed surfaces reject answers; stale tokens are ignored.
 * A write answer has no items. A read answer contains requested representations;
 * for the name-list query ".", return MIME "." containing space-separated names.
 * This API never injects paste input. It copies all ranges during the call. */
enum puzzle_clipboard_status {
    PUZZLE_CLIPBOARD_DONE=0, PUZZLE_CLIPBOARD_UNAVAILABLE=1,
    PUZZLE_CLIPBOARD_DENIED=2, PUZZLE_CLIPBOARD_BUSY=3,
    PUZZLE_CLIPBOARD_IO=4, PUZZLE_CLIPBOARD_INVALID=5, PUZZLE_CLIPBOARD_TOO_LARGE=6
};
/* At most 64 items / 64 MiB total. items may be NULL only when count is zero. */
/* A user paste gesture supplies the native clipboard revision, supported MIME names,
 * and optional ordinary text fallback. Call from the user gesture, not program output.
 * revision MUST change on every content change, even restoration; UINT64_MAX is invalid.
 * Puzzle generates a secure one-use token and sends a listing if mode 5522 is enabled;
 * otherwise it pastes fallback text normally. Empty text can be null. primary is 0/1. */
puzzle_result_t puzzle_surface_clipboard_paste(puzzle_surface_t surface, uint32_t primary,
    uint64_t revision, const puzzle_bytes_t *mimes, size_t count, puzzle_bytes_t text);
/* revision is the generation checked before AND after a read, UINT64_MAX otherwise. */
puzzle_result_t puzzle_surface_clipboard_mime_answer(puzzle_surface_t surface, uint64_t token,
    uint32_t status, uint64_t revision, const puzzle_mime_item_t *items, size_t count);
/* Per-surface searchUi option: 0 builtIn (default), 1 host. Set immediately after
 * creation before keys; this setter preserves the ABI-v1 options structure.
 * host: the bound search action emits search/requested (null), without opening
 * or focusing Puzzle's query overlay. Switching to host closes built-in capture
 * but keeps matches; switching back does not open anything until the next action.
 * This choice survives settings reloads and never changes platform input policy.
 * See docs/embedding/search.md; every call below is creating-thread-only, queued.
 */
puzzle_result_t puzzle_surface_set_search_ui(puzzle_surface_t surface, uint32_t ui);
/* Replace host UI coverage; count 0 clears and permits NULL. At most 64 rects,
 * all coordinates finite, width/height nonnegative. The readable array is copied
 * before return. Invalid input leaves the previous list intact. On the next
 * owner/frame turn movable Puzzle overlays avoid it and match centring uses an
 * unobscured row band. This is not padding: grid, text and PTY size are unchanged.
 * Keep it updated when native UI moves/resizes; clear it when hidden.
 * See docs/embedding/occlusion.md for lifecycle and placement bounds.
 */
puzzle_result_t puzzle_surface_set_occluded_rects(puzzle_surface_t surface, const puzzle_rect_t *rects, size_t count);
/* Read the selection at this message's ordered position, without changing the
 * clipboard. selection/text returns {request: uint64, text: UTF-8 JSON string}
 * through app_tick. Empty/unavailable selection is "". Copy callback bytes if
 * retained, correlate request (parse uint64 exactly), ignore stale UI requests.
 * Closing the surface suppresses outstanding results. Success means queued.
 */
puzzle_result_t puzzle_surface_selected_text(puzzle_surface_t surface, uint64_t request);
/* Literal query; case_mode: 0 smart (uppercase makes it sensitive), 1 sensitive,
 * 2 insensitive. Empty query clears. search/state reports discovered count,
 * zero-based current index (null only when count is zero), complete and scanned rows.
 * Initial selection prefers the lowest visible match without scrolling, otherwise
 * centres the first discovered match; later slices preserve that selection.
 * Results grow incrementally over retained screen/history; later output may
 * make complete false again. Navigation uses discovered matches, wraps and
 * centres the match in the viewport. Before any matches are found, the Session
 * retains one pending step (the latest direction) and applies it when the first
 * match is discovered. Query replacement or clear discards that pending step;
 * hosts may queue next/previous immediately after starting a query. The pending
 * step runs after automatic selection, exactly as a step issued after results.
 */
puzzle_result_t puzzle_surface_search(puzzle_surface_t surface, puzzle_bytes_t query, uint32_t case_mode);
/* Navigate current results. */
puzzle_result_t puzzle_surface_search_next(puzzle_surface_t surface);
/* Navigate backwards. */
puzzle_result_t puzzle_surface_search_prev(puzzle_surface_t surface);
/* Cancels incremental work and clears hits/highlights; emits changed search/state.
 * Hosts call this when closing their find UI; it never opens or focuses a UI. */
puzzle_result_t puzzle_surface_search_clear(puzzle_surface_t surface);
/* surface/colors is delivered once after creation, then when effective default
 * colours change (theme/appearance, OSC 10/11 and resets). Payload:
 * {background:[r,g,b],foreground:[r,g,b],dark:boolean}, encoded sRGB bytes 0..255.
 * dark is derived from background relative luminance (white contrast >= black).
 * Delivery is through app_tick, never a synchronous query into Session.
 * Use this to match native find controls and chrome; see docs/embedding/colors.md.
 */
/* Per-surface session-layer JSON patch using the same native schema. */
puzzle_result_t puzzle_surface_set_config(puzzle_surface_t surface, puzzle_bytes_t patch);
/* Host transport only: accepted prefix may be zero. Retry suffix after wakeup. */
puzzle_result_t puzzle_surface_feed_output(puzzle_surface_t surface, puzzle_bytes_t output, size_t *accepted);
/* Host transport only. OK means the completion message was queued; actual receipt
 * validation runs on the Session owner. Zero IDs are rejected synchronously. */
puzzle_result_t puzzle_surface_reply_complete(puzzle_surface_t surface, uint64_t reply_id);
/* Host transport only: signal UTF-8, empty means absent; code_present 0/1. */
puzzle_result_t puzzle_surface_exited(puzzle_surface_t surface, uint32_t code_present, int32_t code, puzzle_bytes_t signal, uint64_t runtime_ms);
/* Suggestion text, never application bytes until accepted. Empty clears. */
puzzle_result_t puzzle_surface_ghost_text(puzzle_surface_t surface, puzzle_bytes_t text);
/* Explicit acceptance of current suggestion. */
puzzle_result_t puzzle_surface_ghost_accept(puzzle_surface_t surface);
/* Debug HUD model visibility (0/1). */
puzzle_result_t puzzle_surface_performance_hud(puzzle_surface_t surface, uint32_t shown);
/* Expose damage requests a paced frame, never draws on the host thread. */
puzzle_result_t puzzle_surface_request_frame(puzzle_surface_t surface);
/* Correlated asynchronous tracked endpoint creation; marker IDs are surface-local. */
puzzle_result_t puzzle_surface_marker_add(puzzle_surface_t surface, uint64_t request, uint64_t line, size_t column);
/* Stale local marker IDs are rejected; declarations using it become Gone. */
puzzle_result_t puzzle_surface_marker_remove(puzzle_surface_t surface, uint64_t marker);
/* Registers any declaration kind; result supplies a monotonic decoration identity. */
puzzle_result_t puzzle_surface_decoration_add(puzzle_surface_t surface, uint64_t request, const puzzle_decoration_t *declaration);
/* Queues a replacement through the core's atomic declaration controller. */
puzzle_result_t puzzle_surface_decoration_update(puzzle_surface_t surface, uint64_t decoration, const puzzle_decoration_t *declaration);
/* Idempotent declaration removal; markers remain caller-owned. */
puzzle_result_t puzzle_surface_decoration_remove(puzzle_surface_t surface, uint64_t decoration);
/* Distribution-specific declarations preserve the base ABI layout. */
#include "puzzle_extra.h"
#ifdef __cplusplus
}
#endif
#endif
