# Last-presented thumbnails

Use `Surface::snapshot(max_width_px, &mut buffer)` in Rust or
`puzzle_surface_snapshot(surface, max_width_px, buffer, capacity, &width, &height)`
in C for drag previews, pane overviews and thumbnails. Call on the surface's
creating thread, including from a tick event callback. The call copies the last
image actually assigned for presentation; it does not render, query Session,
wait for rendering or submit a GPU readback. A hidden surface retains that
image. A pending resize or completed-but-unpresented frame does not replace it.
Only the terminal image is included, with its padding and Puzzle overlays; host
chrome and uncovered space around an older retained image are not included.

The output is **premultiplied, encoded sRGB RGBA8**, top-left first, with tight
rows of `width * 4` bytes. No second gamma conversion or DPR multiplication is
needed. Source alpha stays premultiplied during bilinear sampling. Width is the
smaller of the presented image width and the positive `max_width_px`. Height
preserves aspect ratio, rounded down, at least one pixel; images never enlarge.
The crop is the actual rendered extent, excluding reusable texture-pool padding.

Pass an empty Rust slice, or C `NULL, 0`, to query storage. Rust returns
`SnapshotError::BufferTooSmall(SnapshotSize { width, height })`; C returns
`PUZZLE_EXHAUSTED` with both dimensions. Allocate `width * height * 4` bytes and
call again. On success the dimensions describe the copied image. A newer
presentation between calls may require another allocation; a C size query does
not reserve a frame. Extra buffer capacity is untouched, as are all pixel bytes
on an error. In C, both dimension pointers are required writable `uint32_t`s;
they and the pixel buffer must not overlap. Outputs are zero on other errors
when the output pointers are valid.

Before any image presents, return `Unsupported` / `PUZZLE_UNSUPPORTED`. The same
result applies to a native backend without readable presented storage (currently
the macOS IOSurface path implements it; Windows/Linux presentation is M13).
`Busy` / `PUZZLE_BUSY` means native storage access was unavailable: keep the
previous preview or try on a later host event. Ordinary thread/closed/handle
checks still apply. This additive function leaves ABI-v1 layouts unchanged.

On macOS the image is already in completed shared IOSurface storage. Main's
contents commit publishes its immutable ownership and exact size into a separate
front-image cell; no worker accesses that cell. Copy retains the slot to exclude
GPU reuse and brackets CPU access read-only, then samples directly into the
host buffer. The GPU fence has already completed; the native access bracket handles
CPU cache coherence/paging, not a receive from a worker. It allocates no full-resolution mirror and adds no render pass or
readback to ordinary presentation. Closing clears the front image, even when a
Rust Surface clone outlives native teardown.

The task gate is `cargo xtask accept M12 --task tabs-snapshot`. Its C consumer
tests both NSView and borrowed-layer surfaces, before-first-frame behavior,
capacity/canary rules, exact encoded colour bytes, pending geometry and the
release-only **≤4 ms** 5120×2880 →360×202 copy budget. Pixel tests separately
cover orientation, transparent premultiplied edges and allocation-padding crops.
