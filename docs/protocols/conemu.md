# ConEmu progress

Reference: [ConEmu OSC extensions](https://conemu.github.io/en/AnsiEscapeCodes.html#ConEmu_specific_OSC).

OSC 9;4;state;percent emits progress. State is clamped to 0–4, percentage to
0–100; invalid/missing numbers use zero. Other ConEmu subcommands are not
implemented. Shared Session/overlay state draws the result.
Code: `puzzle-vt/src/protocols/conemu.rs`.
