#include "../Bridge/PuzzleShell.h"
#include <stddef.h>

/* Catch accidental field-layout changes before Swift/Rust could misread an event.
 * These offsets are for the supported 64-bit macOS architectures. */
_Static_assert(sizeof(PuzzleShellEvent) == 144, "ABI v2 record size changed");
_Static_assert(offsetof(PuzzleShellEvent, text) == 120, "ABI v2 text offset changed");
_Static_assert(offsetof(PuzzleShellEvent, scale) == 48, "ABI v2 scale offset changed");

_Static_assert(offsetof(PuzzleShellEvent, pane_id) == 136, "ABI v2 pane offset changed");
_Static_assert(sizeof(PuzzleShellSplit) == 40, "Split topology ABI changed");

/* Exercise actual C symbol linkage rather than testing only Swift function names. */
uint64_t bridge_probe_create_window(void) { return puzzle_shell_create_window(); }

/* Called inside the event callback: every mutation must fail without emitting again. */
int32_t bridge_probe_reentrant_commands(uint64_t window, uint64_t tab) {
    return puzzle_shell_create_window() == 0 &&
           puzzle_shell_create_tab(window) == 0 &&
           puzzle_shell_select_tab(tab) == 0 &&
           puzzle_shell_close_tab(tab) == 0 &&
           puzzle_shell_close_window(window) == 0 &&
           puzzle_shell_move_tab_to_new_window(tab) == 0 &&
           puzzle_shell_move_tab(tab, window, 0) == 0 &&
           puzzle_shell_set_tab_title(tab, NULL, 0) == 0 &&
           puzzle_shell_set_tab_custom_name(tab, NULL, 0) == 0 &&
           puzzle_shell_set_tab_color(tab, 0x6195D5FFu, 1) == 0 &&
           puzzle_shell_collect_tabs(window) == 0 &&
           puzzle_shell_set_placeholder_visible(tab, 0) == 0 &&
           puzzle_shell_set_ime_cursor_rect(tab, 0, 0, 8, 18) == 0 &&
           puzzle_shell_split_tab(tab, PUZZLE_SPLIT_RIGHT) == 0 &&
           puzzle_shell_select_pane(tab) == 0 &&
           puzzle_shell_close_pane(tab) == 0 &&
           puzzle_shell_resize_split(tab, tab, 0.5) == 0 &&
           puzzle_shell_set_pane_placeholder_visible(tab, 0) == 0 &&
           puzzle_shell_set_pane_ime_cursor_rect(tab, 0, 0, 8, 18) == 0 &&
           puzzle_shell_set_event_callback(NULL, NULL) == 0;
}
