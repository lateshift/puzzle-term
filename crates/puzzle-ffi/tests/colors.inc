/* Native chrome receives copied sRGB bytes through the ordinary tick callback. */
static void colors_event(state_t *state, const puzzle_event_t *ev) {
    if (!named(ev, "surface/colors")) return;
    char json[256] = {0};
    assert(ev->payload.len < sizeof(json));
    memcpy(json, ev->payload.data, ev->payload.len);
    const char *bg = strstr(json, "\"background\":[");
    const char *fg = strstr(json, "\"foreground\":[");
    assert(bg && fg);
    assert(sscanf(bg, "\"background\":[%u,%u,%u]", &state->background[0], &state->background[1], &state->background[2]) == 3);
    assert(sscanf(fg, "\"foreground\":[%u,%u,%u]", &state->foreground[0], &state->foreground[1], &state->foreground[2]) == 3);
    for (unsigned i = 0; i < 3; i++) assert(state->background[i] <= 255 && state->foreground[i] <= 255);
    assert(strstr(json, "\"dark\":true") || strstr(json, "\"dark\":false"));
    state->colors_dark = strstr(json, "\"dark\":true") != NULL;
    state->colors++;
}
/* Creation, OSC changes/reset, appearance and duplicate suppression share one flow. */
static void colors_contract(state_t *state) {
    wait_count(state, &state->colors, 1);
    assert(state->colors == 1);
    unsigned background[3], foreground[3];
    memcpy(background, state->background, sizeof(background));
    memcpy(foreground, state->foreground, sizeof(foreground));
    unsigned before = state->colors;
    geometry_feed(state, "\033]10;#112233\007\033]11;#fefefe\007");
    wait_count(state, &state->colors, before + 1);
    assert(state->foreground[0] == 17 && state->foreground[1] == 34 && state->foreground[2] == 51);
    assert(state->background[0] == 254 && state->background[1] == 254 && state->background[2] == 254);
    assert(!state->colors_dark);
    before = state->colors;
    geometry_feed(state, "ordinary output"); pump(state, 5);
    assert(state->colors == before);
    geometry_feed(state, "\033]110\007\033]111\007");
    wait_count(state, &state->colors, before + 1);
    assert(!memcmp(background, state->background, sizeof(background)));
    assert(!memcmp(foreground, state->foreground, sizeof(foreground)));
    before = state->colors;
    assert(puzzle_surface_set_config(state->surface, bytes("{\"theme\":{\"mode\":\"light\"}}")) == PUZZLE_OK);
    wait_count(state, &state->colors, before + 1);
    assert(!state->colors_dark);
    before = state->colors;
    assert(puzzle_surface_set_config(state->surface, bytes("{\"theme\":{\"mode\":\"dark\"}}")) == PUZZLE_OK);
    wait_count(state, &state->colors, before + 1);
    assert(state->colors_dark);
    assert(!memcmp(background, state->background, sizeof(background)));
}
