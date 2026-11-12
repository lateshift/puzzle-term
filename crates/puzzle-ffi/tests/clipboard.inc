/* C callers observe grants and host completions through the real Session queue.
 * Credentials remain in memory only; no test prints the password or clipboard data. */
static void clipboard_wait_bytes(state_t *state, const char *suffix) {
    time_t end = time(NULL) + 120;
    size_t length = strlen(suffix);
    while (state->input_len < length ||
        memcmp(state->input + state->input_len - length, suffix, length) != 0) {
        assert(time(NULL) < end); pump(state, 1);
    }
}
static void clipboard_grant_calls(state_t *state) {
    size_t accepted = 0;
    state->input_len = 0;
    /* The query is an owner-side barrier: mode must precede this gesture. */
    assert(puzzle_surface_feed_output(state->surface,
        bytes("\033[?5522h\033[5n"), &accepted) == PUZZLE_OK);
    clipboard_wait_bytes(state, "\033[0n");
    state->input_len = 0;
    puzzle_bytes_t mime = bytes("text/plain");
    assert(puzzle_surface_clipboard_paste(state->surface, 0, 42, &mime, 1,
        bytes("not pasted in event mode")) == PUZZLE_OK);
    clipboard_wait_bytes(state, "\033]5522;type=read:status=DONE\033\\");
    char listing[4097] = {0};
    memcpy(listing, state->input, state->input_len);
    assert(strstr(listing, "not pasted") == NULL);
    const char *password = strstr(listing, ":pw="); assert(password);
    password += 4;
    const char *end = strchr(password, '\033'); assert(end);
    size_t length = (size_t)(end - password); assert(length < 128);
    char request[512] = {0};
    int size = snprintf(request, sizeof(request),
        "\033]5522;type=read:name=UGFzdGU=:pw=%.*s;dGV4dC9wbGFpbg==\033\\",
        (int)length, password);
    assert(size > 0 && (size_t)size < sizeof(request));
    state->input_len = 0;
    unsigned previous = state->mime_requests;
    assert(puzzle_surface_feed_output(state->surface, bytes(request), &accepted) == PUZZLE_OK);
    wait_count(state, &state->mime_requests, previous + 1);
    assert(state->mime_revision == 42);
    puzzle_mime_item_t item = {bytes("text/plain"), bytes("hello")};
    assert(puzzle_surface_clipboard_mime_answer(state->surface, state->mime_token,
        PUZZLE_CLIPBOARD_DONE, 42, &item, 1) == PUZZLE_OK);
    clipboard_wait_bytes(state, "\033]5522;type=read:status=DONE\033\\");
    state->input[state->input_len] = 0;
    assert(strstr((char *)state->input, ";aGVsbG8=") != NULL);
    /* Reusing a correct credential is refused before any native read. */
    state->input_len = 0;
    assert(puzzle_surface_feed_output(state->surface, bytes(request), &accepted) == PUZZLE_OK);
    clipboard_wait_bytes(state, "\033]5522;type=read:status=EPERM\033\\");
    assert(state->mime_requests == previous + 1);
    assert(puzzle_surface_clipboard_paste(state->surface, 2, 42, &mime, 1,
        bytes("")) == PUZZLE_INVALID_ARGUMENT);
    assert(puzzle_surface_clipboard_paste(state->surface, 0, UINT64_MAX, &mime, 1,
        bytes("")) == PUZZLE_INVALID_ARGUMENT);
    assert(puzzle_surface_clipboard_paste(state->surface, 0, 42, NULL, 1,
        bytes("")) == PUZZLE_INVALID_ARGUMENT);
    state->input_len = 0;
    assert(puzzle_surface_feed_output(state->surface,
        bytes("\033[?5522l\033[5n"), &accepted) == PUZZLE_OK);
    clipboard_wait_bytes(state, "\033[0n");
}
