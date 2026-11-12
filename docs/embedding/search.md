# A replaceable find UI

Puzzle owns literal search, tracked matches, highlights and viewport navigation.
Your host chooses who provides the query controls for each surface (D-61).
The built-in bar and a native component use the same Session search model;
a host never needs to copy scrollback or implement a second search engine.

## Choose the UI

Safe Rust: set `SurfaceOptions.search_ui` to `SearchUi::BuiltIn` (the default)
or `SearchUi::Host`. After creation, `Surface::set_search_ui` changes that choice.

C: `puzzle_surface_set_search_ui(surface, 0)` selects `builtIn`, and `1` selects
`host`. Call it immediately after creation, before forwarding input. This setter
preserves the ABI-v1 `puzzle_surface_options_t` layout. Other values are rejected.
The choice is per surface and survives settings/keybinding reloads.

With `builtIn`, the bound `search` action opens Puzzle's overlay, which handles
editing, IME and navigation. With `host`, it emits `search/requested` with JSON
`null` and does not open or focus the overlay. Show/focus your own find box when
that event arrives. Ordinary terminal text continues to reach the child.
The legacy bus fact `terminal/searchRequested` is retained in both modes; use
`search/requested` for native UI handoff, so you do not open two interfaces.
In Rust this handoff is `AppEvent::Host(HostEvent::SearchRequested)`.

Switching an open built-in bar to `host` closes its input capture but preserves
matches. Switching back does not open a bar until another search action.
Commands that run a search never open a UI themselves.

## Query and navigate

| C call | Result |
|---|---|
| `puzzle_surface_search(surface, query, case_mode)` | Replace the query; an empty query clears it. UTF-8 literal text, no regex. Case mode: `0` smart (uppercase selects sensitive), `1` sensitive, `2` insensitive. |
| `puzzle_surface_search_next(surface)` | Select the next discovered match, wrapping at the end; centre it in the viewport. |
| `puzzle_surface_search_prev(surface)` | Select the previous discovered match, wrapping at the start; centre it. |
| `puzzle_surface_search_clear(surface)` | Cancel incremental work and remove matches/highlights. Call when the host closes its find box. |

Safe Rust has `Surface::search(SearchCommand::Find { query, options })`,
`search_next`, `search_prev` and `search_clear`. `FindOptions.case_sensitive` is
`None` for smart case, `Some(true)` for sensitive and `Some(false)` for insensitive.
Navigation uses matches already found; a partially scanned history does not block
the UI while waiting for more results. You may queue next or previous immediately
after starting a query. While there are no matches, the Session retains one pending
step (the latest direction) and applies it when the first match is discovered,
including centring it. The step applies after automatic selection, so immediate
Enter behaves exactly like waiting for results and then pressing Enter. Replacing or clearing the query discards that pending step.
Hosts do not need to buffer navigation intent themselves.

`search/state` reports JSON `{count,current,complete,scanned}` when progress or
navigation state changes. `current` is a **zero-based** index, or `null` only when `count` is zero.
When a query first finds matches, Puzzle selects the visible match nearest the
bottom of the viewport without scrolling. If none is visible, it selects and
centres the first discovered match. Later slices keep that selection; only a
new query or explicit next/previous changes it (unless its source is removed). `count` is the capped number found so far; `complete`
means the retained source is fully searched or the result cap was reached.
`scanned` is physical rows inspected in the last search slice. Live output may
make results partial again. A no-op need not emit another identical state.
Display a one-based counter in your UI if desired, but keep the API index zero-based.
In Rust the value is `HostEvent::SearchState(SearchState)`.

## Use Selection for Find

Call `puzzle_surface_selected_text(surface, request)` (Rust:
`Surface::selected_text(request)`). The owner reads the selection at that
message's ordered position and returns `selection/text` JSON `{request,text}`;
Rust receives `HostEvent::SelectedText { request, text }`. The token is your own
`u64`, returned unchanged: parse it exactly and ignore answers for stale UI work.
With no surviving selection, `text` is `""`. Extraction uses the current plain-text copy
rules, including whole grapheme clusters and trimming preferences. It neither
changes the clipboard nor sends input to the child. A host may place the answer
on its platform's shared find pasteboard, then start a new query.

## Threading and lifetime

All calls above run on the App/surface's creating thread and enqueue messages.
Success means **queued**, not finished. They never wait for a Session or scan
scrollback on the UI thread. Keep pumping `puzzle_app_tick` / `App::tick` after
the worker-safe wakeup; only tick delivers events on the creating thread.
Callbacks may enqueue these surface commands. Do not call tick recursively.

C strings and event payloads are borrowed only for the call/callback: copy data
that your UI retains. Search and UI choice are independent per surface. Closing
or synchronously freeing a surface suppresses pending events; drop the component's
cached state with that surface. View/window moves preserve the surface's search.

The reusable AppKit component belongs to `platforms/macos/PuzzleSearch/` and
session 2. It depends on a small host-language protocol, never example-app types
or `puzzle.h`; each embedder adapts that protocol to these calls and events.

Product checks: `cargo xtask accept M12 --task search`. Native component appearance,
keys, find pasteboard and focused-pane routing remain the user's live review.
