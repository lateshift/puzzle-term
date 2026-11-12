/* Two real native targets exercise the same surface setup as harness and AppKit.
 * The Rust diagnostic copies each acquired drawable before its actual presentation.
 * The host separately checks compositor metadata, including a pre-existing P3 layer.
 */
void puzzle_test_layer_wide_gamut(void *window, void *pointer);
int puzzle_test_layer_srgb(void *window, void *pointer, unsigned kind, const char *phase);
int puzzle_test_colour_extent(void *window, void *pointer, uint32_t width, uint32_t height);
void puzzle_test_colour_resize(void *window, uint32_t width, uint32_t height);
int puzzle_test_contents_display(void *window, void *pointer, uint32_t width, uint32_t height, double scale);
int puzzle_test_background(void *window, void *pointer, unsigned r, unsigned g, unsigned b);
/* The child's backing must follow live settings even if its retained image lags.
 * This is a functional hang guard, with no new timing threshold or cadence policy. */
static void background_theme(state_t *state, void *window, void *layer, const char *config,
    unsigned r, unsigned g, unsigned b, int app) {
    assert((app ? puzzle_app_set_config(state->app,bytes(config)) :
        puzzle_surface_set_config(state->surface,bytes(config))) == PUZZLE_OK);
    time_t finish=time(NULL)+120;
    while (!puzzle_test_background(window,layer,r,g,b)) {
        assert(time(NULL)<finish); puzzle_test_pump(); assert(puzzle_app_tick(state->app)==PUZZLE_OK);
    }
}
static int surface_colour(void) {
    state_t state = {0}; state.owner = pthread_self();
    puzzle_app_options_t options = {1, {NULL, 0}, {&state, wakeup, event}};
    options.config_path = bytes("/nonexistent-puzzle-colour-config.json");
    assert(puzzle_app_new(&options, &state.app) == PUZZLE_OK);
    assert(puzzle_app_set_config(state.app, bytes("{\"theme\":{\"mode\":\"dark\",\"dark\":\"puzzle-dark\"}}")) == PUZZLE_OK);
    for (unsigned kind = 1; kind <= 5; kind += 4) {
        void *window = puzzle_test_view_new();
        void *layer = kind == 5 ? puzzle_test_layer(window) : NULL;
        if (layer) puzzle_test_layer_wide_gamut(window, layer);
        puzzle_surface_options_t surface = {0};
        surface.native_kind = kind;
        surface.native_view = layer ? layer : puzzle_test_view(window);
        surface.transport = 1;
        surface.geometry = (puzzle_geometry_t){800, 480, 2, puzzle_test_display(window), 60000};
        assert(puzzle_surface_new(state.app, &surface, &state.surface) == PUZZLE_OK);
        // The backing exists before the first actual image has arrived.
        assert(puzzle_test_background(window,layer,14,18,26));
        wait_count(&state, &state.first_frames, kind == 1 ? 1 : 2);
        assert(puzzle_test_layer_srgb(window, layer, kind, "configure"));
        unsigned cursor_before = state.cursor_rects;
        // Seed hostile metadata again: resize must reapply sRGB after wgpu clears it.
        puzzle_test_layer_wide_gamut(window, layer);
        puzzle_test_colour_resize(window, 600, 360);
        assert(puzzle_surface_resize(state.surface,
            (puzzle_geometry_t){600, 360, 2, puzzle_test_display(window), 60000}) == PUZZLE_OK);
        time_t end = time(NULL) + 120; // Deadlock guard, not a speed assertion.
        while (!puzzle_test_colour_extent(window, layer, 600, 360)) {
            assert(time(NULL) < end);
            puzzle_test_pump(); assert(puzzle_app_tick(state.app) == PUZZLE_OK);
        }
        // Cursor events follow actual presentation with the new geometry.
        // Use that barrier instead of reading drawableSize mid-configure.
        wait_count(&state, &state.cursor_rects, cursor_before + 1);
        assert(puzzle_test_layer_srgb(window, layer, kind, "resize"));
        // Populate Unicode while exercising abrupt host geometry changes. The
        // actual native display callback runs with no AppKit live-resize phase.
        puzzle_bytes_t unicode = bytes("\033[HAlpha 一二三 é ñ Ω 🦀\r\nsecond line\r\n");
        size_t accepted = 0;
        assert(puzzle_surface_feed_output(state.surface, unicode, &accepted) == PUZZLE_OK && accepted == unicode.len);
        const puzzle_geometry_t jumps[] = {
            {1300,800,2,0,60000}, {300,600,2,0,60000},
            {900,500,1,0,60000}, {600,360,2,0,60000}
        };
        for (unsigned jump=0; jump<sizeof(jumps)/sizeof(jumps[0]); ++jump) {
            puzzle_geometry_t geometry=jumps[jump]; geometry.display=puzzle_test_display(window);
            // Multiple changes within one transaction must retain only the last.
            puzzle_test_geometry(window,geometry.width+20,geometry.height,geometry.scale);
            assert(puzzle_surface_resize(state.surface,(puzzle_geometry_t){geometry.width+20,geometry.height,geometry.scale,geometry.display,geometry.millihertz}) == PUZZLE_OK);
            puzzle_test_geometry(window,geometry.width,geometry.height,geometry.scale);
            assert(puzzle_surface_resize(state.surface,geometry) == PUZZLE_OK);
            puzzle_test_contents_display(window,layer,geometry.width,geometry.height,geometry.scale);
            // Timeout is permitted by D-60; asynchronous completion must still
            // deliver exact geometry, never stretch the retained previous image.
            time_t finish=time(NULL)+120;
            while (!puzzle_test_colour_extent(window,layer,geometry.width,geometry.height)) {
                assert(time(NULL)<finish); puzzle_test_pump(); assert(puzzle_app_tick(state.app)==PUZZLE_OK);
            }
        }
        background_theme(&state,window,layer,"{\"theme\":{\"mode\":\"light\"}}",250,250,250,0);
        background_theme(&state,window,layer,"{\"theme\":{\"mode\":\"dark\",\"dark\":\"solarized-dark\"}}",0,43,54,0);
        background_theme(&state,window,layer,"{\"theme\":{\"mode\":\"dark\",\"dark\":\"puzzle-dark\"}}",14,18,26,1);
        assert(puzzle_surface_free(state.surface) == PUZZLE_OK);
        puzzle_test_view_free(window);
    }
    assert(puzzle_app_free(state.app) == PUZZLE_OK);
    puts("M17_C_HOST_PASS M18 real NSView and borrowed CAMetalLayer colour witnesses");
    return 0;
}
