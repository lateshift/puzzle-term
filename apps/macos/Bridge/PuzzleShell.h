#ifndef PUZZLE_SHELL_H
#define PUZZLE_SHELL_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Version 2 is a proposed shell contract, not an interface copied from Puzzle.
 * All functions must run on the AppKit main thread. Commands return 0/failure
 * off-thread or during a callback. Queue commands until the callback returns.
 * The shell owns windows, views, and layers; Rust owns its terminal and GPU work.
 */
#define PUZZLE_SHELL_ABI_VERSION 2u

/* Numeric values are fixed ABI: never reorder or reuse them. */
enum PuzzleShellEventKind {
    PUZZLE_WINDOW_CREATED = 1, PUZZLE_WINDOW_CLOSED = 2,
    PUZZLE_TAB_CREATED = 3, PUZZLE_TAB_CLOSING = 4,
    PUZZLE_TAB_SELECTED = 5, PUZZLE_TAB_MOVED = 6,
    PUZZLE_SURFACE_CHANGED = 7, PUZZLE_FOCUS_CHANGED = 8,
    PUZZLE_KEY_DOWN = 9, PUZZLE_KEY_UP = 10,
    PUZZLE_TEXT_COMMITTED = 11, PUZZLE_COMPOSITION_CHANGED = 12,
    PUZZLE_POINTER = 13, PUZZLE_SCROLL = 14,
    PUZZLE_MODIFIERS_CHANGED = 15, PUZZLE_VISIBILITY_CHANGED = 16,
    PUZZLE_TAB_REORDERED = 17, PUZZLE_TAB_METADATA_CHANGED = 18,
    PUZZLE_PANE_CREATED = 19, PUZZLE_PANE_CLOSING = 20,
    PUZZLE_PANE_SELECTED = 21, PUZZLE_PANE_LAYOUT_CHANGED = 22
};

/* A single fixed-layout record keeps bindgen/manual repr(C) integration simple.
 * Zero-initialized fields not relevant to the kind must be ignored.
 * ns_view/metal_layer are borrowed Objective-C pointers, valid from PANE_CREATED
 * through PANE_CLOSING (inclusive). Release GPU surfaces synchronously during
 * PANE_CLOSING before returning; neither pointer is safe after that callback.
 * Moving a tab preserves both pointers and the tab ID. window_id is its new owner.
 * TAB_MOVED/TAB_REORDERED code is the final zero-based index within that window.
 * TAB_CREATED code carries the tab index; its initial-pane handles are informational.
 * PANE_CREATED announces each surface (including the first), code=visual traversal index.
 * TAB_CLOSING is structural and follows all PANE_CLOSING callbacks for that tab.
 * pane_id identifies every surface/input/focus/visibility event, independent of tab_id.
 * PANE_SELECTED reports the tab's active pane even when keyboard focus is false.
 * PANE_LAYOUT_CHANGED invalidates the tree; query pane IDs and splits inside the callback.
 * ABI v2 APPENDS pane_id at offset 136 (record size 144); regenerate Rust bindings.
 * Existing field offsets, function signatures and event numbers 1..18 are preserved.
 * TAB_METADATA_CHANGED text is the effective name without a duplicate suffix;
 * flags uses PuzzleShellMetadataFlag, code is packed sRGB 0xRRGGBBAA when colored.
 * Read-only getters may be called during any callback; mutations remain rejected.
 * SURFACE_CHANGED carries pixel dimensions and the point-to-pixel scale.
 * width/height may be zero: suspend presentation until both are nonzero.
 * FOCUS_CHANGED/VISIBILITY_CHANGED use flags=1 for true, 0 for false.
 * KEY events: code=macOS hardware key code, flags=modifier bitmask below,
 * detail=1 when repeating. MODIFIERS_CHANGED carries hardware key code and
 * modifier flags only; detail/text are empty.
 * TEXT/COMPOSITION: UTF-8, not NUL-terminated; copy before returning.
 * COMPOSITION code/detail are selection location/length
 * in UTF-16 code units, with UINT32_MAX meaning no location.
 * POINTER: code=0 down, 1 up, 2 move/drag; detail=button number.
 * Coordinates are view-local points with the origin at the TOP LEFT.
 * SCROLL: delta_x/y=AppKit scrollingDeltaX/Y; detail=1 means precise points,
 * 0 means line deltas; phase/momentum_phase are NSEvent.Phase raw values.
 * KEY text is informational charactersIgnoringModifiers; only TEXT_COMMITTED
 * inserts text. Escape/return/arrows etc. remain hardware key events.
 */
typedef struct PuzzleShellEvent {
    uint32_t abi_version;
    uint32_t kind;
    uint64_t window_id;
    uint64_t tab_id;
    void *ns_view;
    void *metal_layer;
    uint32_t width_px;
    uint32_t height_px;
    double scale;
    uint64_t flags;
    uint32_t code;
    uint32_t detail;
    double x;
    double y;
    double delta_x;
    double delta_y;
    uint64_t phase;
    uint64_t momentum_phase;
    const uint8_t *text;
    size_t text_length;
    uint64_t pane_id; /* ABI v2: leaf identity; TAB_CREATED also identifies its initial pane. */
} PuzzleShellEvent;

/* Platform-independent modifier bits used by input events. */
enum PuzzleShellModifier {
    PUZZLE_SHIFT = 1, PUZZLE_CONTROL = 2, PUZZLE_OPTION = 4,
    PUZZLE_COMMAND = 8, PUZZLE_CAPS_LOCK = 16, PUZZLE_FUNCTION = 32
};

/* Select which independent naming layer the UTF-8 getter copies. */
enum PuzzleShellTabTitleKind {
    PUZZLE_TITLE_AUTOMATIC = 0, PUZZLE_TITLE_CUSTOM = 1,
    PUZZLE_TITLE_EFFECTIVE = 2, PUZZLE_TITLE_DISPLAY = 3, PUZZLE_TITLE_FALLBACK = 4
};
/* Metadata notification flags distinguish absent overrides from valid empty/zero values. */
enum PuzzleShellMetadataFlag {
    PUZZLE_TAB_HAS_CUSTOM_NAME = 1, PUZZLE_TAB_HAS_COLOR = 2
};

/* Callback and context remain borrowed until replacement/unregistration.
 * Callbacks run synchronously on the main thread. Never unwind/panic across FFI.
 * Installing a callback replays WINDOW_CREATED, TAB_CREATED, PANE_CREATED, layout, metadata,
 * geometry, visibility, tab/pane selection and focus for current state.
 * Passing NULL detaches it. Release any Rust surfaces before detaching/replacing.
 */
typedef void (*PuzzleShellEventCallback)(void *context, const PuzzleShellEvent *event);
int32_t puzzle_shell_set_event_callback(PuzzleShellEventCallback callback, void *context);
/* Pure version query: safe on any thread and before NSApplication initializes. */
uint32_t puzzle_shell_abi_version(void);
/* Create an independent window with one selected placeholder tab. */
uint64_t puzzle_shell_create_window(void);
/* Add and select a new tab in a live window; unknown IDs return zero. */
uint64_t puzzle_shell_create_tab(uint64_t window_id);
/* Select a live session in its current owner, revealing its title-bar page. */
int32_t puzzle_shell_select_tab(uint64_t tab_id);
/* Destroy a session, synchronously notifying Rust; the last tab closes its window. */
int32_t puzzle_shell_close_tab(uint64_t tab_id);
/* Destroy all sessions in one window before its WINDOW_CLOSED event. */
int32_t puzzle_shell_close_window(uint64_t window_id);
/* Returns the new window ID; preserves the tab's view, layer and session. */
uint64_t puzzle_shell_move_tab_to_new_window(uint64_t tab_id);
/* Move/reorder at a final zero-based index AFTER removing the source tab.
 * Valid range is 0..remaining destination count, inclusive; failures do not mutate either window.
 * Reordering preserves selection/focus; cross-window moves select the arriving tab. */
int32_t puzzle_shell_move_tab(uint64_t tab_id, uint64_t window_id, uint32_t index);
/* Set the runtime/automatic title, preserving a custom name. Empty/blank uses the
 * stable fallback. UTF-8 is borrowed for this call; reject controls and >4096 bytes.
 * Invalid UTF-8/IDs return 0. NULL is allowed only with length=0. */
int32_t puzzle_shell_set_tab_title(uint64_t tab_id, const uint8_t *title_utf8, size_t length);
/* Set a trimmed custom name; empty/whitespace clears the override. Same validation as above. */
int32_t puzzle_shell_set_tab_custom_name(uint64_t tab_id, const uint8_t *name_utf8, size_t length);
/* Copy UTF-8 into caller-owned storage, WITHOUT a NUL terminator. out_length is required.
 * NULL buffer with capacity=0 queries the size. Return 1 on success, -1 if too small
 * (out_length reports required bytes, buffer untouched), 0 for invalid arguments/IDs/thread.
 * An absent custom name reads as zero bytes; metadata flags distinguish the naming mode.
 * DISPLAY includes the window-local duplicate suffix; EFFECTIVE never includes it.
 * Read-only: allowed inside callbacks, main thread only. Failure 0 leaves outputs untouched. */
int32_t puzzle_shell_get_tab_title(uint64_t tab_id, uint32_t kind, uint8_t *buffer, size_t capacity, size_t *out_length);
/* Optional subtle tab accent in packed sRGB 0xRRGGBBAA. enabled must be 0 or 1;
 * disabling restores neutral chrome. A transparent/zero color is distinct from no color. */
int32_t puzzle_shell_set_tab_color(uint64_t tab_id, uint32_t rgba, int32_t enabled);
/* Both outputs are required; rgba=0 when disabled. Read-only, callback-safe on main thread. */
int32_t puzzle_shell_get_tab_color(uint64_t tab_id, uint32_t *rgba, int32_t *enabled);
/* Append every other window's tabs here in window creation order, preserving each
 * source's tab order, destination selection, tab IDs, names, colors and surface pointers.
 * Empty source windows close. An already-collected live window returns 1 (no-op). */
int32_t puzzle_shell_collect_tabs(uint64_t window_id);
/* Legacy tab-scoped surface commands target its ACTIVE pane. Prefer the pane-scoped versions below.
 * Rust can turn off the placeholder after its first frame; it starts visible. */
int32_t puzzle_shell_set_placeholder_visible(uint64_t tab_id, int32_t visible);
/* Position IME candidate UI at Rust's actual caret; coordinates are top-left
 * view-local POINTS (divide renderer pixel coordinates by the surface scale).
 * Rejects non-finite values and negative dimensions; initially uses a placeholder rect. */
int32_t puzzle_shell_set_ime_cursor_rect(uint64_t tab_id, double x, double y, double width, double height);

/* A split creates a NEW pane at this edge of the active pane and focuses it.
 * Invalid direction/ID, insufficient room, reentrant or off-thread calls return zero.
 * The addressed tab is selected before splitting; existing panes preserve views/layers. */
enum PuzzleShellSplitDirection {
    PUZZLE_SPLIT_LEFT = 0, PUZZLE_SPLIT_RIGHT = 1, PUZZLE_SPLIT_TOP = 2, PUZZLE_SPLIT_BOTTOM = 3
};
uint64_t puzzle_shell_split_tab(uint64_t tab_id, uint32_t direction);
/* Select a live pane and its containing tab in its current window. */
int32_t puzzle_shell_select_pane(uint64_t pane_id);
/* Last-pane closure closes the tab (and its window when it is the last tab). */
int32_t puzzle_shell_close_pane(uint64_t pane_id);
/* Active pane ID, or zero for a stale tab/off-thread query. Callback-safe. */
uint64_t puzzle_shell_get_active_pane(uint64_t tab_id);
/* Caller-owned array in visual traversal order. out_count required.
 * NULL/0 queries size. 1=success, -1=too small (required count written, buffer untouched),
 * 0=invalid (all outputs untouched). All tree getters are main-thread, callback-safe. */
int32_t puzzle_shell_get_pane_ids(uint64_t tab_id, uint64_t *buffer, size_t capacity, size_t *out_count);
/* Each child ID references another branch or a pane leaf. side_by_side=1 is left/right;
 * 0 is top/bottom. first is left/top. fraction is the preferred first-child proportion
 * of the available extent excluding the divider; minimum-size clamping may limit it.
 * Branch IDs share the stable process-wide namespace. reserved is always zero. */
typedef struct PuzzleShellSplit {
    uint64_t split_id;
    uint64_t first_id;
    uint64_t second_id;
    uint32_t side_by_side;
    uint32_t reserved;
    double fraction;
} PuzzleShellSplit;
/* Parent-first traversal; child references plus ordered leaves describe the complete binary tree.
 * A single-pane tab returns count zero. Buffer/status semantics match get_pane_ids. */
int32_t puzzle_shell_get_splits(uint64_t tab_id, PuzzleShellSplit *buffer, size_t capacity, size_t *out_count);
/* Set a finite proportion strictly between 0 and 1; native dragging uses the same sizing policy. */
int32_t puzzle_shell_resize_split(uint64_t tab_id, uint64_t split_id, double fraction);
/* Leaf-specific rendering/IME controls avoid depending on the current focus selection. */
int32_t puzzle_shell_set_pane_placeholder_visible(uint64_t pane_id, int32_t visible);
int32_t puzzle_shell_set_pane_ime_cursor_rect(uint64_t pane_id, double x, double y, double width, double height);

#ifdef __cplusplus
}
#endif
#endif
