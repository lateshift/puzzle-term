# UAPI context signalling

Origin: UAPI Group. Reference: [UAPI.15 OSC 3008, version 1.0](https://uapi-group.org/specifications/specs/osc_context/).
Implementation: `puzzle-vt/src/protocols/uapi.rs`; shared state in `context.rs`
and `terminal/contexts.rs`. This auxiliary information is untrusted; it never
authorizes host actions or proves a user's privileges.

Puzzle retains an active root-to-leaf context path. `start=<id>` pushes a new
child; repeating an ID replaces its metadata and removes its descendants.
`end=<id>` removes that entry and its descendants; unknown IDs do nothing.
All fifteen start metadata fields are supported. Unknown or malformed fields
are ignored individually. End metadata is not retained: this API exposes the
current path rather than a historical command database.

Bounds: 64 entries, decoded printable-ASCII IDs of 1–64 bytes, metadata text
up to 255 Unicode scalars, raw OSC below 64 KiB. Text is UTF-8; the protocol's
semicolon/backslash escapes are decoded once. Numeric identities are decimal
strings at the host boundary, preserving all 64 bits. Canonical field order
keeps event recordings deterministic. A full stack rejects new IDs while
allowing updates and ends. RIS, DECSTR and screen switches preserve the path;
Session exit clears it even when the final screen is held.

`Session::buffer().contexts()` borrows the current path without copying.
The public bus fact `terminal/contextsChanged` carries
`{"contexts":[{"id":"...","fields":{"type":"remote","targethost":"example"}}]}`.
It is coalesced to the final state per parser drain and copied only for an
observer. Native hosts receive this fact through `puzzle_app_tick`'s existing
JSON callback. No new C function is needed, and no core UI is drawn (M12).

Tests: `cargo xtask accept M18 --task context`.
