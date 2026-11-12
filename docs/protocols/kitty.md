# kitty protocols

Reference: [keyboard](https://sw.kovidgoyal.net/kitty/keyboard-protocol/) and [graphics](https://sw.kovidgoyal.net/kitty/graphics-protocol/).

| Sequences | Current Puzzle behaviour / bounds |
|---|---|
| CSI >/< /=/? u | All five progressive keyboard flags; per-screen 16-entry stacks; functional, keypad and named-key tables |
| APC G | Direct, chunked raw RGB/RGBA/PNG; native zlib; query, status, placement, deletion |
| APC G U=1 and U+10EEEE | Virtual placements; exact published diacritics; raw foreground/underline IDs, inheritance; continuous clipped image transform |
| OSC 21 | Shared palette/fg/bg/cursor/cursor_text/selection/visual_bell set/query/reset |
| OSC 5522 | Bounded MIME reads/writes, streaming, aliases, asynchronous host completion; paste grants via mode 5522 |
| OSC 99 | Bounded chunks/base64, identifiers, title/body, urgency/occasion, activation/close, query |
| OSC 72 | Local registration, positioned MIME offers, acceptance, data reads and conclusion; 16 alternatives / 64 MiB |
| OSC 66 | Full rectangular grid text: bounded sparse blocks, editing/reflow, contents, selection/search, scaled rendering and cursor extents |

Graphics metadata ≤4,096 bytes, Base64 chunk ≤65,536 bytes. Decoded and GPU
allowances default to 320 MiB each, charged at admission by the larger exact
footprint, including tile borders. Only visible placements upload; image-free
frames do no image/placeholder work. Unsupported media (file/temp/shared
memory), animation and relative placement get protocol errors; no silent
capability advertisement. See `docs/image-review.md` for the accepted desktop scope.

Code: `puzzle-vt/src/protocols/kitty` for keyboard and framing;
`puzzle-term/src/protocols/kitty/graphics` for interpretation. Image storage,
admission, anchors, decoded pixels, renderer batching and policy remain shared.

OSC 21 operates on the same table as xterm colours, in request order. Bare
keys reset; empty nonpalette values request dynamic colour; `?` queries.
Replies preserve BEL/ST; unknown keys return `unknown=<unpadded Base64 key>`,
so hostile names cannot inject controls. Transparent-background slots are not
implemented and receive this unsupported-key reply. Pointer/Tektronix and
special colours remain reachable via xterm, not invented kitty key names.
`visual_bell` is stored/reported; the built-in bell uses its existing UI token.

OSC 5522 reference: [kitty clipboard](https://sw.kovidgoyal.net/kitty/clipboard/).
64 MiB decoded per transaction, 64 MIME alternatives, strict RFC 4648 including
padding, 512-byte sanitized IDs. Read data is independently encoded in chunks
of at most 4,096 bytes. A host acknowledgement arrives asynchronously; one
outstanding request and a ten-second host hang guard bound pending work.
Unsupported harness MIME combinations return ENOSYS, never partial success.

Reads and type listings are denied by default (§16, a deliberate difference
from kitty's prompt-free listing). User paste in mode 5522 issues a single-use
session/location/content-generation grant, with no timer. Input/reset/next
paste/clipboard change revoke it. Passwords without a name grant nothing;
persistent approval prompts are unavailable until M12. No native UI is invoked
by the parser or Session thread. See the host contract in puzzle.h.

OSC 99 reference: [kitty desktop notifications](https://sw.kovidgoyal.net/kitty/desktop-notifications/).
Plain chunks are at most 2,048 bytes, encoded chunks 4,096 bytes. Title/body
have independent base64 tails and may be interleaved by ID. Each field is
capped at 1,024 scalars and strips controls; UTF-8 is validated on completion.
A maximum of 64 incomplete and 64 admitted notifications bounds abandoned
transactions and hosts that never complete. The shared §16 admission limit is
one notification per second across OSC 9/777/99 and API requests. Close requests
are not rate limited. Replacement invalidates the old generation without a close
report; anonymous notifications never replace one another. RIS clears both tables
and makes old callbacks inert, retaining the rate-limit clock.

Occasions use the latest host state: `unfocused` requires no keyboard focus;
`invisible` additionally requires not visible. `a=focus,report` and their disabling
forms, `c=1`, `p=close` and `p=?` are supported. Activation/close replies go only
to the creating Session and join its normal writer order when feedback arrives.
Untracked closure reports `untracked`. Icons, buttons, sounds, auto-expiry and
alive polling are not advertised. Native hosts consume structured callbacks;
the harness uses theme-token overlay toasts with body activation and × dismissal.
The web retains its event delivery; native Notification Center posting/activation/dismissal was user-confirmed on
2026-10-12 (apps/macos/README.md).

OSC 72 reference: [kitty drag and drop](https://sw.kovidgoyal.net/kitty/dnd-protocol/).
`t=q` replies before a following DA1, with an empty optional-capability payload.
`t=a` registers a target and `t=A`, RIS or child exit unregisters it. Each
registration has a new native generation; stale host messages are inert.
Registration/status MIME lists are bounded to 1 MiB across 4096-byte packets.
First-chunk metadata persists, and queries may interleave without consuming it.

A host supplies enter/move positions in view-local logical pixels, including
padding. Puzzle uses presented geometry, DPR and fractional scrolling to encode
zero-based cell and device-pixel coordinates. `t=m` program feedback returns
copy/move/reject and ordered MIME preferences to the host. Hover has names only;
data reads before an actual Drop get EPERM and do not terminate the hover.
Only actual user drops supply up to 16 ordered native representations, at most
64 MiB total. `t=M` repeats the full MIME list. One-based `t=r:x=N` reads use
4096-byte Base64 packets and a separate empty EOF; a conclusion releases data.
Leave, error, replacement and reset also release captured data. Replies share
the normal terminal FIFO and preserve the request terminator.

Machine identifiers are accepted
without remote transfer, file/URI contents are never opened, and directory/edit
and drag-out operations are unavailable. Remote read forms return EINVAL;
drag-out requests return EPERM. Unsupported directions are absent from query
capabilities. The protocol-mandated pre-drop EPERM deliberately differs from
a plain ENOENT. The full native host seam supports position/MIME
negotiation; only winit's real-file adapter has the documented subset in the
[support index](support.md). Synthetic commands exercise richer MIME flows.

OSC 66 reference: [kitty text sizing](https://sw.kovidgoyal.net/kitty/text-sizing-protocol/).
The six numeric keys and 4096-byte UTF-8 payload bound feed sparse, screen-owned
rectangles. Cell storage remains 16 bytes. Ordinary text/scrolling/frame assembly
has no sparse lookup, extra scan or warm allocation. Explicit-width runs retain
one logical payload; editing and reflow preserve or remove whole rectangles.
Selection/copy/search and cursor/IME extents resolve the same sealed geometry.
No new font setting, shader math, terminfo flag or host callback is introduced.
See the [approved design](text-sizing.md) for limits and overwrite/reflow rules.
All eight DPR/reflow images are approved golden references (user, 2026-10-13).

OSC 133 C command metadata: Puzzle snippets and modern fish use the existing
`cmdline_url` form. Percent-decoding is bounded, display controls become spaces,
and the host label is at most 1 KiB UTF-8 including a truncation ellipsis
(M12.tabs G1). `cmdline` shell-quoting form is not evaluated; a missing label
remains null without losing the output boundary. [Host contract](../embedding/tabs.md).
