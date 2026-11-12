#include "../Bridge/PuzzleShell-Bridging.h"
/* Test-only helpers check the public ABI from a separately compiled C translation unit. */
uint64_t bridge_probe_create_window(void);
int32_t bridge_probe_reentrant_commands(uint64_t window, uint64_t tab);
