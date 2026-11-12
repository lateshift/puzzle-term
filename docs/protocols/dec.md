# DEC VT common base

Reference: [VT510 programmer reference](https://vt100.net/docs/vt510-rm/contents.html).

Common handlers remain under `perform/{esc,csi,dcs}.rs`; no DEC origin wrapper
is added around the common base.

| Functions | Puzzle behaviour / bounds |
|---|---|
| DECSC/DECRC, IND/NEL/RI, HTS, RIS, DECKPAM/DECKPNM, SCS, SS2/SS3, LS2/LS3, DECALN | Cursor/charset state; RIS resets session; ASCII/UK/DEC Special Graphics |
| DECSTBM, DECSLRM, DECSED/DECSEL/DECSCA, DECSCUSR, DECSTR | Margin/protection/cursor controls; DECSCUSR 0 restores configured cursor (deliberate choice) |
| DECSET/DECRST/DECRQM | Modes ?1,3,5,6,7,9,12,25,40,45,47,66,69,1000,1002–1007,1015,1016,1036,1039,1047–1049,2004 plus origin extensions indexed below |
| DECRQSS `DCS $q` | SGR, margins, cursor style/protection; unknown reports failure |
| DA1/DA2/DA3, DSR/CPR/DECXCPR | Fixed nonidentifying DA1/DA3; package version in DA2; origin-aware CPR |

Double-size DEC lines and other undocumented functions remain unsupported.
