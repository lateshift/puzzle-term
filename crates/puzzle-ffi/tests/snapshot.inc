/* Real C consumers copy only the image which actually reached the native layer.
 * Functional hang guards concern setup; only the release task times the copy.
 * Native kinds 1 and 5 share the same API, colour bytes and pixel dimensions.
 */
static void snapshot_before_first(puzzle_surface_t surface) {
    uint32_t width = 99, height = 99;
    assert(puzzle_surface_snapshot(surface, 360, NULL, 0, &width, &height) == PUZZLE_UNSUPPORTED);
    assert(width == 0 && height == 0);
}
/* Empty queries and small buffers reveal the required shape without pixel writes. */
static void snapshot_buffer_contract(puzzle_surface_t surface, uint32_t source_width, uint32_t source_height) {
    uint32_t width = 0, height = 0;
    assert(puzzle_surface_snapshot(surface, 0, NULL, 0, &width, &height) == PUZZLE_INVALID_ARGUMENT);
    assert(puzzle_surface_snapshot(surface, 360, NULL, 0, NULL, &height) == PUZZLE_INVALID_ARGUMENT);
    assert(puzzle_surface_snapshot(surface, 360, NULL, 0, &width, &height) == PUZZLE_EXHAUSTED);
    assert(width == (source_width < 360 ? source_width : 360));
    assert(height == (uint64_t)source_height * width / source_width);
    size_t length = (size_t)width * height * 4;
    uint8_t *pixels = malloc(length + 16); assert(pixels); memset(pixels, 0x5a, length + 16);
    assert(puzzle_surface_snapshot(surface, 360, pixels, length - 1, &width, &height) == PUZZLE_EXHAUSTED);
    for (size_t i = 0; i < length + 16; i++) assert(pixels[i] == 0x5a);
    assert(puzzle_surface_snapshot(surface, 360, pixels, length, &width, &height) == PUZZLE_OK);
    for (size_t i = length; i < length + 16; i++) assert(pixels[i] == 0x5a);
    free(pixels);
}
/* A frame requested by a resize is not the frame that the user has already seen. */
static void snapshot_pending_geometry(state_t *state, puzzle_geometry_t next) {
    uint32_t width = 0, height = 0;
    assert(puzzle_surface_snapshot(state->surface, UINT32_MAX, NULL, 0, &width, &height) == PUZZLE_EXHAUSTED);
    uint32_t previous_width = width, previous_height = height;
    assert(puzzle_surface_resize(state->surface, next) == PUZZLE_OK);
    /* No AppKit pump/tick/display occurs between these calls, regardless of
     * how fast a GPU completion is published. Only main can assign contents. */
    assert(puzzle_surface_snapshot(state->surface, UINT32_MAX, NULL, 0, &width, &height) == PUZZLE_EXHAUSTED);
    assert(width == previous_width && height == previous_height);
}
/* Unequal RGB corners catch both a vertical flip and accidental channel swizzling. */
static void snapshot_colour_contract(state_t *state) {
    geometry_feed(state, "\033]11;#e06c75\033\\\033[2J\033[9;1H\033[48;2;97;175;239m\033[J");
    uint32_t width = 0, height = 0;
    uint8_t *pixels = malloc(800 * 480 * 4); assert(pixels);
    time_t end = time(NULL) + 120; /* Wait for the requested scene, not a speed budget. */
    for (;;) {
        assert(puzzle_surface_snapshot(state->surface, 800, pixels, 800 * 480 * 4, &width, &height) == PUZZLE_OK);
        assert(width == 800 && height == 480);
        size_t top = (30 * width + 30) * 4, bottom = (440 * width + 30) * 4;
        if (pixels[top] == 224 && pixels[top+1] == 108 && pixels[top+2] == 117 &&
            pixels[bottom] == 97 && pixels[bottom+1] == 175 && pixels[bottom+2] == 239) break;
        assert(time(NULL) < end); pump(state, 1);
    }
    free(pixels);
    geometry_feed(state, "\033[0m\033]11;#0e121a\033\\\033[2J\033[H");
}
/* This mode is built with an optimized Rust library; debug never asserts a speed budget. */
static int snapshot_contract(void) {
    state_t state = {0}; state.owner = pthread_self();
    puzzle_app_options_t options = {1, {NULL, 0}, {&state, wakeup, event}};
    options.config_path = bytes("/nonexistent-puzzle-snapshot-config.json");
    assert(puzzle_app_new(&options, &state.app) == PUZZLE_OK);
    assert(puzzle_app_set_config(state.app, bytes("{\"theme\":{\"mode\":\"dark\"},\"cursor\":{\"blink\":false}}")) == PUZZLE_OK);
    for (unsigned kind = 1; kind <= 5; kind += 4) {
        void *window = puzzle_test_view_new();
        void *layer = kind == 5 ? puzzle_test_layer(window) : NULL;
        puzzle_surface_options_t surface = {0}; surface.native_kind = kind; surface.transport = 1;
        surface.native_view = layer ? layer : puzzle_test_view(window);
        surface.geometry = (puzzle_geometry_t){0, 0, 2, puzzle_test_display(window), 60000};
        assert(puzzle_surface_new(state.app, &surface, &state.surface) == PUZZLE_OK);
        snapshot_before_first(state.surface);
        unsigned first = state.first_frames;
        surface.geometry.width = 800; surface.geometry.height = 480;
        puzzle_test_geometry(window, 800, 480, 2);
        assert(puzzle_surface_resize(state.surface, surface.geometry) == PUZZLE_OK);
        geometry_feed(&state, "\033[?25l\033]11;#0e121a\033\\\033[2J");
        wait_count(&state, &state.first_frames, first + 1);
        snapshot_buffer_contract(state.surface, 800, 480);
        snapshot_colour_contract(&state);
        surface.geometry.width = 5120; surface.geometry.height = 2880;
        snapshot_pending_geometry(&state, surface.geometry);
        puzzle_test_geometry(window, 5120, 2880, 2);
        time_t end = time(NULL) + 120; /* Functional hang guard for first 5K image. */
        while (!puzzle_test_colour_extent(window, layer, 5120, 2880)) {
            assert(time(NULL) < end); pump(&state, 1);
        }
        uint32_t width = 0, height = 0;
        assert(puzzle_surface_snapshot(state.surface, 360, NULL, 0, &width, &height) == PUZZLE_EXHAUSTED);
        assert(width == 360 && height == 202);
        size_t length = (size_t)width * height * 4;
        uint8_t *pixels = malloc(length); assert(pixels);
        struct timespec start, finish;
        assert(clock_gettime(CLOCK_MONOTONIC, &start) == 0);
        assert(puzzle_surface_snapshot(state.surface, 360, pixels, length, &width, &height) == PUZZLE_OK);
        assert(clock_gettime(CLOCK_MONOTONIC, &finish) == 0);
        double ms = (finish.tv_sec - start.tv_sec) * 1000.0 + (finish.tv_nsec - start.tv_nsec) / 1e6;
        printf("M12_SNAPSHOT_BUDGET kind=%u source=5120x2880 output=%ux%u ms=%.6f limit_ms=4\n", kind, width, height, ms);
        assert(ms <= 4.0);
        /* Blank screen: all thumbnail bytes equal the encoded theme, with no
         * sRGB re-encoding or BGRA channel leak. Source is already premultiplied. */
        for (size_t i = 0; i < length; i += 4) {
            assert(pixels[i] == 14 && pixels[i+1] == 18 && pixels[i+2] == 26 && pixels[i+3] == 255);
        }
        free(pixels);
        assert(puzzle_surface_free(state.surface) == PUZZLE_OK);
        puzzle_test_view_free(window); pump(&state, 2);
    }
    assert(puzzle_app_free(state.app) == PUZZLE_OK);
    puts("M12_SNAPSHOT_PASS real NSView and host layer; retained image, capacity, exact sRGB and 5K budget");
    return 0;
}
