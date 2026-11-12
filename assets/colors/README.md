# X11 named colours (M18.1)

User-approved asset, 2026-10-11 (R4). No new crate or runtime initialization.

- Source: X.Org `app/rgb` [rgb.txt](https://gitlab.freedesktop.org/xorg/app/rgb/-/blob/master/rgb.txt), preserved verbatim (782 entries, 17,780 bytes).
- License: MIT/X11; upstream [COPYING](https://gitlab.freedesktop.org/xorg/app/rgb/-/blob/master/COPYING) is retained here. Copyright The Open Group 1985, 1989, 1998 and Oracle and/or affiliates 1994, 2008.
- `rgb.txt` SHA-256: `f8e3a7bea17acc0b91e6285c5d32001db27d84c4461c655afdbce9dbeb4fb6f0`.
- `COPYING` SHA-256: `eb3b0a2c9810a0a364cd813377713a223651c17ae8be97c62c2e5d680f71f843`.
- Generated blob SHA-256: `b77319940ba9e8f755a3677cd8fd2f62a0e46d6df58b7e5a5d2f77273119f6af`.

Regenerate with `python3 tools/generate-x11-colours.py`; verify without writing
with `python3 tools/generate-x11-colours.py --check`. The generator validates
all 782 rows, removes ASCII spaces and folds ASCII case, checks aliases have
equal RGB, then sorts the unique keys. The packed blob has a little-endian
u16 count, count+1 u16 record offsets, and records of three RGB bytes followed
by a normalized ASCII name. A binary search compares the query iterator
without allocating or normalizing it into a temporary string. No table is
built on first use. The parser is shared by every xterm/kitty colour sequence.
