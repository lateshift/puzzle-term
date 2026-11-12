/* A cursor response is a hit test against the actual presented table, not a
 * duplicated button-layout formula. Enter the output, then probe its right edge
 * until the visible Copy output button supplies the Pointer cursor. */
static void block_copy_click(state_t *state) {
    geometry_feed(state, "\033[2J\033[H\033[?25h\033]133;A\007$ \033]133;B\007echo\r\n\033]133;C\007one\r\ntwo\r\n\033]133;D;0\007");
    geometry_cursor(state, 8, 8 + 3 * state->cursor[3]);
    for (unsigned covered = 0; covered < 2; covered++) {
    puzzle_rect_t find = {250, 8 + state->cursor[3], 150, 44};
    assert(puzzle_surface_set_occluded_rects(state->surface, covered ? &find : NULL, covered) == PUZZLE_OK);
    puzzle_pointer_t point = {0};
    point.x = 24; point.y = 8 + state->cursor[3] + 4;
    assert(puzzle_surface_pointer(state->surface, &point) == PUZZLE_OK);
    time_t end = time(NULL) + 120;
    /* Stay clear of the scrollbar; its cursor cannot certify this button. */
    point.x = covered ? 230 : 368;
    do {
        unsigned before = state->pointer_shapes;
        assert(time(NULL) < end);
        assert(puzzle_surface_pointer(state->surface, &point) == PUZZLE_OK);
        wait_count(state, &state->pointer_shapes, before + 1);
        pump(state, 1);
    } while (!state->pointing);
    unsigned before = state->copy_writes;
    point.kind = 1; point.button = 0; point.held = 1; point.clicks = 1;
    assert(puzzle_surface_pointer(state->surface, &point) == PUZZLE_OK);
    point.kind = 2; point.held = 0;
    assert(puzzle_surface_pointer(state->surface, &point) == PUZZLE_OK);
    wait_count(state, &state->copy_writes, before + 1);
    assert(!strcmp(state->copy_json, "{\"text\":\"one\\ntwo\\n\"}"));
    fprintf(stderr, "M12_BLOCK_COPY occluded=%u presented hit -> policy -> clipboard/write PASS\n", covered);
    }
    assert(puzzle_surface_set_occluded_rects(state->surface, NULL, 0) == PUZZLE_OK);
}
/* Exercise both host target kinds within their shared process App. */
static void block_copy_hosts(state_t *state) {
    assert(puzzle_app_set_config(state->app, bytes("{\"render\":{\"padding\":8},\"cursor\":{\"blink\":false}}")) == PUZZLE_OK);
    for (unsigned kind = 1; kind <= 5; kind += 4) {
        void *window = puzzle_test_view_new();
        puzzle_surface_options_t surface = {0};
        surface.native_kind = kind;
        surface.native_view = kind == 1 ? puzzle_test_view(window) : puzzle_test_layer(window);
        surface.transport = 1;
        surface.geometry = (puzzle_geometry_t){800, 480, 2, puzzle_test_display(window), 60000};
        puzzle_test_geometry(window, 800, 480, 2);
        unsigned before = state->first_frames;
        assert(puzzle_surface_new(state->app, &surface, &state->surface) == PUZZLE_OK);
        wait_count(state, &state->first_frames, before + 1);
        block_copy_click(state);
        assert(puzzle_surface_free(state->surface) == PUZZLE_OK);
        puzzle_test_view_free(window);
    }
}
/* The focused mode runs this same C witness without unrelated fixture work. */
static int block_copy_contract(void) {
    state_t state = {0}; state.owner = pthread_self();
    puzzle_app_options_t options = {1, {NULL, 0}, {&state, wakeup, event}};
    options.config_path = bytes("/nonexistent-puzzle-ffi-config.json");
    assert(puzzle_app_new(&options, &state.app) == PUZZLE_OK);
    block_copy_hosts(&state);
    assert(puzzle_app_free(state.app) == PUZZLE_OK);
    return 0;
}
