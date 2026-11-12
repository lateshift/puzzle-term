# M5.5b static terminal preview fixtures

These `.vt` files are PTY byte recordings made with macOS `script -q` on
2026-09-29. They exercise the same `Terminal::advance` byte entry point as a
future live transport. The preview is harness code only and is replaced by
M6's frame builder.

| Recording | Captured process | Visible purpose |
|-----------|------------------|-----------------|
| `ls.vt` | `TERM=xterm-256color script -q … ls -G` at the repository root | column layout and names from this repository only |
| `git-log.vt` | `TERM=xterm-256color script -q … git log --graph --color=always --oneline -20` | graph characters and SGR color |
| `chart.vt` | `TERM=xterm-256color script -q … target/debug/xtask preview-chart` | all indexed colors and a truecolor row |
| `box.vt` | `TERM=xterm-256color script -q … target/debug/xtask preview-box` | box drawing, blocks, Braille and Powerline |
| `vim.vt` | `TERM=xterm-256color script -q … vim -u NONE -c 'redraw!' -c 'sleep 1' -c 'q!' assets/fixtures/preview/sample.txt` | an application entering and leaving the alternate screen |

`vim-open.vt` is `vim.vt` up to its final `CSI ? 1049 l`; `vim-close.vt`
contains that leave sequence. The split lets the scenario screenshot the
alternate screen before the primary screen returns. The `script` recording
prefix `^D BS BS` was removed from each capture because it describes the
recorder's own terminal, not process output.

Before committing, each recording was inspected as printable text and
scanned for the local home path, username, hostname, Git author name and
`@localhost`. No personal prompt or host path is present. The chart and
box generators use the neutral prompt `puzzle$`; Vim opens only `sample.txt`.
