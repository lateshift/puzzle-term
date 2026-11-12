/* Full protocol → callback → completion → ordered reply, through puzzle.h only.
 * Both native host paths call this after ordinary output has reached their Session. */
static void notification_calls(state_t *state) {
    size_t accepted = 0;
    state->input_len = 0;
    unsigned shows = state->notification_shows;
    assert(puzzle_surface_feed_output(state->surface,
        bytes("\033]99;i=origin:u=2:o=always:a=report:c=1:d=0;Title\033\\"
            "\033]99;i=origin:p=body;Body\033\\\033[5n"), &accepted) == PUZZLE_OK);
    wait_count(state, &state->notification_shows, shows + 1);
    clipboard_wait_bytes(state, "\033[0n");
    assert(state->input_len == 4); /* Posting must not wait for host feedback. */
    assert(puzzle_surface_notification_feedback(state->surface, bytes(state->notification_token), 4) == PUZZLE_INVALID_ARGUMENT);
    assert(puzzle_surface_notification_feedback(state->surface, bytes(""), 0) == PUZZLE_INVALID_ARGUMENT);
    assert(puzzle_surface_notification_feedback(state->surface, bytes("wrong:token"), 0) == PUZZLE_OK);
    unsigned focus = state->notification_focus;
    unsigned closes = state->notification_closes;
    assert(puzzle_surface_notification_feedback(state->surface, bytes(state->notification_token), 0) == PUZZLE_OK);
    clipboard_wait_bytes(state, "\033]99;i=origin;\033\\\033]99;i=origin:p=close;\033\\");
    wait_count(state, &state->notification_focus, focus + 1);
    wait_count(state, &state->notification_closes, closes + 1);
    size_t length = state->input_len;
    /* A second callback for an already completed generation cannot duplicate reports. */
    assert(puzzle_surface_notification_feedback(state->surface, bytes(state->notification_token), 1) == PUZZLE_OK);
    pump(state, 5);
    assert(state->input_len == length);
}
