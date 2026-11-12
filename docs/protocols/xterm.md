# xterm common extensions

Reference: [xterm control sequences](https://invisible-island.net/xterm/ctlseqs/ctlseqs.html).

Common extension dispatch remains in `perform/csi`, `perform/osc` and
`perform/dcs`, per D-58. Shared colour/clipboard policy is independent of origin.

| Sequences | Current Puzzle behaviour / bounds |
|---|---|
| OSC 0/1/2; CSI 22/23 t | Titles ≤1,024 printable scalars, stack 10 |
| OSC 4/104 | 256 palette set/query/reset; special aliases 256–260; malformed pairs skipped |
| OSC 10/11/12, 110/111/112 | Foreground/background/cursor override and reset; BEL/ST query terminator echoed |
| OSC 5/105, 13–19/113–119 | Set/query/reset for all roles; selection 17/19 drawn; pointer/Tektronix stored only |
| OSC 22 | Finite CSS pointer-name allowlist, no stack/query; xterm defined the original sequence |
| OSC 52 | Strict bounded Base64 UTF-8 write event; read denied by default (§16) |
| CSI 14/16/18 t | Text-area/cell dimensions; window movement/resizing ignored, title disclosure disabled |
| XTVERSION, XTGETTCAP | Own Puzzle identity and implemented terminfo capabilities only; unknown capabilities fail |
| CSI > Ps s XTSHIFTESCAPE | Omitted/0 releases Shift; 1 captures; malformed ignored; RIS clears the request, DECSTR preserves it |
| CSI >4;2m | modifyOtherKeys is optional and not advertised/implemented |
| Mouse/focus/paste modes, alternate screen, reverse wrap, 256/truecolour/underline SGR | xterm-compatible subset in §6.5–6.8; urxvt decimal mouse dialect has its own origin module |

**Deliberate §14.7 difference:** RIS and theme/appearance changes clear every
OSC override (palette, special and dynamic, including selection). Programs may
reapply overrides after mode 2031 notification. This decision is unchanged.

All colour numbers use one shared parser: `#` (1–4 hex digits per channel,
most significant bits), `rgb:` (independent 1–4 digit scaled channels), finite
`rgbi:` intensities and the complete approved X11 names (ASCII case-insensitive,
spaces ignored). Defaults come from the theme; each entry has one optional
override. Query replies preserve BEL/ST and use 16-bit replicated RGB.
Special roles are stored/reported: our themes have no separate special-colour
style tokens. Successful colour changes invalidate paint, never reshape text.

`mouse.shiftCapture` is `false` by default. Boolean `false`/`true` chooses a
program-overridable default; `"always"`/`"never"` forces capture/selection.
An explicit XTSHIFTESCAPE 0/1 wins over a Boolean default. Without tracking,
mouse gestures select locally in every setting. Legacy `mouse.shiftSelects`
and `mouse_shift_selects` Boolean keys map true to `"never"`, false to
`"always"`, preserving their forced behavior; exported layers use the new key.
Shift-wheel remains local by the deliberate §7.4 policy.
