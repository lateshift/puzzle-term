/* Native refresh belongs to every live App, not the most recent subscriber.
 * Each changed caret certifies a newly committed image, not a queued parse. */
static void apps_feed(state_t *state, unsigned column) {
    char output[64];
    snprintf(output, sizeof(output), "\033[H\033[%uGx", column);
    size_t accepted = 0;
    assert(puzzle_surface_feed_output(state->surface, bytes(output), &accepted) == PUZZLE_OK);
    assert(accepted == strlen(output));
}
/* Drain both independent host inboxes while CoreVideo drives both render actors.
 * This retains the existing functional hang guard, never a cadence budget. */
static void apps_wait(state_t *a, state_t *b, unsigned ca, unsigned cb) {
    time_t end = time(NULL) + 120;
    while (a->cursor_rects < ca || b->cursor_rects < cb) {
        if (time(NULL) >= end) {
            fprintf(stderr, "M12_APPS cursor delivery stalled: A=%u/%u B=%u/%u\n",
                a->cursor_rects, ca, b->cursor_rects, cb);
            abort();
        }
        pump(a, 1); pump(b, 1);
    }
}
/* Keep two complete Apps alive on the same real display, then close only one. */
static int apps_contract(void) {
    state_t states[2] = {0}; void *windows[2] = {0};
    for (unsigned i = 0; i < 2; i++) {
        state_t *s = &states[i]; s->owner = pthread_self();
        puzzle_app_options_t options = {1, {NULL, 0}, {s, wakeup, event}};
        options.config_path = bytes("/nonexistent-puzzle-ffi-config.json");
        assert(puzzle_app_new(&options, &s->app) == PUZZLE_OK);
        assert(puzzle_app_set_config(s->app, bytes("{\"render\":{\"padding\":8},\"cursor\":{\"blink\":false}}")) == PUZZLE_OK);
        windows[i] = puzzle_test_view_new();
        puzzle_surface_options_t surface = {0};
        surface.native_kind = i == 0 ? 1 : 5;
        surface.native_view = i == 0 ? puzzle_test_view(windows[i]) : puzzle_test_layer(windows[i]);
        surface.transport = 1;
        surface.geometry = (puzzle_geometry_t){800,480,2,puzzle_test_display(windows[i]),60000};
        puzzle_test_geometry(windows[i],800,480,2);
        assert(puzzle_surface_new(s->app,&surface,&s->surface) == PUZZLE_OK);
        assert(puzzle_surface_visibility(s->surface,1,0) == PUZZLE_OK);
    }
    assert(puzzle_test_display(windows[0]) == puzzle_test_display(windows[1]));
    apps_wait(&states[0],&states[1],1,1);
    for (unsigned column = 2; column <= 4; column++) {
        unsigned a = states[0].cursor_rects, b = states[1].cursor_rects;
        apps_feed(&states[0],column); apps_feed(&states[1],column);
        apps_wait(&states[0],&states[1],a+1,b+1);
        for (unsigned i=0; i<2; i++)
            assert(states[i].cursor[0] == 8 + column * states[i].cursor[2]);
    }
    fprintf(stderr,"M12_APPS_CONCURRENT both Apps presented three updates PASS\n");
    assert(puzzle_app_free(states[0].app) == PUZZLE_OK);
    puzzle_test_view_free(windows[0]);
    for (unsigned column=5; column<=7; column++) {
        unsigned before=states[1].cursor_rects;
        apps_feed(&states[1],column);
        wait_count(&states[1],&states[1].cursor_rects,before+1);
        assert(states[1].cursor[0] == 8 + column * states[1].cursor[2]);
    }
    fprintf(stderr,"M12_APPS_SURVIVOR presentation after sibling App teardown PASS\n");
    assert(puzzle_app_free(states[1].app) == PUZZLE_OK);
    puzzle_test_view_free(windows[1]);
    block_copy_contract(); block_copy_contract();
    fprintf(stderr,"M12_APPS_RECREATION sequential App recreation PASS\n");
    return 0;
}
