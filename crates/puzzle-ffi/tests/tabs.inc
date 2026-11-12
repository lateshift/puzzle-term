/* Activity is one edge per acknowledgement, independent of frame cadence. */
static void tabs_activity_contract(state_t *state) {
    unsigned before = state->activities;
    assert(puzzle_surface_activity_seen(state->surface) == PUZZLE_OK);
    geometry_feed(state, "tab output one");
    wait_count(state, &state->activities, before + 1);
    geometry_feed(state, "tab output two");
    assert(state->activities == before + 1);
    assert(puzzle_surface_activity_seen(state->surface) == PUZZLE_OK);
    geometry_feed(state, "tab output three");
    wait_count(state, &state->activities, before + 2);
    geometry_feed(state, "tab output four");
    assert(state->activities == before + 2);
    puts("M12_TABS_ACTIVITY_PASS explicit acknowledgement, no per-chunk events");
}
