/* Native find contract through puzzle.h only; host mode never captures terminal text. */
static void search_event(state_t *state, const puzzle_event_t *ev) {
    if (named(ev, "search/requested")) { state->find_requested++; return; }
    if (!named(ev, "search/state") && !named(ev, "selection/text")) return;
    char json[256] = {0};
    assert(ev->payload.len < sizeof(json));
    memcpy(json, ev->payload.data, ev->payload.len);
    if (named(ev, "selection/text")) {
        assert(strstr(json, "\"request\":101") && strstr(json, "\"text\":\"hello\""));
        state->selected_texts++;
        return;
    }
    const char *count = strstr(json, "\"count\":"), *current = strstr(json, "\"current\":");
    assert(count && current);
    state->find_count = (unsigned)strtoul(count + 8, NULL, 10);
    state->find_current = !strncmp(current + 10, "null", 4) ? -1 : atoi(current + 10);
    state->find_complete = strstr(json, "\"complete\":true") != NULL;
}
/* Completion/state, rather than a fixed sleep, orders each assertion. */
static void search_wait(state_t *state, unsigned count, int current) {
    time_t end = time(NULL) + 120;
    while (!state->find_complete || state->find_count != count || state->find_current != current) {
        assert(time(NULL) < end); pump(state, 1);
    }
}
static void search_contract(state_t *state) {
    assert(puzzle_surface_set_search_ui(state->surface, 2) == PUZZLE_INVALID_ARGUMENT);
    assert(puzzle_surface_set_search_ui(state->surface, 1) == PUZZLE_OK);
    unsigned requested = state->find_requested;
    puzzle_key_t key = {0}; key.logical = bytes("f"); key.base = bytes("f");
    key.physical = bytes("KeyF"); key.modifiers = 8; key.pressed = 1;
    assert(puzzle_surface_key(state->surface, &key) == PUZZLE_OK);
    wait_count(state, &state->find_requested, requested + 1);
    /* This would disappear into the built-in query if host mode only hid its ink. */
    size_t before = state->input_len;
    assert(puzzle_surface_text(state->surface, bytes("z")) == PUZZLE_OK);
    geometry_report(state, before, "z");
    geometry_feed(state, "\033[2J\033[Hhello world\r\nHELLO world\r\nhello again\033[H");
    geometry_cursor(state, 4, 4);
    puzzle_pointer_t pointer = {0}; pointer.kind = 1; pointer.clicks = 2; pointer.held = 1;
    pointer.x = state->cursor[0] + state->cursor[2]/2;
    pointer.y = state->cursor[1] + state->cursor[3]/2;
    assert(puzzle_surface_pointer(state->surface, &pointer) == PUZZLE_OK);
    pointer.kind = 2; pointer.held = 0;
    assert(puzzle_surface_pointer(state->surface, &pointer) == PUZZLE_OK);
    unsigned selected = state->selected_texts;
    assert(puzzle_surface_selected_text(state->surface, 101) == PUZZLE_OK);
    wait_count(state, &state->selected_texts, selected + 1);
    assert(puzzle_surface_search(state->surface, bytes("hello"), 2) == PUZZLE_OK);
    search_wait(state, 3, 2);
    assert(puzzle_surface_search_next(state->surface) == PUZZLE_OK);
    search_wait(state, 3, 0);
    assert(puzzle_surface_search_prev(state->surface) == PUZZLE_OK);
    search_wait(state, 3, 2);
    assert(puzzle_surface_search(state->surface, bytes("hello"), 1) == PUZZLE_OK);
    search_wait(state, 2, 1);
    assert(puzzle_surface_search_clear(state->surface) == PUZZLE_OK);
    search_wait(state, 0, -1);
    assert(puzzle_surface_set_search_ui(state->surface, 0) == PUZZLE_OK);
}
