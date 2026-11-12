/* Real host ABI: registration, hover permission, acceptance, binary data and disposal.
 * Called for NSView and a borrowed CAMetalLayer. The host copies callback values;
 * no protocol parsing or reply construction is implemented in host glue. */
static void drop_calls(state_t *state) {
    size_t accepted = 0;
    state->input_len = 0;
    unsigned registrations = state->drop_registrations;
    assert(puzzle_surface_feed_output(state->surface, bytes("\033]72;t=a:i=9;text/plain\033\\"), &accepted) == PUZZLE_OK);
    wait_count(state, &state->drop_registrations, registrations + 1);
    uint64_t generation = state->drop_generation;
    puzzle_mime_item_t item = {bytes("text/plain"), bytes("")};
    assert(puzzle_surface_drop(state->surface, generation, 0, 8, 8, 3, &item, 1) == PUZZLE_OK);
    clipboard_wait_bytes(state, ";text/plain\033\\");
    assert(memcmp(state->input, "\033]72;t=m:x=0:y=0:", 17) == 0);
    state->input_len = 0;
    assert(puzzle_surface_feed_output(state->surface, bytes("\033]72;t=r:x=1\033\\"), &accepted) == PUZZLE_OK);
    clipboard_wait_bytes(state, ";EPERM\033\\");
    unsigned acceptances = state->drop_acceptances;
    assert(puzzle_surface_feed_output(state->surface, bytes("\033]72;t=m:o=1;text/plain\033\\"), &accepted) == PUZZLE_OK);
    wait_count(state, &state->drop_acceptances, acceptances + 1);
    assert(puzzle_surface_drop(state->surface, generation, 1, 8, 8, 0, NULL, 0) == PUZZLE_OK);
    clipboard_wait_bytes(state, "\033]72;t=m:x=-1:y=-1:i=9;\033\\");
    state->input_len = 0;
    uint8_t binary[] = {0, 255, 1};
    item.data.data = binary; item.data.len = sizeof(binary);
    assert(puzzle_surface_drop(state->surface, generation, 2, 8, 8, 1, &item, 1) == PUZZLE_OK);
    clipboard_wait_bytes(state, ";text/plain\033\\");
    state->input_len = 0;
    assert(puzzle_surface_feed_output(state->surface, bytes("\033]72;t=r:x=1\033\\\033[5n"), &accepted) == PUZZLE_OK);
    clipboard_wait_bytes(state, "\033]72;t=r:x=1:m=0:i=9;AP8B\033\\\033]72;t=r:x=1:m=0:i=9;\033\\\033[0n");
    unsigned conclusions = state->drop_conclusions;
    assert(puzzle_surface_feed_output(state->surface, bytes("\033]72;t=r:o=1\033\\"), &accepted) == PUZZLE_OK);
    wait_count(state, &state->drop_conclusions, conclusions + 1);
    assert(puzzle_surface_feed_output(state->surface, bytes("\033]72;t=A\033\\"), &accepted) == PUZZLE_OK);
    wait_count(state, &state->drop_registrations, registrations + 2);
    state->input_len = 0;
    assert(puzzle_surface_drop(state->surface, generation, 2, 8, 8, 1, &item, 1) == PUZZLE_OK);
    pump(state, 5);
    assert(state->input_len == 0);
}
