# Rockorager in-band resize

Reference: [mode-2048 proposal](https://gist.github.com/rockorager/e695fb2924d36b2bcf1fff4a3704bd83).

Mode 2048 enables `CSI 48;rows;cols;height_px;width_px t` after a grid resize.
Dimensions use current cell-pixel geometry and saturating arithmetic. Generic
DECSET/DECRQM and grid resizing stay shared. This task preserves the existing
report timing (no new enable-time report).
Code: `puzzle-vt/src/protocols/rockorager.rs`.

## Paste events, mode 5522

Reference: [bracketed paste MIME](https://rockorager.dev/misc/bracketed-paste-mime/).
DECSET/DECRST/DECRQM 5522 enables/queries unsolicited OSC 5522 MIME listings
on a user paste gesture. It takes precedence over bracketed text paste; mode
reset revokes the gesture capability even if re-enabled in the same VT slice.
Kitty defines the clipboard wire exchange and password metadata; §16 defines
Puzzle's single-use security policy. Both halves run on the Session owner.
