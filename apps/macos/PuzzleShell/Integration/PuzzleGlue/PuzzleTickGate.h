/* Worker-safe tick accounting and one-pending-block gate, using the system C11 atomics. */
#ifndef PUZZLE_TICK_GATE_H
#define PUZZLE_TICK_GATE_H
#include <stdint.h>
/* Opt-in probe counters are disabled during ordinary app use. */
void puzzle_tick_measure(int enabled);
/* Count a worker callback without allocating or entering AppKit. */
void puzzle_tick_wakeup(void);
/* Count a block actually submitted to the main queue. */
void puzzle_tick_enqueued(void);
/* Atomically claim the only pending tick; zero means a queued block already owns it. */
int puzzle_tick_claim(void);
/* Clear before entering tick, so callbacks produced during tick schedule its successor. */
void puzzle_tick_clear(void);
/* Snapshot cumulative callbacks/blocks; the main-thread probe computes interval deltas. */
uint64_t puzzle_tick_wakeups(void);
uint64_t puzzle_tick_blocks(void);
#endif
