# Protocol support by origin

D-58 organizes sequences by their defining project. **Puzzle** records our
implementation of each row. Grouped rows name their supported subset in the family
page. Common-base handlers remain under `csi`/`esc`/`osc`/`dcs`; shared models and
host policy stay outside origin modules.

| Sequences / family | Origin | Puzzle | Behaviour and limits |
|---|---|---|---|
| C0, cursor/editing, base SGR and ANSI modes | [ECMA-48](ecma48.md) | supported common subset | Grid bounded; REP capped; see family table |
| ESC/DEC charset, cursor/margins/protection, DEC modes, DA/DSR, DECRQSS | [DEC](dec.md) | supported common subset | No DEC double-size lines; unknown mode query 0 |
| OSC 0–2, 22, 52; extended SGR/input; size queries, XTVERSION/XTGETTCAP | [xterm](xterm.md) | supported subset | BEL/ST matched; title read disabled; clipboard reads denied |
| CSI 22/23 t title stack | [xterm](xterm.md) | supported | Stack depth 10 |
| OSC 4/104, 10–12/110–112 | [xterm](xterm.md) | supported | Shared defaults/overrides; reset restores theme; replies preserve terminator |
| OSC 5/105, 13–19/113–119; OSC 4 special aliases 256–260 | [xterm](xterm.md) | supported | Stores/queries all, draws selection 17/19 |
| CSI > Ps s XTSHIFTESCAPE | [xterm](xterm.md) | supported | Shared false/true/always/never policy; legacy settings retained; RIS resets preference |
| CSI u keyboard | [kitty](kitty.md) | supported | All five flags; per-screen stack 16 |
| APC G images and U=1 Unicode placeholders | [kitty](kitty.md) | supported desktop subset | Image allowance and unsupported media replies documented in family page |
| OSC 21 colours | [kitty](kitty.md) | supported subset | Palette, fg, bg, cursor, cursor_text, selection and stored visual_bell; unknown keys safely reported |
| OSC 5522 clipboard | [kitty](kitty.md) | supported native MIME seam | Strict 64-MiB streaming; asynchronous completion; reads/listings denied by default (§16); macOS text/PNG host writes user-confirmed 2026-10-12 |
| Mode 5522 paste events | [Rockorager](rockorager.md) | supported native seam | One user-gesture read, scoped to session/location/content revision; no timer |
| OSC 99 notifications | [kitty](kitty.md) | supported bounded subset | Title/body chunks, base64, urgency/occasion, activation/close, query; harness toasts; macOS Notification Center user-confirmed 2026-10-12 |
| OSC 72 drag and drop | [kitty](kitty.md) | supported local native seam | Full enter/move/accept/drop/leave C flow; 16 representations / 64 MiB; harness OS file-only limitation below |
| OSC 66 text sizing | [kitty](kitty.md) | full | [Sparse rectangles](text-sizing.md), atomic editing/reflow, selection/search and scaled glyph/cursor geometry; user app/harness and reference review approved 2026-10-13 |
| OSC 133 / VS Code 633 A–D alias | [FinalTerm](finalterm.md) | supported boundary subset | 10,000 tracked zone marks |
| OSC 9 notifications | [iTerm2](iterm2.md) | supported | 1,024-scalar body; shared rate limit |
| OSC 9;4 progress | [ConEmu](conemu.md) | supported | State 0–4, percent 0–100 |
| OSC 777 notify; mode 1015 mouse | [rxvt](rxvt.md) | supported | 256/1,024-scalar notification title/body |
| Mode 2031, CSI ?996 / ?997; modes 2027 and 2026 | [Contour](contour.md) | supported | 32-byte clusters; bounded synchronized output |
| Mode 2048 / CSI 48 report | [Rockorager](rockorager.md) | supported | Current grid/cell pixels |
| OSC 7 | [Apple Terminal](apple.md) | supported file-URI subset | 2,048 scalars; no filesystem access |
| OSC 8 | [VTE](vte.md) | supported | 2-KiB URI, 4,096 entries, safe host policy |
| OSC 7501 | [Rex](rex.md) | optional handler | Adopted draft 0.3, 4,096 bytes / 256 records |
| OSC 3008 | [UAPI](uapi.md) | supported active context path | 64 contexts, 64-byte IDs, 255-character fields; RIS preserves, overflow rejects new; contents and tick event |
**Deliberate theme difference (§14.7):** RIS and theme/appearance changes clear
all Puzzle OSC overrides, including palette, special and dynamic colours (OSC
17/19 included). Programs reapply after
mode 2031. This is the user's decision of 2026-10-11, not missing coverage.

Protocol parsers do no I/O. Optional protocols require runtime registration.
Unsupported sequences are consumed safely; protocol errors are used where
specified, and unsupported features are excluded from capability replies.
Evidence and exact test/golden receipts: [M18](../evidence/M18.md).

**Harness drag limitation (reviewer/user, 2026-10-12):** winit supplies file
enter/drop/leave only, without drag positions or OS acceptance feedback. The
harness offers file URIs (`text/uri-list`) and plain paths (`text/plain`), at the
latest pointer position, and delivers a registered drop only after program copy
acceptance. Unregistered drops retain shell-quoted path paste. This is a harness
limitation, not a product limitation: the C API exposes full position/MIME
negotiation for native hosts (NSDraggingDestination in the macOS app). Synthetic
scenario commands and the C fixture cover the full flow; no puzzle-sys bridge.

## Automatic shell integration (M18.8)

| Startup route | Puzzle | Limits |
|---|---|---|
| GNU bash POSIX ENV | supported | Restore login profiles, options, ENV and history; no startup-file edits |
| Apple bash 3.2 | manual sourcing, true login retained | Version-detected ENV exception|
| Zsh ZDOTDIR | supported | Restore original directory, user startup and framework hooks |
| Fish XDG_DATA_DIRS vendor config | supported | Preserve vendor/user configuration; no duplicate native marks |
Puzzle's wrappers are original code, using the existing snippets; no GPL shell
code from other terminals is copied. `shellIntegration`
is a native next-session setting, default `detect`; forced choices and `none`
are documented in [shell integration](../shell-integration.md). Fresh-zsh startup without manual sourcing or duplicate marks was user-approved
in the app and harness on 2026-10-13; other supported-shell mechanics retain
their automated coverage and Apple bash keeps its documented manual exception.

