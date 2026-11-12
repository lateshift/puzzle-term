# Terminal parser corpora

`cargo xtask corpus` regenerates five deterministic byte streams and measures native `Terminal::advance` throughput. Each source has complete records and is at least 64 KiB; the test examples repeat whole sources to about 8 MiB per timed sample. Timers exclude generation, terminal construction, warm-up and result-file writes.

| File | Workload |
|---|---|
| `plain-ascii.vt` | 78 printable columns followed by CR/LF, repeatedly scrolling an 80×24 grid |
| `sgr-heavy.vt` | Indexed and RGB SGR changes mixed with printable status text |
| `cursor-heavy.vt` | A 24-row dashboard repaint using CUP, EL, indexed color and short process fields |
| `unicode-mix.vt` | Narrow, wide and combining text for a diagnostic rate without an M5 budget |
| `flood.vt` | Four changing 80×40 truecolor foreground/background `▀` frames with cursor-home repaint; the headless flood probe repeats this source continuously |

The generator is `xtask/src/corpus/generate.rs`; the timed native examples are in `crates/puzzle-vt/examples/`. The fixture records are synthetic and contain no machine-specific data.
