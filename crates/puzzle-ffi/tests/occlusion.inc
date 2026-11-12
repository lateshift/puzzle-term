/* The list is copied atomically; clearing accepts a null empty array. */
static void occlusion_contract(state_t *state) {
    assert(puzzle_surface_set_occluded_rects(state->surface, NULL, 1) == PUZZLE_INVALID_ARGUMENT);
    assert(puzzle_surface_set_occluded_rects(state->surface, NULL, 65) == PUZZLE_INVALID_ARGUMENT);
    puzzle_rect_t rect = {100, 0, 150, -1};
    assert(puzzle_surface_set_occluded_rects(state->surface, &rect, 1) == PUZZLE_INVALID_ARGUMENT);
    rect.height = 60;
    assert(puzzle_surface_set_occluded_rects(state->surface, &rect, 1) == PUZZLE_OK);
    assert(puzzle_surface_set_occluded_rects(state->surface, NULL, 0) == PUZZLE_OK);
}
