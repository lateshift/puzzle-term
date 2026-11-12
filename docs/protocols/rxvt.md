# rxvt-unicode extensions

Reference: [urxvt Perl OSC hook](https://cvs.schmorp.de/rxvt-unicode/doc/rxvtperl.3.man).

OSC 777;notify;title;body uses rxvt-unicode's extension-hook namespace.
The notify plugin convention is attributed to that family.
Title ≤256 and body ≤1,024 printable Unicode scalars; non-notify commands
ignored. Rate limits and host policy remain shared with OSC 9.

Mode 1015 uses rxvt's decimal mouse encoding; common tracking and modifier
policy runs before its origin-specific formatter.
Code: `puzzle-vt/src/protocols/rxvt.rs`.
