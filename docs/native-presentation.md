# macOS native presentation (M12.layer / D-60)

The harness and C host use the same `puzzle-embed` path. The host's existing
CAMetalLayer remains the stable container (native kind 5). Puzzle installs a
host-sized CALayer subclass beneath it, renders into BGRA8Unorm IOSurface
textures, and sets those finished images as the child's contents. The IOSurface
carries an sRGB ICC profile. There is no drawable acquisition on this path.
Puzzle’s child also has an explicit sRGB `backgroundColor`, matching the default
rendered background and opacity. It exists before the first image, follows live
theme/config changes (and rendered default-colour overrides), and fills bounds
that a retained old-size image does not yet cover. Background, scale/gravity and
contents updates share a disabled-actions CA transaction. Both native host paths
receive this in Rust; hosts need no additional colour-setting calls.

The host owns container bounds and backing scale. Forward size/scale/display
changes through `puzzle_surface_resize` before the Core Animation transaction
commits. Initial creation geometry and later physical width/height/scale updates
are the single presentation key.
Puzzle divides those pixels by scale without rounding to whole points (991 px
at scale 2 is exactly 495.5 pt), and sets its child bounds/position directly.
Native parent-resize callbacks schedule display, bypassing CALayer's default
point-rounded autoresizing. A remaining CA/host size disagreement resolves to
the host key; it cannot veto or permanently suppress asynchronous presentation.
Do not set drawableSize or replace Puzzle's child. Retain the container
until synchronous `puzzle_surface_free` returns. Moving the same container to
another view/window preserves the surface identity. Pump `puzzle_app_tick` from
wakeup as before; firstFrame and cursorRect callbacks are still delivered only
by tick, after actual contents assignment.

When bounds or backing scale differ from the last committed image, the child's
`display` callback requests those exact physical dimensions and may wait for at
most one current display period. This covers edge drags, Magnet jumps and display
moves even without an AppKit live-resize notification. The callback commits a
matching GPU-completed image in the current CA transaction. On timeout it retains
the previous, unscaled image at the visual top left. A later GPU completion wakes
the normal asynchronous path. At unchanged geometry the main thread never waits;
ordinary frames remain display-link paced. A timed-out request retains the one
waiting credit until a GPU answer arrives; a later size replaces the request
asynchronously. Repeated callbacks for that pending geometry never wait again.
Waiting admission never gates asynchronous delivery. Every uncommitted latest
geometry can be reposted from ordinary tick; the render mailbox coalesces it.
Host geometry retires obsolete completed images immediately, and a delayed GPU
completion cannot reinstall them. This leaves a free slot serviceable even while
old window transactions retain the other images. An image for the latest host
size is preserved and the child is laid out at that exact key if CA layout lags. Old CA bounds cannot replace
the newer host geometry. A changed native hierarchy resets per-host wait gating
without changing the surface identity or allowing old completion ordinals to
answer a newer request. Glyph work remains on the render thread and input remains
on the independent Session/writer path.

The pool derives its bound from native frame latency plus the displayed front
image. GPU completion, the ready mailbox, layer ownership and IOSurface's compositor
use count all prevent premature reuse. Device replacement invalidates imported
textures. Each slot grows into device-limited power-of-two size buckets and is
reused for smaller intermediate sizes. `contentsRect` selects the exact rendered
device-pixel rectangle; allocation capacity never determines grid size or scales
the image. Session geometry messages coalesce within a control turn, with input
as an ordering barrier. PTY notifications use a first-event 25-ms throttle:
later events replace the size without resetting the deadline, so the child
redraws throughout a continuous drag and receives the final size within 25 ms.
Close makes retained callbacks inert before cancelling/joining workers.
Hit regions and caret notifications refer to the image actually assigned, including
when CA coalesces intermediate GPU completions during native event tracking.

Sources inspected 2026-10-17: the locked wgpu 30.0.1 / objc2 0.6.4 / objc2 0.3.2 generated binding sources.

Run the task-level mechanical gate: `cargo xtask accept M12 --task layer`.
Native readbacks retain exact theme-byte and sRGB assertions; offscreen golden
references are unchanged. The remaining five-run ProMotion criterion and the
four transferred M16 observations belong to M12.refresh, with budgets unchanged.

User review remains pending in both hosts: continuous edge drags, sudden half/full
Magnet changes, and moves between LG 5K (60 Hz) and built-in (120 Hz). Verify that
stale ink stays beneath the title bar, never stretches, and is replaced at the
new size; type during movement and check close during output. Session 2 owns the
app adaptation/build and its visual receipt; this task edits no app source.
