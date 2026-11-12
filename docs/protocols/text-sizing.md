# OSC 66 text sizing — approved M18.7 design

Status: **implemented and accepted in M18: user app/harness scene, copy and
eight reference images approved 2026-10-13; reviewer comments approved 292e4be**.
The sparse block model, atomic rectangular reflow, selection/search extents,
cursor extent and bounded storage below are approved. Existing budgets and
platform boundaries stay unchanged; support is advertised only after step 5.

## Reference and compatibility

Origin: kitty, [text sizing protocol](https://sw.kovidgoyal.net/kitty/text-sizing-protocol/),
retrieved 2026-10-12. The reference controls wire syntax and editing behavior.
Puzzle retains its deliberately specified D-31 grapheme/width behavior where
kitty's evolving Unicode algorithm differs; explicit widths remain authoritative.
This is grid text, not an overlay or image placement.

| Layer | Puzzle before M18.7 | Implemented Puzzle |
|---|---|---|
| OSC 66 parser | Unsupported | Kitty-origin parser, same bounds |
| Terminal application | Unsupported | Persistent multi-cell grid text |
| Selection, reflow, rendering | Unsupported | Defined below and mechanically tested |

Parser support alone is not reported as working text sizing in the support
matrix. The proposed terminal behavior follows kitty's published protocol.

## Wire boundary

The kitty origin module parses `OSC 66 ; metadata ; text BEL/ST`. Split only
at the metadata delimiter; semicolons inside text remain text. The six bounded
numeric keys are s (1–7), w (0–7), n/d (0–15), and v/h (0–2), with the defaults
in the reference. Ignore unknown or invalid pairs individually, retaining the
last valid value for a repeated key.

Accept escape-code-safe UTF-8 payloads of at most 4096 bytes. Reject invalid or
truncated payloads as a whole, including controls that vte would otherwise
silently remove; consume through the terminator without swallowing later output.
Reuse the bounded raw-OSC observer already used by 99/3008. Empty text is a
no-op. No clipboard, host action, new reply, capability bit or terminfo entry is
introduced. Feature discovery uses the reference's three CPR observations.

With w=0, split text using Puzzle's existing mode-2027/D-31 cluster rules and
create one scaled block per normal character cluster. With w>0, all text in the
payload belongs to one block whose width is explicitly fixed. A subsequent
joining mark may change its text, never its explicit width. The existing
32-byte cap still bounds individual grapheme joins; the 4096-byte explicit
payload is a bounded text run, not permission for an unbounded grapheme.

Integer scale controls occupied cells. Fractional scale controls ink only;
use n/d when 0<n<d, otherwise use the integer size. Alignment positions that
fractional render area within the occupied rectangle. For text too long for an
explicit width, fit uniformly into the requested ink area without changing cell
occupancy (a choice the protocol permits). Metadata is per operation, not a
persistent SGR or global font-size setting.

## Cell model and ownership

Keep `Cell` exactly 16 bytes. Add a layout flag identifying multi-cell text;
its `ch` word then holds a checked block key rather than a Unicode scalar.
Every occupied cell references that block; only its logical owner emits text.
Foreground, background, protection and attributes keep their existing inline
representation. Normal cells and the existing wide-character representation
remain unchanged.

Add a lazily created, screen-owned `MulticellStore`, separate for primary and
alternate screens. A block record owns text, scale/alignment, explicit-width
status, starting column and a short list of row fragments. Each fragment records
its stable row identity and original vertical offset. A row summary flag and
sparse row-to-block index avoid discovering these records by scanning cells.
The maximum occupied rectangle follows from the protocol: up to 49 columns and
7 rows for an explicit-width block.

Keys are slot indices allocated from a free list. A slot is freed only after no
cell references it; the consistency checker proves this before reuse. A generation
is checked in debug builds only and never wraps. On exhaustion, discard the block,
increment the discard counter and log at debug. Each sealed
publication owns its key-to-record copies and never resolves a key against the
mutable Session store, so old publications need no reference-count message.
An invalid reference reads as an erased cell and is a debug invariant failure
in tests, never a panic caused by terminal input.

VT owns all block mutation. Printing/editing intersects only the current row's
sparse entries and the at-most-seven fragments of affected blocks. A row-handle
move carries its index with it; it does not update every historical anchor.
Retirement is tied to the rows actually removed. In particular, no global
per-scroll scan of the store is allowed (the M15 retired-anchor lesson).

`BufferView` exposes borrowed block text and geometry. A `FrameSnapshot` copies
only visible block records once, including the payload of a block whose first
row is above the viewport. Each slot owns that copy; it cannot borrow from the
Session or retain a mutable store handle. D-50's publication ownership and the
single-threaded web path therefore need no new synchronization.

## Placement, wrap and editing

Before writing, compute the whole rectangle and check screen dimensions using
checked integer arithmetic. A block larger than the screen is discarded. If
it fits the screen but crosses the active right margin, autowrap starts it on
the next line; without autowrap, back up enough columns to fit. Existing origin
mode and scroll margins still apply. At the bottom margin, scroll enough rows
to fit the block before writing its fragments, preserving the same full-width
history versus partial-region discard rules as ordinary output.

Printing advances horizontally by the rectangle's width, on its initial row;
vertical advance remains the application's responsibility. Reserve the lower
rows rather than inserting new lines into unrelated content. When ordinary
printing reaches a lower fragment, skip to the block's right edge before
printing, even with autowrap disabled. Repeated adjacent blocks are skipped
with guaranteed cursor progress, bounded by the active row/region.

Apply kitty's overwrite precedence: combining text joins the block; overwriting
its first cell removes it; overwriting another cell on its top row clears the
entire rectangle to spaces; encountering a lower fragment skips it. Erasure
uses the current BCE background. All touched rows receive damage/version
updates, including fragments outside the directly addressed row.

ICH/DCH remove intersected multi-row blocks in the shifted suffix and single-row
blocks cut by an operation boundary. Intact single-row blocks can shift as a
unit. ECH/EL/ED erase every intersected block in full. IL/DL and partial-region
scrolls remove blocks split by their boundaries, then move intact row handles.
Full-width scrolling moves fragments into history without rewriting their cells.
Selective erase respects a block's protection as a unit. RIS/alternate-screen
clear release that screen's records just as they clear its cells. OSC 3008
contexts are independent and keep their approved RIS lifetime.

Unsupported controls stay unsupported: for example the protocol's reference
to ED 22 does not silently add that sequence to Puzzle's base VT feature set.

## Resize and reflow

Preserve ordinary logical-line reflow when no multi-cell record is present.
For an affected logical band, treat a block as an indivisible rectangular item,
with text emitted once and its lower-row footprint represented as reservations.
Pack items left to right in source text order into destination rows. A candidate
must fit its full rectangle in free cells; otherwise advance to the next fitting
position/row. Respect hard line breaks and skip the block's lower reservations
when placing following text. Never flatten lower fragments into spaces that
would become extra copied characters or false hard line breaks.

Build the sparse occupancy map only for bands containing blocks, using reused
scratch storage. Track source owner/fragment positions to destination
owner/fragment positions in the same pass for cursor, selection, links and other
tracked anchors. Rows not involved in a block use the existing mapping. A block
larger than the new screen dimensions is removed as the protocol allows; damage
and tracked-position disposal accompany its removal. Alternate screen keeps its
existing no-reflow truncation, removing any block cut by the new boundaries.

Concrete acceptance examples include `s=2` A followed by ordinary BC, widths
8→5→8, and a w=3/s=2 block resized to width 5 (removed). A two-row block crossing
the viewport/history boundary must remain one logical text item. Vertical
resize uses the existing blank-below-cursor-first rule; it must not revive an
erased fragment from history or duplicate shell prompts.

## Selection, copy, search and serialization

Pointer hit-testing returns both the addressed physical cell and its block
owner/extent. Mouse reports and CSI cursor moves still use physical cell units;
hosts continue to pass view-local logical points without special subtraction.
Normal selection expands an intersected block to its full footprint. Rectangular
selection preserves its rectangle but copies an intersected block's text once,
at its first selected fragment, with no text from continuation cells. Plain
copy, HTML and logical-line access retain payload order and suppress artificial
newlines from reserved lower fragments. Word selection segments the retained
text, mapping each selected byte span to its block rectangle.

Search visits each owner text once, using the existing incremental line budget.
Matches map to block extents rather than matching blank continuations; a match
inside an explicit-width run highlights that indivisible block. Navigation
reveals the complete extent where possible. A root above the viewport remains
searchable/copyable while any fragment survives in retained history.

VT serialization emits the needed OSC 66 operation once per owner, restores
styles, and positions subsequent text explicitly where reservations require it.
Add round trips for mixed scaled/unscaled text, retained fragments and reflow.
Bulk cell access gains an optional sparse block table (text, dimensions,
scale and fragment origin), allocated only when requested, so callers do not
mistake a block key for a code point. Existing ordinary-cell buffers retain
their current meaning. New wire/API fields are documented and tested together;
no ABI size change to an existing C struct is required.

## Cursor and rendering

The actual cursor remains one physical grid coordinate. CPR never reports a
logical block number. On a fragment, block cursors cover the block extent; bars
cover its first column and underlines its bottom edge, clipped to visible
fragments. The glyph-under-cursor redraw uses the same transform and clip as
ordinary ink. `cursorRect` reports that logical-pixel extent for IME candidates,
or null when fully outside the viewport; host geometry stays centralized.

Frame building resolves a visible block once, shapes its retained text through
the existing fallback/shaping path, and places scaled glyphs in its occupied
rectangle. Scale and fractional alignment are transforms through the existing
Slug pipeline, not font settings or an overlay. Outline-backed transformed text
uses unhinted vectors under §8.9/D-47; bitmap-only emoji/fallback selects the
effective device-size source and existing premultiplied sampling. Font/DPR
changes alter geometry/cache keys, not VT occupancy or record lifetime.

Background, text decorations, selection and minimum contrast retain §10.5 order.
Cell-dependent selection/cursor colors use clipped portions of the same block
transform; they must not reshape or independently fit each fragment. Damage to
one fragment invalidates the associated block rendering on every visible
fragment. Row caches include block revision/geometry along with existing row
identity/style epochs. Scrolling can reveal a lower fragment with its owner
outside the snapshot without missing ink or dereferencing Session state.

## Storage bounds and zero cost when unused

Charge text capacity, block records, fragment arrays and sparse indexes exactly
once to the existing retained-row byte accounting. History eviction releases
fragments/records and keeps the configured scrollback allowance; active-screen
storage is also explicitly counted, bounded by occupied cells and the protocol
payload limit. Do not retain a 4096-byte allocation for a retired one-cell block.
Reusable spare storage remains charged and cannot grow without limit. If a block
cannot be admitted within the existing storage bound, discard it rather than
claiming support for a partially stored rectangle. This needs no new setting.

The optional store has no allocation before first use. Ordinary printing,
scrolling, snapshot copying, search and frame building do not scan for this
feature. A conservative per-row summary chooses the existing ordinary path
before any sparse lookup, and absence of a screen store bypasses mutation work.
Idle text adds no timers or frames. No extra ordinary per-cell probe/pass is
permitted: branch at a row/operation boundary, using the existing special-layout
path only for flagged rows/cells. Sparse feature memory is included in the
snapshot/scrollback cost reports; nothing is hidden in a global registry.

## Implementation and approval checks

After approval, implement in this order:

1. Kitty parser and pure VT block model, placement and editing; conformance over
   both terminators, split chunks, malformed input, limits, overwrite precedence,
   explicit-width variation selectors, margins and CPR discovery.
2. Retention/reflow and read APIs, selection/copy/search/serialization; tests for
   history eviction, clipped roots, rectangle selections, tracked positions,
   alternate screen and repeated shrink/grow. Pin `size_of::<Cell>() == 16`.
3. Sealed snapshot data and existing glyph pipeline, cursor/IME geometry; golden
   candidates for headings, fraction/alignment, mixed lines and reflow at DPR
   1/2, with mechanical ink/clip assertions. Human references stay pending.
4. Count/allocation proof that image-free, OSC-66-free ASCII/Unicode printing,
   plain/region scrolls and warm frame assembly retain current work; no new
   allocations or full-row scans. Measure affected §15 frame/snapshot/reflow
   budgets without weakening limits, and run shared wasm/Chrome checks.
5. Add independent M18.7 acceptance lines and app/harness review scripts. Update
   support only when these behaviors exist; list any failure, never advertise a
   parser-only implementation. No new dependencies or handwritten unsafe.

## Binding approval additions (reviewer, 2026-10-12)

- Add a debug/test-only consistency checker: every flagged cell references a live
  block, recorded fragments exist at their rows/columns, blocks never overlap,
  and exact storage accounting agrees. Run it after every operation in tests.
- Add property tests over interleaved OSC 66 (every key, w=0/w>0, fractions),
  ordinary printing, ICH/DCH/ECH/EL/ED/IL/DL, margins, scrolling, history eviction,
  shrink/grow reflow, alternate screen, RIS and selection. Assert no panic,
  consistency after every operation and copied logical text against an oracle.
  Extend the existing parser property target with OSC 66.
- Commit each of the five steps separately with a green `cargo xtask check`.
  Steps 1–2 change no existing golden or budget.
- Only after step 5 passes, the support index reports Puzzle as "full".
- Add both-host review scripts to `docs/protocol-review.md`: s=2/3 headings,
  aligned fractional n/d, mixed scaled/unscaled text, resize 8→5→8 columns,
  and selection/copy of a heading. Visual review remains pending under R15.

M18.8 shell injection waits until this implementation and verification finish.
