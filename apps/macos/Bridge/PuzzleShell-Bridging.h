/* Keep the optional host observer ABI in both modes; enabled builds import Puzzle's real ABI. */
#ifndef PUZZLE_SHELL_BRIDGING_H
#define PUZZLE_SHELL_BRIDGING_H
#include "PuzzleShell.h"
#include "../PuzzleShell/Integration/PuzzleGlue/PuzzleTickGate.h"

/* PUZZLE_ENABLED=YES in Xcode sets this C macro to 1 via Puzzle.xcconfig.
 * Do not copy declarations here: the committed puzzle.h remains authoritative (D-56). */
#if PUZZLE_ENABLED
#include <puzzle.h>
#endif
#endif
