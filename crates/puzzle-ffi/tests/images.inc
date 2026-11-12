/* M15.4: graphics settings and replies use the ordinary C byte-transport seam.
 * A DA query follows every image so the expected byte sequence proves native
 * worker completion cannot be overtaken by ordinary parser replies. */
static void image_exchange(state_t *state, const char *wire, const char *reply) {
    size_t begin = state->input_len, accepted = 0;
    puzzle_bytes_t output = bytes(wire);
    assert(puzzle_surface_feed_output(state->surface, output, &accepted) == PUZZLE_OK);
    assert(accepted == output.len);
    size_t length = strlen(reply);
    /* Functional worker completion has a hang guard, not a debug speed budget. */
    time_t end = time(NULL) + 120;
    while (state->input_len < begin + length) {
        assert(time(NULL) < end);
        pump(state, 1);
    }
    assert(state->input_len == begin + length);
    assert(memcmp(state->input + begin, reply, length) == 0);
}
/* App defaults and surface overrides must reach the actual Session owner. */
static void image_settings(state_t *state) {
    const char *raw = "\033_Ga=T,i=910,f=32,s=1,v=1,C=1;/wAA/w==\033\\\033[c";
    const char *ok = "\033_Gi=910;OK\033\\\033[?62;22c";
    const char *disabled = "\033_Gi=910;ENOSPC:image storage limit exceeded\033\\\033[?62;22c";
    assert(puzzle_app_set_config(state->app, bytes("{\"images\":{\"storageLimitMb\":0}}")) == PUZZLE_OK);
    image_exchange(state, raw, disabled);
    /* Surface settings take precedence over the inherited application value. */
    assert(puzzle_surface_set_config(state->surface, bytes("{\"images\":{\"storageLimitMb\":1}}")) == PUZZLE_OK);
    image_exchange(state, raw, ok);
    image_exchange(state,
        "\033_Ga=T,i=911,f=100,C=1;iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DQAAAEgQGALFXOsAAAAABJRU5ErkJggg==\033\\\033[c",
        "\033_Gi=911;OK\033\\\033[?62;22c");
    /* A live disable retires stored sources; re-enabling cannot resurrect them. */
    assert(puzzle_surface_set_config(state->surface, bytes("{\"images\":{\"storageLimitMb\":0}}")) == PUZZLE_OK);
    image_exchange(state, raw, disabled);
    assert(puzzle_surface_set_config(state->surface, bytes("{\"images\":{\"storageLimitMb\":1}}")) == PUZZLE_OK);
    image_exchange(state, "\033_Ga=p,i=911,C=1\033\\\033[c",
        "\033_Gi=911;ENOENT:image not found\033\\\033[?62;22c");
    image_exchange(state, raw, ok);
    image_exchange(state, "\033_Ga=d,d=A\033\\\033[c", "\033[?62;22c");
    /* Leave the fixture's later PTY sessions on the shipped default allowance. */
    assert(puzzle_app_set_config(state->app, bytes("{\"images\":{\"storageLimitMb\":320}}")) == PUZZLE_OK);
    assert(puzzle_surface_set_config(state->surface, bytes("{\"images\":{\"storageLimitMb\":320}}")) == PUZZLE_OK);
}
