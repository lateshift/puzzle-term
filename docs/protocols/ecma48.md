# ECMA-48 common base

Reference: [ECMA-48, fifth edition](https://ecma-international.org/publications-and-standards/standards/ecma-48/).

Common handlers stay in `puzzle-vt/src/perform/{c0,csi,esc,sgr}.rs`. Origin
modules are reserved for extensions (D-58); grid, pen and cursor remain shared.

| Functions | Puzzle behaviour / bounds |
|---|---|
| ENQ, BEL, BS, HT, LF/VT/FF, CR, SO/SI | ENQ empty; bell event; cursor/tab/charset operations; other C0 ignored |
| CUU/CUD/CUF/CUB, CNL/CPL, CHA/HPA/HPR, VPA/VPR, CUP/HVP, CHT/CBT | Margin-aware movement, bounded by grid |
| ED/EL/ECH, ICH/DCH, IL/DL, SU/SD, REP, TBC | BCE erasure; REP capped to screen size; region scroll moves handles |
| IRM, LNM | Shared mode table and input/printing mirrors |
| SGR 0–9, 21–25, 27–29, 30–49, 53/55, 58/59, 90–107; colon underline styles and indexed/truecolour subparameters | 16-byte cells; rare underline colour in sparse extras; unsupported fields consumed |

All input uses bounded VTE parameters. Detailed semantics and conformance are
in design §6.5 and `crates/puzzle-vt/tests/conformance/`.
