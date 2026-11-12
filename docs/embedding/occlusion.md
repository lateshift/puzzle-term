# Tell Puzzle where host UI covers a surface

Report native find controls, popovers and other host UI per surface so Puzzle can
position floating overlays and centre search results in the remaining area.
This is presentation metadata: it does not add padding, resize the grid or PTY,
reflow text, or change pointer coordinates, selection or copied text.

```c
puzzle_rect_t find_box = {x, y, width, height};
puzzle_surface_set_occluded_rects(surface, &find_box, 1);
/* When the native UI closes: */
puzzle_surface_set_occluded_rects(surface, NULL, 0);
```

Rust: `surface.set_occluded_rects(&[[x, y, width, height]])?`; pass `&[]` to clear.
The call replaces the entire list, copies it before returning, and queues owner
work. Use the App/surface's creating thread. Keep ticking on worker wakeups as
usual; no call waits for a Session or a frame.

Coordinates are **view-local logical top-left points**, exactly the space used
by `surface/cursorRect`. Do not multiply by backing scale, subtract padding or
the producer gutter, or use window/screen coordinates. Convert native UI bounds
into that surface's view, including a Y flip for bottom-left native coordinates.
Include any shadow/edge area you want left clear. Outside-surface rectangles are
allowed; zero-area rectangles cover nothing.

At most 64 rectangles are accepted (`Surface::MAX_OCCLUDED_RECTS`). Coordinates
must be finite and representable as logical render coordinates; width and height
must be nonnegative. Invalid input rejects the entire update and preserves the
previous list. The C array must be initialized/readable for the call; count zero
permits a null pointer.

Update the list whenever host UI appears, moves or changes size. Clear it when
hidden. Lists belong to each surface and survive view/window moves; update them
if the UI's view-relative position changes.

Floating search/help panels, HUDs, toasts, new-output pills and movable hints/
tooltips try unobscured positions at rectangle edges, keeping their usual place
when it is free. If a panel cannot fit, it is omitted with its hit targets until
space becomes available. Source-anchored text, decorations, preedit, gutters,
the scrollbar and the grid remain in place.
Interactive source controls stay within their source bounds: slide left on the
same row, then try the first unobscured visible row below; if neither fits, omit
the control and its hit target, while fixed source ink keeps its position.
The fixed scrollbar track excludes only covered vertical bands from paging; a
partially exposed thumb keeps its full drag target, and only a fully covered
thumb loses that target. Native UI above the surface owns covered pointer input.

Search treats covered matches as outside the visible area for initial selection.
Navigation centres the match in the largest unobscured row band for the match's columns, revealing
user-folded results as usual. Existing document bounds still clamp scrolling;
occlusion cannot invent rows above history or reveal text when the host covers
every feasible position. Leave usable grid space for results to remain visible.

Built-in and host search UIs share this policy. A native find component usually
chooses `searchUi=host` and reports its box here. Custom overlay views can use
`ViewCtx::place` to share the placement rule. With no occlusions, placement and
navigation keep their original behaviour.

Product checks: `cargo xtask accept M12 --task occlusion`. Native component
placement remains the user's live review after the host adopts the API.
