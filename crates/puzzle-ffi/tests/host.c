/* C ABI witness: valid host calls, callback affinity, bounded input and lifecycle.
 * This fixture uses AppKit only to supply owned native targets. Terminal work
 * always enters through the installed header, never private Rust diagnostics.
 */
#include "puzzle.h"
#include <assert.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

void *puzzle_test_view_new(void);
void *puzzle_test_view(void *window);
void *puzzle_test_layer(void *window);
int puzzle_test_layer_is_top_left(void *layer);
void puzzle_test_geometry(void *handle, uint32_t width, uint32_t height, double scale);
int puzzle_test_layer_size(void *layer, uint32_t width, uint32_t height);
uint64_t puzzle_test_display(void *window);
void puzzle_test_pump(void);
void puzzle_test_view_free(void *window);
void *puzzle_test_transfer(void *window, void *expected_layer);
void *puzzle_test_split_layer(void *, uint32_t, uint32_t, double);
void puzzle_test_split_resize(void *, void *, uint32_t, uint32_t, double);
int puzzle_test_child_geometry(void *, uint32_t, uint32_t, double);
void puzzle_test_stale_container(void *);

/* Worker notifications cannot invoke host event handling or touch its UI. */
typedef struct state {
    pthread_t owner;
    atomic_uint wakes;
    puzzle_app_t app;
    puzzle_surface_t surface;
    unsigned titles, bells, searches, releases, decorations, statuses;
    unsigned activities;
    unsigned copy_writes, pointer_shapes;
    int pointing;
    char copy_json[256];
    unsigned first_frames, cursor_rects, offscreen_rects;
    unsigned find_requested, find_count, find_complete, selected_texts;
    unsigned colors, background[3], foreground[3], colors_dark;
    int find_current;
    unsigned mime_requests;
    unsigned notification_shows, notification_closes, notification_focus;
    char notification_token[65];
    uint64_t mime_token, mime_revision;
    uint64_t drop_generation;
    unsigned drop_registrations, drop_acceptances, drop_conclusions;
    unsigned context_changes;
    char context_json[2048];
    unsigned grid_columns, grid_rows;
    unsigned host_clock;
    uint64_t display;
    unsigned close_on_first, callbacks_after_close;
    puzzle_surface_t closed_in_callback;
    double cursor[4];
    uint64_t marker, decoration;
    uint8_t input[4096];
    size_t input_len;
} state_t;
/* Search fixture observations share the same tick-only callback. */
static void search_event(state_t *state, const puzzle_event_t *ev);
/* Surface colours are delivered on the same host-thread tick as other events. */
static void colors_event(state_t *state, const puzzle_event_t *ev);
static puzzle_bytes_t bytes(const char *text) {
    puzzle_bytes_t value = {(const uint8_t *)text, strlen(text)};
    return value;
}
static void wakeup(void *userdata) {
    state_t *state = userdata;
    atomic_fetch_add_explicit(&state->wakes, 1, memory_order_relaxed);
}
static int named(const puzzle_event_t *event, const char *name) {
    return event->name.len == strlen(name) && memcmp(event->name.data, name, event->name.len) == 0;
}
/* Callback strings are copied before returning; no borrowed event data survives. */
/* Optional fact hooks are independent of the standard callback contract. */
static void optional_event(state_t *state, const puzzle_event_t *ev);

static void event(void *userdata, puzzle_surface_t surface, const puzzle_event_t *ev) {
    state_t *state = userdata;
    assert(pthread_equal(state->owner, pthread_self()));
    assert(surface != 0);
    if (surface == state->closed_in_callback && !named(ev, "surface/released"))
        state->callbacks_after_close++;
    assert(puzzle_app_tick(state->app) == PUZZLE_BUSY);
    assert(puzzle_app_free(state->app) == PUZZLE_BUSY);
    if (named(ev, "clipboard/write")) {
        assert(ev->payload.len < sizeof(state->copy_json));
        memcpy(state->copy_json, ev->payload.data, ev->payload.len);
        state->copy_json[ev->payload.len] = 0; state->copy_writes++;
    }
    if (named(ev, "pointer/shape")) {
        state->pointing = ev->payload.len == 9 && !memcmp(ev->payload.data, "\"Pointer\"", 9);
        state->pointer_shapes++;
    }
    if (named(ev, "transport/input")) {
        assert(state->input_len + ev->payload.len <= sizeof(state->input));
        memcpy(state->input + state->input_len, ev->payload.data, ev->payload.len);
        state->input_len += ev->payload.len;
    }
    if (named(ev, "transport/reply")) {
        /* Receipt and wire bytes are one call-scoped buffer; writing to this
         * fixture's sink completes consumption before returning credit. */
        assert(ev->payload.len >= 8);
        uint64_t receipt = 0;
        for (unsigned i = 0; i < 8; i++) receipt |= (uint64_t)ev->payload.data[i] << (8 * i);
        size_t length = ev->payload.len - 8;
        assert(state->input_len + length <= sizeof(state->input));
        memcpy(state->input + state->input_len, ev->payload.data + 8, length);
        state->input_len += length;
        assert(puzzle_surface_reply_complete(surface, receipt) == PUZZLE_OK);
    }
    if (named(ev, "transport/resize")) {
        /* Only copy callback-scoped JSON; no borrowed payload survives tick. */
        char payload[256] = {0};
        assert(ev->payload.len < sizeof(payload));
        memcpy(payload, ev->payload.data, ev->payload.len);
        const char *cols = strstr(payload, "\"cols\":");
        const char *rows = strstr(payload, "\"rows\":");
        assert(cols && rows);
        state->grid_columns = (unsigned)strtoul(cols + 7, NULL, 10);
        state->grid_rows = (unsigned)strtoul(rows + 7, NULL, 10);
    }
    if (named(ev, "notification/show")) {
        char payload[4096] = {0};
        assert(ev->payload.len < sizeof(payload));
        memcpy(payload, ev->payload.data, ev->payload.len);
        assert(strstr(payload, "\"title\":\"Title\"") && strstr(payload, "\"body\":\"Body\""));
        assert(strstr(payload, "\"urgency\":2") && strstr(payload, "\"occasion\":\"always\""));
        assert(strstr(payload, "\"focus\":true") && strstr(payload, "\"report\":true"));
        assert(strstr(payload, "\"closeReport\":true") && strstr(payload, "\"id\":\"origin\""));
        const char *token = strstr(payload, "\"token\":\"");
        assert(token); token += 9;
        const char *end = strchr(token, '"'); assert(end && end - token < 65);
        memcpy(state->notification_token, token, (size_t)(end-token));
        state->notification_token[end-token] = 0;
        state->notification_shows++;
    }
    if (named(ev, "notification/close")) state->notification_closes++;
    if (named(ev, "notification/focus")) state->notification_focus++;
    if (named(ev, "drop/registration") || named(ev, "drop/acceptance") || named(ev, "drop/concluded")) {
        char payload[1024] = {0};
        assert(ev->payload.len < sizeof(payload));
        memcpy(payload, ev->payload.data, ev->payload.len);
        const char *generation = strstr(payload, "\"generation\":\""); assert(generation);
        state->drop_generation = strtoull(generation + 14, NULL, 10);
        assert(state->drop_generation != 0);
        if (named(ev, "drop/registration")) state->drop_registrations++;
        if (named(ev, "drop/acceptance")) { assert(strstr(payload, "\"operation\":1")); state->drop_acceptances++; }
        if (named(ev, "drop/concluded")) { assert(strstr(payload, "\"operation\":1")); state->drop_conclusions++; }
    }
    if (named(ev, "clipboard/mime")) {
        char payload[2048] = {0};
        assert(ev->payload.len < sizeof(payload));
        memcpy(payload, ev->payload.data, ev->payload.len);
        const char *token = strstr(payload, "\"token\":\"");
        assert(token); state->mime_token = strtoull(token + 9, NULL, 10);
        assert(state->mime_token); state->mime_requests++;
        const char *revision = strstr(payload, "\"revision\":\"");
        state->mime_revision = revision ? strtoull(revision + 12, NULL, 10) : UINT64_MAX;
    }
    if (named(ev, "terminal/contextsChanged")) {
        assert(ev->payload.len < sizeof(state->context_json));
        memcpy(state->context_json, ev->payload.data, ev->payload.len);
        state->context_json[ev->payload.len] = 0;
        state->context_changes++;
    }
    if (named(ev, "surface/activity")) {
        assert(ev->payload.len == 4 && !memcmp(ev->payload.data, "null", 4));
        state->activities++;
    }
    if (named(ev, "terminal/titleChanged")) state->titles++;
    if (named(ev, "surface/firstFrame")) {
        assert(ev->payload.len == 4 && memcmp(ev->payload.data, "null", 4) == 0);
        state->first_frames++;
        if (state->close_on_first) {
            state->close_on_first = 0;
            assert(puzzle_surface_free(surface) == PUZZLE_OK);
            state->closed_in_callback = surface;
        }
    }
    if (named(ev, "surface/cursorRect")) {
        if (ev->payload.len == 4 && memcmp(ev->payload.data, "null", 4) == 0) {
            state->offscreen_rects++;
        } else {
            char rect[256] = {0};
            assert(ev->payload.len < sizeof(rect));
            memcpy(rect, ev->payload.data, ev->payload.len);
            assert(sscanf(rect, "[%lf,%lf,%lf,%lf]", &state->cursor[0], &state->cursor[1],
                &state->cursor[2], &state->cursor[3]) == 4);
            assert(state->cursor[2] > 0 && state->cursor[3] > 0);
            state->cursor_rects++;
        }
    }
    if (named(ev, "terminal/bell")) state->bells++;
    if (named(ev, "search/state")) state->searches++;
    search_event(state, ev);
    colors_event(state, ev);
    optional_event(state, ev);
    if (named(ev, "surface/released")) state->releases++;
    if (named(ev, "surface/markerAdded") || named(ev, "surface/decorationAdded")) {
        char payload[512];
        assert(ev->payload.len < sizeof(payload));
        memcpy(payload, ev->payload.data, ev->payload.len);
        payload[ev->payload.len] = 0;
        const char *key = named(ev, "surface/markerAdded") ? "\"marker\":" : "\"id\":";
        const char *start = strstr(payload, key);
        assert(start != NULL);
        uint64_t id = strtoull(start + strlen(key), NULL, 10);
        if (named(ev, "surface/markerAdded")) state->marker = id;
        else { state->decoration = id; state->decorations++; }
    }
}
/* A wall-clock deadline is a deadlock guard only, never a performance budget. */
static void pump(state_t *state, unsigned turns) {
    for (unsigned i = 0; i < turns; i++) {
        if (state->host_clock) {
            /* Inject a monotonic physical grid, not one refresh per pump turn.
             * Duplicates within a turn must never create extra submissions. */
            struct timespec now;
            assert(clock_gettime(CLOCK_MONOTONIC, &now) == 0);
            uint64_t ns = (uint64_t)now.tv_sec * 1000000000 + (uint64_t)now.tv_nsec;
            uint64_t period = 16666667, index = ns / period;
            uint64_t delay = period - ns % period;
            assert(puzzle_surface_refresh(state->surface, state->display, index, delay, period) == PUZZLE_OK);
            assert(puzzle_surface_refresh(state->surface, state->display, index, delay, period) == PUZZLE_OK);
        }
        assert(puzzle_app_tick(state->app) == PUZZLE_OK);
        puzzle_test_pump();
    }
}
static void wait_count_at(state_t *state, const unsigned *value, unsigned minimum,
    const char *name, unsigned line) {
    time_t end = time(NULL) + 120;
    while (*value < minimum) {
        if (time(NULL) >= end) {
            fprintf(stderr, "host.c:%u: %s expected >=%u, got %u; first=%u cursor=%u offscreen=%u\n",
                line, name, minimum, *value, state->first_frames, state->cursor_rects, state->offscreen_rects);
            abort();
        }
        pump(state, 1);
    }
}
#define wait_count(state, value, minimum) wait_count_at(state, value, minimum, #value, __LINE__)
/* Foreign-thread misuse is rejected before the native App allocation is accessed. */
static void *wrong_thread(void *userdata) {
    state_t *state = userdata;
    assert(puzzle_app_tick(state->app) == PUZZLE_WRONG_THREAD);
    assert(puzzle_app_free(state->app) == PUZZLE_WRONG_THREAD);
    assert(puzzle_surface_focus(state->surface, 1) == PUZZLE_WRONG_THREAD);
    assert(puzzle_surface_refresh(state->surface, state->display, 1, 1, 1) == PUZZLE_WRONG_THREAD);
    return NULL;
}
static void input_calls(state_t *state) {
    puzzle_surface_t s = state->surface;
    puzzle_key_t key = {0};
    key.logical = bytes("x"); key.base = bytes("x"); key.physical = bytes("KeyX");
    key.text = bytes("x"); key.pressed = 1;
    assert(puzzle_surface_key(s, &key) == PUZZLE_OK);
    assert(puzzle_surface_text(s, bytes("世界")) == PUZZLE_OK);
    /* Clock epoch is arbitrary; timed inputs keep the same key/text semantics. */
    assert(puzzle_surface_key_timed(s, &key, 123.0, 123.002) == PUZZLE_OK);
    assert(puzzle_surface_text_timed(s, bytes("timed"), 123.0, 123.003) == PUZZLE_OK);
    assert(puzzle_surface_key_timed(s, &key, 124.0, 123.0) == PUZZLE_INVALID_ARGUMENT);
    assert(puzzle_surface_text_timed(s, bytes("refused"), -1.0, 123.0) == PUZZLE_INVALID_ARGUMENT);
    assert(puzzle_surface_preedit(s, bytes("é"), 1) == PUZZLE_INVALID_ARGUMENT);
    assert(puzzle_surface_preedit(s, bytes("é"), 2) == PUZZLE_OK);
    assert(puzzle_surface_preedit(s, bytes(""), SIZE_MAX) == PUZZLE_OK);
    assert(puzzle_surface_paste(s, bytes("paste")) == PUZZLE_OK);
    assert(puzzle_surface_clipboard_answer(s, bytes("clipboard")) == PUZZLE_OK);
    assert(puzzle_surface_ghost_text(s, bytes("suggestion")) == PUZZLE_OK);
    assert(puzzle_surface_ghost_accept(s) == PUZZLE_OK);
    puzzle_pointer_t pointer = {0};
    pointer.cell_width = 18; pointer.cell_height = 36;
    assert(puzzle_surface_pointer(s, &pointer) == PUZZLE_OK);
    pointer.kind = 1; pointer.held = 1; pointer.clicks = 1;
    assert(puzzle_surface_pointer(s, &pointer) == PUZZLE_OK);
    pointer.kind = 2; pointer.held = 0;
    assert(puzzle_surface_pointer(s, &pointer) == PUZZLE_OK);
    assert(puzzle_surface_scroll(s, -1, 0, &pointer) == PUZZLE_OK);
    assert(puzzle_surface_scroll(s, 0.5, 1, &pointer) == PUZZLE_OK);
    pointer.kind = 3;
    assert(puzzle_surface_pointer(s, &pointer) == PUZZLE_OK);
    assert(puzzle_surface_focus(s, 1) == PUZZLE_OK);
    assert(puzzle_surface_visibility(s, 0, 1) == PUZZLE_OK);
    assert(puzzle_surface_visibility(s, 1, 0) == PUZZLE_OK);
    assert(puzzle_surface_performance_hud(s, 1) == PUZZLE_OK);
    assert(puzzle_surface_performance_hud(s, 0) == PUZZLE_OK);
    assert(puzzle_surface_request_frame(s) == PUZZLE_OK);
}
static void annotations(state_t *state) {
    assert(puzzle_surface_marker_add(state->surface, 42, 0, 0) == PUZZLE_OK);
    time_t end = time(NULL) + 120;
    while (!state->marker) { assert(time(NULL) < end); pump(state, 1); }
    for (uint32_t kind = 0; kind <= 6; kind++) {
        puzzle_decoration_t d = {0};
        d.kind = kind; d.anchor_kind = 2; d.start = state->marker;
        d.slot = 5; d.color = 0x44aaeeff; d.size = 14; d.opacity = 1;
        d.transform[0] = 1; d.transform[3] = 1;
        d.text = bytes(kind == 5 ? "M0 0L1 0L1 1Z" : "annotation");
        d.tooltip = bytes("host tooltip");
        unsigned count = state->decorations;
        assert(puzzle_surface_decoration_add(state->surface, kind, &d) == PUZZLE_OK);
        wait_count(state, &state->decorations, count + 1);
        d.color = 0xeeaa44ff;
        assert(puzzle_surface_decoration_update(state->surface, state->decoration, &d) == PUZZLE_OK);
        assert(puzzle_surface_decoration_remove(state->surface, state->decoration) == PUZZLE_OK);
    }
    assert(puzzle_surface_marker_remove(state->surface, state->marker) == PUZZLE_OK);
    assert(puzzle_surface_marker_remove(state->surface, state->marker) == PUZZLE_INVALID_HANDLE);
}
#include "pointer_geometry.inc"
#include "search.inc"
#include "colors.inc"
#include "occlusion.inc"
#include "images.inc"
#include "surface_colour.inc"
#include "clipboard.inc"
#include "notifications.inc"
#include "dnd.inc"
#include "context.inc"
#include "optional_calls.inc"
#include "split_geometry.inc"
#include "tabs.inc"
#include "snapshot.inc"
#include "block_copy.inc"
#include "apps.inc"
/* A clipboard callback is a pending host request, not an inline success reply.
 * Normal output/query bytes flow before the host sends its completion message. */
static void clipboard_mime_calls(state_t *state) {
    state->input_len = 0;
    unsigned previous = state->mime_requests;
    size_t accepted = 0;
    puzzle_bytes_t output = bytes("\033]5522;type=write:id=c-host\033\\\033]5522;type=wdata:mime=aW1hZ2UvcG5n;iVBORw==\033\\\033]5522;type=wdata\033\\");
    assert(puzzle_surface_feed_output(state->surface, output, &accepted) == PUZZLE_OK);
    wait_count(state, &state->mime_requests, previous + 1);
    assert(state->input_len == 0);
    output = bytes("flowing\033[5n");
    assert(puzzle_surface_feed_output(state->surface, output, &accepted) == PUZZLE_OK);
    time_t end = time(NULL) + 120;
    while (state->input_len < 4) { assert(time(NULL) < end); pump(state, 1); }
    assert(state->input_len == 4 && memcmp(state->input, "\033[0n", 4) == 0);
    assert(puzzle_surface_clipboard_mime_answer(state->surface, state->mime_token, PUZZLE_CLIPBOARD_DONE, UINT64_MAX, NULL, 0) == PUZZLE_OK);
    const char *done = "\033[0n\033]5522;type=write:status=DONE:id=c-host\033\\";
    while (state->input_len < strlen(done)) { assert(time(NULL) < end); pump(state, 1); }
    assert(state->input_len == strlen(done) && memcmp(state->input, done, strlen(done)) == 0);
    /* Read results stay binary and cannot enter the ordinary Paste path. */
    assert(puzzle_surface_set_config(state->surface, bytes("{\"clipboard\":{\"osc52Read\":\"allow\"}}")) == PUZZLE_OK);
    output = bytes("\033]5522;type=read:id=r;dGV4dC9wbGFpbg==\033\\");
    assert(puzzle_surface_feed_output(state->surface, output, &accepted) == PUZZLE_OK);
    wait_count(state, &state->mime_requests, previous + 2);
    puzzle_mime_item_t item = {bytes("text/plain"), bytes("hello")};
    assert(puzzle_surface_clipboard_mime_answer(state->surface, state->mime_token, PUZZLE_CLIPBOARD_DONE, UINT64_MAX, &item, 1) == PUZZLE_OK);
    pump(state, 20);
    assert(puzzle_surface_clipboard_mime_answer(state->surface, 1, 99, UINT64_MAX, NULL, 0) == PUZZLE_INVALID_ARGUMENT);
    assert(puzzle_surface_clipboard_mime_answer(state->surface, 1, 0, UINT64_MAX, NULL, 1) == PUZZLE_INVALID_ARGUMENT);
    assert(puzzle_surface_clipboard_mime_answer(state->surface, 1, 0, UINT64_MAX, NULL, 65) == PUZZLE_INVALID_ARGUMENT);
    assert(puzzle_surface_set_config(state->surface, bytes("{\"clipboard\":{\"osc52Read\":\"deny\"}}")) == PUZZLE_OK);
}
int main(void) {
    if (getenv("PUZZLE_APPS_CHECK")) return apps_contract();
    if (getenv("PUZZLE_COPY_BLOCK_CHECK")) {
        block_copy_contract();
        /* Recreation registers the new actor after removing the previous member. */
        return block_copy_contract();
    }
    if (getenv("PUZZLE_SNAPSHOT_REPORT")) return snapshot_contract();
    if (getenv("PUZZLE_SURFACE_COLOUR_REPORT")) return surface_colour();
    state_t state = {0}; state.owner = pthread_self();
    puzzle_app_options_t options = {1, {NULL, 0}, {&state, wakeup, event}};
    options.config_path = bytes("/nonexistent-puzzle-ffi-config.json");
    assert(puzzle_app_new(NULL, &state.app) == PUZZLE_INVALID_ARGUMENT);
    assert(puzzle_app_new(&options, NULL) == PUZZLE_INVALID_ARGUMENT);
    assert(puzzle_app_new(&options, &state.app) == PUZZLE_OK);
    assert(puzzle_app_set_config(state.app, bytes("{\"cursor\":{\"blink\":false}}")) == PUZZLE_OK);
    void *window = puzzle_test_view_new();
    puzzle_surface_options_t surface = {0};
    surface.native_kind = 1; surface.native_view = puzzle_test_view(window); surface.transport = 1;
    surface.geometry = (puzzle_geometry_t){800, 480, 2, puzzle_test_display(window), 60000};
    puzzle_test_geometry(window, surface.geometry.width, surface.geometry.height, surface.geometry.scale);
    assert(puzzle_surface_new(state.app, &surface, &state.surface) == PUZZLE_OK);
    /* Worker notifications are queued; native callbacks run only inside tick. */
    assert(state.first_frames == 0 && state.cursor_rects == 0);
    assert(puzzle_surface_set_config(state.surface, bytes("{\"session\":{\"onExit\":\"hold\"},\"scrolling\":{\"smooth\":false}}")) == PUZZLE_OK);
    size_t accepted = 0;
    puzzle_bytes_t output = bytes("hello world\033]2;C host title\007\007\033[6n");
    assert(puzzle_surface_feed_output(state.surface, output, &accepted) == PUZZLE_OK && accepted == output.len);
    wait_count(&state, &state.titles, 1); wait_count(&state, &state.bells, 1);
    time_t reply_end = time(NULL) + 120;
    while (state.input_len < 7) { assert(time(NULL) < reply_end); pump(&state, 1); }
    assert(state.input_len == 7 && memcmp(state.input, "\033[1;12R", 7) == 0);
    wait_count(&state, &state.first_frames, 1);
    wait_count(&state, &state.cursor_rects, 1);
    snapshot_buffer_contract(state.surface, 800, 480);
    /* Taking over an already running native surface must accept the host's
     * independent index namespace, without making input depend on callbacks. */
    assert(puzzle_surface_refresh(state.surface, surface.geometry.display, 1, 0, 0) == PUZZLE_INVALID_ARGUMENT);
    state.host_clock = 1; state.display = surface.geometry.display;
    /* Cursor follows sealed CUP coordinates in logical points, including padding. */
    output = bytes("\033[2;3H");
    assert(puzzle_surface_feed_output(state.surface, output, &accepted) == PUZZLE_OK);
    unsigned cursor_before = state.cursor_rects;
    wait_count(&state, &state.cursor_rects, cursor_before + 1);
    assert(state.cursor[0] > 2 * state.cursor[2] && state.cursor[0] < 3 * state.cursor[2]);
    assert(state.cursor[1] > state.cursor[3] && state.cursor[1] < 2 * state.cursor[3]);
    input_calls(&state);
    const char *input_expected = "x世界xtimedpasteclipboardsuggestion";
    size_t input_end = 7 + strlen(input_expected);
    time_t input_deadline = time(NULL) + 120;
    while (state.input_len < input_end) { assert(time(NULL) < input_deadline); pump(&state, 1); }
    assert(state.input_len == input_end && memcmp(state.input + 7, input_expected, strlen(input_expected)) == 0);
    search_contract(&state);
    colors_contract(&state);
    occlusion_contract(&state);
    annotations(&state);
    pointer_geometry(&state);
    tabs_activity_contract(&state);
    clipboard_mime_calls(&state);
    clipboard_grant_calls(&state);
    notification_calls(&state);
    drop_calls(&state);
    context_calls(&state);
    image_settings(&state);
    /* A viewport which no longer contains the caret reports JSON null. */
    output = bytes("\r\n1\r\n2\r\n3\r\n4\r\n5\r\n6\r\n7\r\n8\r\n9\r\n10\r\n11\r\n12\r\n13\r\n14\r\n15\r\n16\r\n17\r\n18\r\n19\r\n20\r\n21\r\n22\r\n23\r\n24\r\n25");
    assert(puzzle_surface_feed_output(state.surface, output, &accepted) == PUZZLE_OK && accepted == output.len);
    pump(&state, 20);
    puzzle_pointer_t wheel = {0}; wheel.cell_width = 9; wheel.cell_height = 18;
    assert(puzzle_surface_scroll(state.surface, -100, 0, &wheel) == PUZZLE_OK);
    wait_count(&state, &state.offscreen_rects, 1);
    cursor_before = state.cursor_rects;
    assert(puzzle_surface_scroll(state.surface, 100, 0, &wheel) == PUZZLE_OK);
    wait_count(&state, &state.cursor_rects, cursor_before + 1);
    surface.geometry.width = 500;
    surface.geometry.height = 360;
    cursor_before = state.cursor_rects;
    puzzle_test_geometry(window, surface.geometry.width, surface.geometry.height, surface.geometry.scale);
    assert(puzzle_surface_resize(state.surface, surface.geometry) == PUZZLE_OK);
    wait_count(&state, &state.cursor_rects, cursor_before + 1);
    assert(state.cursor[1] + state.cursor[3] <= 180);
    /* Scale changes keep the callback in points, even as backing metrics change. */
    surface.geometry.scale = 1;
    cursor_before = state.cursor_rects;
    puzzle_test_geometry(window, surface.geometry.width, surface.geometry.height, surface.geometry.scale);
    assert(puzzle_surface_resize(state.surface, surface.geometry) == PUZZLE_OK);
    wait_count(&state, &state.cursor_rects, cursor_before + 1);
    assert(state.cursor[1] + state.cursor[3] <= 360);
    surface.geometry.scale = 2;
    puzzle_test_geometry(window, surface.geometry.width, surface.geometry.height, surface.geometry.scale);
    assert(puzzle_surface_resize(state.surface, surface.geometry) == PUZZLE_OK);
    pthread_t thread; assert(pthread_create(&thread, NULL, wrong_thread, &state) == 0);
    assert(pthread_join(thread, NULL) == 0);
    optional_calls(&state);
    assert(puzzle_surface_reply_complete(state.surface, 0) == PUZZLE_INVALID_ARGUMENT);
    assert(puzzle_surface_reply_complete(0, 1) == PUZZLE_INVALID_HANDLE);
    assert(puzzle_surface_exited(state.surface, 1, 0, bytes(""), 1000) == PUZZLE_OK);
    pump(&state, 20);
    puzzle_surface_t old = state.surface;
    state.host_clock = 0;
    assert(puzzle_surface_free(old) == PUZZLE_OK);
    assert(puzzle_surface_focus(old, 1) == PUZZLE_INVALID_HANDLE);
    assert(puzzle_surface_refresh(old, state.display, 1, 1, 1) == PUZZLE_INVALID_HANDLE);
    assert(puzzle_surface_free(old) == PUZZLE_INVALID_HANDLE);
    /* D-56: destroy the view before tick delivers the informational completion. */
    puzzle_test_view_free(window);
    wait_count(&state, &state.releases, 1);
    /* Default PTY ownership and generation reuse use the same public creation call. */
    window = puzzle_test_view_new();
    surface.native_kind = 5; surface.native_view = puzzle_test_layer(window); surface.transport = 0;
    surface.shell = bytes("/bin/sh");
    surface.geometry.display = puzzle_test_display(window);
    puzzle_test_geometry(window, surface.geometry.width, surface.geometry.height, surface.geometry.scale);
    assert(puzzle_surface_new(state.app, &surface, &state.surface) == PUZZLE_OK);
    assert(state.surface != old && puzzle_surface_focus(old, 1) == PUZZLE_INVALID_HANDLE);
    assert(puzzle_test_layer_is_top_left(surface.native_view));
    wait_count(&state, &state.first_frames, 2);
    assert(puzzle_test_layer_size(surface.native_view, surface.geometry.width, surface.geometry.height));
    unsigned title_count = state.titles;
    assert(puzzle_surface_text(state.surface, bytes("printf '\\033]2;C PTY flood\\007'; while :; do printf 'flood\\r\\n'; done\n")) == PUZZLE_OK);
    wait_count(&state, &state.titles, title_count + 1);
    /* Asynchronous resize must configure the borrowed layer, not replace it. */
    surface.geometry.width = 600; surface.geometry.height = 360;
    puzzle_test_geometry(window, surface.geometry.width, surface.geometry.height, surface.geometry.scale);
    assert(puzzle_surface_resize(state.surface, surface.geometry) == PUZZLE_OK);
    time_t resize_end = time(NULL) + 120;
    while (!puzzle_test_layer_size(surface.native_view, 600, 360)) {
        assert(time(NULL) < resize_end); pump(&state, 1);
    }
    assert(puzzle_test_layer_is_top_left(surface.native_view));
    assert(state.first_frames == 2);
    /* Transfer twice without changing either Puzzle or native layer identity. */
    for (unsigned move = 0; move < 2; move++) {
        void *destination = puzzle_test_transfer(window, surface.native_view);
        puzzle_test_view_free(window); window = destination;
        surface.geometry.display = puzzle_test_display(window);
        surface.geometry.width += 20;
        puzzle_test_geometry(window, surface.geometry.width, surface.geometry.height, surface.geometry.scale);
    assert(puzzle_surface_resize(state.surface, surface.geometry) == PUZZLE_OK);
        time_t transfer_end = time(NULL) + 120;
        while (!puzzle_test_layer_size(surface.native_view, surface.geometry.width, 360)) {
            assert(time(NULL) < transfer_end); pump(&state, 1);
        }
        assert(puzzle_test_layer_is_top_left(surface.native_view));
        assert(state.first_frames == 2);
    }
    assert(puzzle_surface_free(state.surface) == PUZZLE_OK);
    puzzle_test_view_free(window);
    pump(&state, 2);
    /* MIME/grant calls also cross the borrowed-layer host transport path. */
    window = puzzle_test_view_new();
    surface.native_view = puzzle_test_layer(window); surface.transport = 1;
    puzzle_test_geometry(window, surface.geometry.width, surface.geometry.height, surface.geometry.scale);
    assert(puzzle_surface_new(state.app, &surface, &state.surface) == PUZZLE_OK);
    tabs_activity_contract(&state);
    clipboard_mime_calls(&state);
    clipboard_grant_calls(&state);
    notification_calls(&state);
    drop_calls(&state);
    context_calls(&state);
    wait_count(&state, &state.first_frames, 3);
    assert(puzzle_surface_free(state.surface) == PUZZLE_OK);
    puzzle_test_view_free(window);
    pump(&state, 2);
    /* A separate fresh surface closes before any app_tick delivers its facts. */
    window = puzzle_test_view_new(); surface.native_view = puzzle_test_layer(window);
    puzzle_test_geometry(window, surface.geometry.width, surface.geometry.height, surface.geometry.scale);
    assert(puzzle_surface_new(state.app, &surface, &state.surface) == PUZZLE_OK);
    assert(puzzle_surface_free(state.surface) == PUZZLE_OK);
    puzzle_test_view_free(window);
    pump(&state, 2);
    assert(state.first_frames == 3);
    /* A callback can close its surface: later facts from the same tick are suppressed. */
    window = puzzle_test_view_new(); surface.native_view = puzzle_test_layer(window);
    puzzle_test_geometry(window, surface.geometry.width, surface.geometry.height, surface.geometry.scale);
    assert(puzzle_surface_new(state.app, &surface, &state.surface) == PUZZLE_OK);
    state.close_on_first = 1;
    wait_count(&state, &state.first_frames, 4);
    assert(state.closed_in_callback == state.surface);
    assert(puzzle_surface_focus(state.surface, 1) == PUZZLE_INVALID_HANDLE);
    pump(&state, 2);
    assert(state.callbacks_after_close == 0);
    puzzle_test_view_free(window);
    optional_launch_calls(&state);
    block_copy_hosts(&state);
    assert(puzzle_app_free(state.app) == PUZZLE_OK);
    assert(puzzle_app_tick(state.app) == PUZZLE_INVALID_HANDLE);
    assert(atomic_load(&state.wakes) > 0);
    geometry_early_reload();
    split_geometry_contract();
    apps_contract();
    puts("M17_C_HOST_PASS every exported operation, callback affinity, native release");
    return 0;
}
