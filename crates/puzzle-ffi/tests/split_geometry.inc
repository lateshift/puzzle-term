/* Physical pixels supplied by the host are authoritative for fractional splits.
 * Every fresh odd-size surface must emit firstFrame with exact native contents;
 * an even-to-odd resize must replace them without emitting a second firstFrame.
 */
static void split_geometry_contract(void) {
    state_t state = {0}; state.owner = pthread_self();
    puzzle_app_options_t options = {1, {NULL, 0}, {&state, wakeup, event}};
    options.config_path = bytes("/nonexistent-puzzle-ffi-config.json");
    assert(puzzle_app_new(&options, &state.app) == PUZZLE_OK);
    const uint32_t sizes[][2] = {{991,1192}, {1001,777}};
    for (unsigned scale = 1; scale <= 2; scale++) for (unsigned i = 0; i < 2; i++) {
        void *window = puzzle_test_view_new();
        void *layer = puzzle_test_split_layer(window, 1000, 1192, scale);
        puzzle_surface_options_t surface = {0};
        surface.native_kind = 5; surface.native_view = layer; surface.transport = 1;
        surface.geometry = (puzzle_geometry_t){0, 0, scale, puzzle_test_display(window), 60000};
        /* Cover both zero-size creation followed by resize and a complete
         * initial physical key which must work without any resize call. */
        if (i == 1) {
            puzzle_test_split_resize(window, layer, sizes[i][0], sizes[i][1], scale);
            surface.geometry.width = sizes[i][0]; surface.geometry.height = sizes[i][1];
        }
        assert(puzzle_surface_new(state.app, &surface, &state.surface) == PUZZLE_OK);
        if (i == 0) {
            puzzle_test_split_resize(window, layer, sizes[i][0], sizes[i][1], scale);
            surface.geometry.width = sizes[i][0]; surface.geometry.height = sizes[i][1];
            assert(puzzle_surface_resize(state.surface, surface.geometry) == PUZZLE_OK);
        }
        /* CA's remaining disagreement must resolve in favour of the host key,
         * not suppress demand forever or replace it with the stale container. */
        if (i == 1) puzzle_test_stale_container(layer);
        unsigned first = state.first_frames;
        geometry_feed(&state, "split ready");
        pump(&state, 2);
        /* Fails immediately on point rounding, before the functional hang guard. */
        assert(puzzle_test_child_geometry(layer, sizes[i][0], sizes[i][1], scale));
        wait_count(&state, &state.first_frames, first + 1);
        assert(puzzle_test_layer_size(layer, sizes[i][0], sizes[i][1]));
        /* A split-like width transition must not keep an old even-size image. */
        for (unsigned width = 1000; width >= 991; width -= 9) {
            puzzle_test_split_resize(window, layer, width, sizes[i][1], scale);
            surface.geometry.width = width;
            assert(puzzle_surface_resize(state.surface, surface.geometry) == PUZZLE_OK);
            time_t end = time(NULL) + 120;
            while (!puzzle_test_layer_size(layer, width, sizes[i][1])) {
                assert(time(NULL) < end); pump(&state, 1);
            }
            assert(puzzle_test_child_geometry(layer, width, sizes[i][1], scale));
        }
        assert(state.first_frames == first + 1);
        assert(puzzle_surface_free(state.surface) == PUZZLE_OK);
        puzzle_test_view_free(window); pump(&state, 2);
    }
    assert(puzzle_app_free(state.app) == PUZZLE_OK);
    puts("M12_SPLIT_GEOMETRY_PASS odd physical sizes at scale 1/2, firstFrame and even-to-odd resize");
}
