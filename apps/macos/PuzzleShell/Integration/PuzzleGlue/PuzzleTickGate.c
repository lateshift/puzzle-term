/* No dependency or platform runtime: a C11 atomic flag works back to macOS 13. */
#include "PuzzleTickGate.h"
#include <stdatomic.h>
#include <stdbool.h>
/* One shared gate belongs to this app process, not to any individual pane. */
static atomic_bool pending = false;
/* Measurement switches and counters are relaxed: they publish no application data. */
static atomic_bool measuring = false;
/* Cumulative counters avoid racing a reset against an in-flight worker notification. */
static _Atomic(uint64_t) wakeups = 0;
static _Atomic(uint64_t) blocks = 0;
/* Only explicit probe launches enable counter traffic on the callback hot path. */
void puzzle_tick_measure(int enabled) { atomic_store_explicit(&measuring, enabled != 0, memory_order_relaxed); }
/* Worker callbacks never allocate, acquire a native lock, or wait for the main queue. */
void puzzle_tick_wakeup(void) {
    if (atomic_load_explicit(&measuring, memory_order_relaxed))
        atomic_fetch_add_explicit(&wakeups, 1, memory_order_relaxed);
}
/* This counts enqueue decisions, not attempts suppressed by coalescing. */
void puzzle_tick_enqueued(void) {
    if (atomic_load_explicit(&measuring, memory_order_relaxed))
        atomic_fetch_add_explicit(&blocks, 1, memory_order_relaxed);
}
/* The exchange admits exactly one producer until the queued consumer clears it. */
int puzzle_tick_claim(void) { return !atomic_exchange_explicit(&pending, true, memory_order_acq_rel); }
/* Clear at block entry; clearing after tick could lose a wake arriving during delivery. */
void puzzle_tick_clear(void) { atomic_store_explicit(&pending, false, memory_order_release); }
/* These counters are diagnostic snapshots, never scheduler synchronization. */
uint64_t puzzle_tick_wakeups(void) { return atomic_load_explicit(&wakeups, memory_order_relaxed); }
/* Both accessors use the same cumulative, monotonic interpretation. */
uint64_t puzzle_tick_blocks(void) { return atomic_load_explicit(&blocks, memory_order_relaxed); }
