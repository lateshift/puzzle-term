/* OSC 3008 current-context facts are delivered through tick, for both native hosts.
 * RIS changes the screen, not the active remote/privilege path. */
static void context_calls(state_t *state) {
    size_t accepted = 0;
    unsigned before = state->context_changes;
    const char *start = "\033]3008;start=ffi-root;type=shell\033\\\033]3008;start=ffi-ssh;type=remote;targethost=example\033\\";
    assert(puzzle_surface_feed_output(state->surface, bytes(start), &accepted) == PUZZLE_OK);
    wait_count(state, &state->context_changes, before + 1);
    assert(strstr(state->context_json, "ffi-root") && strstr(state->context_json, "ffi-ssh"));
    assert(strstr(state->context_json, "\"targethost\":\"example\""));
    before = state->context_changes;
    assert(puzzle_surface_feed_output(state->surface, bytes("\033c"), &accepted) == PUZZLE_OK);
    /* A following title callback is the source-order barrier for the RIS command. */
    unsigned titles = state->titles;
    assert(puzzle_surface_feed_output(state->surface, bytes("\033]2;context barrier\007"), &accepted) == PUZZLE_OK);
    wait_count(state, &state->titles, titles + 1);
    assert(state->context_changes == before);
    assert(puzzle_surface_feed_output(state->surface, bytes("\033]3008;end=ffi-root\033\\"), &accepted) == PUZZLE_OK);
    wait_count(state, &state->context_changes, before + 1);
    assert(strcmp(state->context_json, "{\"contexts\":[]}") == 0);
}
