/* C-interface regression: host points use the same logical grid as the caret. */
/* Protocol completion, never a fixed delay, orders setup before pointer controls. */
static void geometry_feed(state_t *state, const char *text) {
    size_t accepted = 0, before = state->input_len;
    puzzle_bytes_t output = bytes(text);
    assert(puzzle_surface_feed_output(state->surface, output, &accepted) == PUZZLE_OK);
    assert(accepted == output.len);
    output = bytes("\033[6n");
    assert(puzzle_surface_feed_output(state->surface, output, &accepted) == PUZZLE_OK);
    assert(accepted == output.len);
    time_t end = time(NULL) + 120;
    while (state->input_len == before || state->input[state->input_len - 1] != 'R') {
        assert(time(NULL) < end); pump(state, 1);
    }
    /* This setup query is not part of the gesture transcript under examination. */
    state->input_len = before;
}
/* Functional readiness uses the existing hang guard, not a frame-time budget. */
static void geometry_cursor(state_t *state, double x, double y) {
    time_t end = time(NULL) + 120;
    while (state->cursor[0] != x || state->cursor[1] != y) {
        assert(time(NULL) < end); pump(state, 1);
    }
}
static void geometry_report(state_t *state, size_t before, const char *expected) {
    size_t length = strlen(expected);
    time_t end = time(NULL) + 120;
    while (state->input_len < before + length) {
        assert(time(NULL) < end); pump(state, 1);
    }
    assert(state->input_len == before + length);
    assert(memcmp(state->input + before, expected, length) == 0);
}
/* Compare exact protocol bytes, including clicks just outside the grid. */
static void geometry_click(state_t *state, double x, double y, unsigned col, unsigned row) {
    puzzle_pointer_t pointer = {0};
    pointer.kind = 1; pointer.held = 1; pointer.clicks = 1;
    pointer.x = x; pointer.y = y;
    /* Legacy metric fields are deliberately absent: Puzzle owns cell geometry. */

    size_t before = state->input_len;
    assert(puzzle_surface_pointer(state->surface, &pointer) == PUZZLE_OK);
    char expected[64];
    snprintf(expected, sizeof(expected), "\033[<0;%u;%uM", col, row);
    geometry_report(state, before, expected);
    pointer.kind = 2; pointer.held = 0;
    before = state->input_len;
    assert(puzzle_surface_pointer(state->surface, &pointer) == PUZZLE_OK);
    snprintf(expected, sizeof(expected), "\033[<0;%u;%um", col, row);
    geometry_report(state, before, expected);
}
/* The cursor event is derived from the geometry of the actual presented frame. */
static void geometry_cells(state_t *state, double padding) {
    geometry_feed(state, "\033[H\033[?1000h\033[?1006h");
    geometry_cursor(state, padding, padding);
    double w = state->cursor[2], h = state->cursor[3];
    geometry_click(state, padding + w / 2, padding + h / 2, 1, 1);
    geometry_click(state, padding + w - 0.01, padding + h - 0.01, 1, 1);
    geometry_click(state, padding + w + 0.01, padding + h + 0.01, 2, 2);
    geometry_click(state, padding - 0.01, padding - 0.01, 1, 1);
    /* Request the last grid cell through CUP, then inspect its rendered position. */
    geometry_feed(state, "\033[999;999H");
    unsigned cols = (unsigned)((400 - padding * 2) / w);
    unsigned rows = (unsigned)((240 - padding * 2) / h);
    geometry_cursor(state, padding + (cols - 1) * w, padding + (rows - 1) * h);
    /* Reloads must notify the byte transport of exactly the grid being drawn. */
    if (padding == 0 || padding == 8) {
        time_t end = time(NULL) + 120;
        while (state->grid_columns != cols || state->grid_rows != rows) {
            assert(time(NULL) < end); pump(state, 1);
        }
    }
    /* Keep the last-cell sample left of the overlay scrollbar hit strip. */
    geometry_click(state, padding + cols * w - w / 2, padding + rows * h - 0.01, cols, rows);
    geometry_click(state, 401, 241, cols, rows);
    /* Pixel-mode wheel reporting must subtract padding and scale exactly once. */
    geometry_feed(state, "\033[?1016h");
    puzzle_pointer_t pointer = {0}; pointer.x = padding + w / 2; pointer.y = padding + h / 2;
    size_t before = state->input_len;
    assert(puzzle_surface_scroll(state->surface, -1, 0, &pointer) == PUZZLE_OK);
    char expected[64];
    snprintf(expected, sizeof(expected), "\033[<64;%u;%uM", (unsigned)w + 1, (unsigned)h + 1);
    geometry_report(state, before, expected);
    fprintf(stderr, "M17_POINTER_GEOMETRY padding=%.1f cols=%u rows=%u clicks/wheel/caret/PTY PASS\n", padding, cols, rows);
    geometry_feed(state, "\033[?1016l\033[?1000l\033[?1006l");
}
static void pointer_geometry(state_t *state) {
    size_t saved = state->input_len;
    /* Default settings, surface patch, then app patch all use the same path. */
    geometry_cells(state, 4);
    assert(puzzle_surface_set_config(state->surface, bytes("{\"render\":{\"padding\":0}}")) == PUZZLE_OK);
    geometry_cells(state, 0);
    assert(puzzle_app_set_config(state->app, bytes("{\"render\":{\"padding\":8},\"scrolling\":{\"smooth\":false}}")) == PUZZLE_OK);
    geometry_cells(state, 8);
    /* Restore the original transcript and caret expected by the remaining witness. */
    state->input_len = saved;
    geometry_feed(state, "\033[2;3H");
}

/* Reload before any presentation exercises the pooled driver's startup seam. */
static void geometry_early_reload(void) {
    state_t state = {0}; state.owner = pthread_self();
    puzzle_app_options_t options = {1, {NULL, 0}, {&state, wakeup, event}};
    options.config_path = bytes("/nonexistent-puzzle-ffi-config.json");
    assert(puzzle_app_new(&options, &state.app) == PUZZLE_OK);
    void *window = puzzle_test_view_new();
    puzzle_surface_options_t surface = {0};
    surface.native_kind = 1; surface.native_view = puzzle_test_view(window); surface.transport = 1;
    surface.geometry = (puzzle_geometry_t){800, 480, 2, puzzle_test_display(window), 60000};
    assert(puzzle_surface_new(state.app, &surface, &state.surface) == PUZZLE_OK);
    assert(puzzle_surface_set_config(state.surface, bytes("{\"render\":{\"padding\":0}}")) == PUZZLE_OK);
    wait_count(&state, &state.first_frames, 1);
    geometry_cells(&state, 0);
    assert(puzzle_surface_free(state.surface) == PUZZLE_OK);
    puzzle_test_view_free(window);
    assert(puzzle_app_free(state.app) == PUZZLE_OK);
}
