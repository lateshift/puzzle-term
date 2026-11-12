# Contour appearance and Unicode modes

Reference: [appearance proposal](https://github.com/contour-terminal/contour/blob/master/docs/vt-extensions/color-palette-update-notifications.md), [Unicode core](https://github.com/contour-terminal/terminal-unicode-core), [synchronized output](https://github.com/contour-terminal/contour/blob/master/docs/vt-extensions/synchronized-output.md).

| Sequences | Puzzle |
|---|---|
| CSI ?996 n; mode 2031; CSI ?997;1/2 n replies | Resolved dark/light theme; notify on changed theme only |
| Mode 2027 | Grapheme clustering enabled by default (D-31), configurable legacy; 32-byte cluster limit |
| Mode 2026 | Contour's DEC-private-mode form of iTerm2 synchronized output; bounded timeout and sealed publications |

Origin-specific appearance bytes live in `puzzle-vt/src/protocols/contour.rs`.
The generic DECSET/DECRQM mode catalogue, UAX #29 state machine, widths and
Session scheduling remain shared models; origin organization does not create
a second source of mode or rendering state.
