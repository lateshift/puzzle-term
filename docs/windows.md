# Windows

The desktop app builds and runs natively on Windows 10/11 through ConPTY and
wgpu (Vulkan or DX12). This page records the Windows-specific behaviour, the
decisions behind it and how to measure input latency.

## Building

```powershell
cargo build --profile live -p puzzle-desktop
.\target\live\puzzle-desktop.exe run
```

Endpoint protection may lock or quarantine the freshly linked
`puzzle-desktop.exe`. A ConPTY shell spawn looks suspicious to heuristic
scanners, and so does the latency probe's synthetic input. If builds fail with
"failed to remove file" or the exe disappears, check the scanner's quarantine
and exclude `target\`. Excluding it also speeds up builds. A still-running
instance holds the same lock: check with `Get-Process puzzle-desktop`.

## Default shell

Without an explicit shell setting, Windows starts `%ComSpec%`, falling back to
`cmd.exe` (`crates/puzzle-embed/src/pty/login.rs`). `SHELL` is ignored on
Windows: MSYS and Git Bash export POSIX paths such as `/usr/bin/bash` that
`CreateProcessW` cannot run. Unix keeps `SHELL`, then `/bin/sh`.

Open question: PowerShell (`pwsh.exe`, then `powershell.exe`) could become the
Windows default instead. It would be a one-line change in the same place.

## GPU backend

No backend is pinned. wgpu chooses among the compiled backends: it picked
Vulkan on the Intel and AMD machines tested, and DX12 remains the fallback
wherever no Vulkan driver exists (VMs, Remote Desktop, some older or ARM
devices). The renderer logs its choice once at startup:

```
event="render.adapter" backend=Vulkan name=Intel(R) UHD Graphics driver=Intel Corporation
```

Set `WGPU_BACKEND` (`dx12`, `vulkan`, `gl`, or a comma list) to override it
without a rebuild. `App` reads the variable itself
(`native_instance_descriptor()` in `crates/puzzle-embed/src/app.rs`) and logs
`render.backend_override`. wgpu's own `*_from_env` helpers cannot be used:
wgpu is built with `default-features = false`, so `wgpu-types` lacks `std`
and its environment lookup always returns `None`.

On screen, Vulkan was never slower than DX12 and was sometimes a frame faster
(see below), so the automatic choice is also the recommended one.

## Input latency

Typing used to feel sluggish on Windows: Backspace appeared after ~550 ms and
other keys after 30–70 ms. ConPTY was never the cause, since echoes reach the
PTY reader about 3 ms after the keystroke. Both causes were between the
read and the present.

### Late frame lost behind a stale reply

`draw_due` bounds the snapshot wait by `build_deadline()`. When the render
thread wakes late, that deadline has already passed, so the frame request gets
no wait at all. The Session owner publishes the frame about 0.1 ms later.
`select_replies` (`crates/puzzle-embed/src/session_thread/endpoint.rs`) used
to report "answered" as soon as it drained any reply, including a stale
NothingNew reply to an earlier request. `begin_snapshot` then skipped
`request_redraw()`, and the late publication waited in the channel until the
next unrelated timer, the 530 ms cursor blink.

`select_replies` now reports whether the current request was answered, so an
unanswered request re-arms the pacer. The regression test is
`freshness::stale_reply_at_expired_deadline_leaves_current_request_unanswered`.

### Timer resolution

Windows rounds `recv_timeout` up to the system timer tick, 15.6 ms by default,
so every render-actor deadline overshot by up to 17 ms. That late wake is also
what expired the deadline above. `puzzle_sys::timer_resolution::Resolution` is
an RAII guard for `timeBeginPeriod(1)`/`timeEndPeriod(1)` that does nothing on
other platforms. `App` holds one on Windows for its lifetime, so the guard
covers every host, not only `puzzle-desktop`. Overshoot dropped to 0.2–3 ms.

### Measurements

Intel UHD, 60 Hz, cmd.exe. The "in-app" column runs from the PTY read to the
present call and comes from temporary trace logs. "On screen" runs from the
keystroke until the probe sees pixels change.

| State | Key | In-app (median) | On screen (p50 / p90) |
|---|---|---|---|
| Before | Backspace | ~550 ms | — |
| Before | `.` | 26 ms, outliers to 275 ms | — |
| Stale-reply fix | Backspace / `.` | 38 / 40 ms | — |
| Both fixes, Vulkan | Backspace / `.` | 14–22 ms | 20–34 ms / 35–51 ms |
| Both fixes, DX12 | Backspace / `.` | 17–24 ms | 35–37 ms / 52–67 ms |

On-screen samples cluster at about 17, 34 and 51 ms, so the compositor
quantizes them to whole refreshes. The backends match inside the app, and
their difference lies after present, in the swapchain and DWM. What remains is
by design: the 8 ms settle window (`render.settleMs`) and alignment to the
next refresh.

### Frame queue depth (D-35)

Surfaces use `PresentMode::Fifo` with `desired_maximum_frame_latency = 2`.
D-35 measured that the second queued frame removes missed refreshes for
+1–1.5 ms median input latency. Lowering the depth to 1 would trade that
stutter for about a millisecond. `Mailbox` is not a drop-in alternative,
because the pacer learns the refresh phase by filling the idle FIFO. Neither
has been re-measured on Windows.

## Measuring

`xtask/scripts/windows-keylat-probe.ps1` starts the app, focuses it and
alternates `.` and Backspace through `SendInput`. It reports how long until
the top of the window changes on screen. While it runs, about 15 s with
`-N 20`, it takes focus and types into the window.

```powershell
. .\xtask\scripts\windows-keylat-probe.ps1 -Exe .\target\live\puzzle-desktop.exe -N 30
. .\xtask\scripts\windows-keylat-probe.ps1 -Exe .\target\live\puzzle-desktop.exe -N 30 -Backend dx12
```

- The probe makes itself per-monitor DPI-aware. Without that, every sample
  timed out on scaled displays (225 %).
- It stops the app it started even when a run fails. Instances started by hand
  still block relinking, so stop them before rebuilding.
- The app's stderr goes to `%TEMP%\kl_*.log` (`$global:KLLOG`).

For an in-app timeline, add temporary `tracing::info!(target: "keylat", ...)`
events and run with `PUZZLE_LOG=warn,keylat=info` (the desktop app reads
`PUZZLE_LOG`, not `RUST_LOG`). Useful places are the PTY
read, the pacer's output request, `draw_due`, the snapshot in
`begin_snapshot`, `publish_frame` and the present call. Use info level:
`release-logs` compiles trace-level events out of the build.

## Known gaps

- `cargo test -p puzzle-embed`: 15 tests spawn `/bin/sh -c …` and fail on
  Windows, including `real_pty_da_cpr_reply_order` and `app_drop_joins_open_surfaces`.
  Two system-font tests (`no_outline_candidate_is_evaluated_only_once`,
  `collection_faces_share_one_mapping`) depend on the installed fonts.
- `puzzle-embed` has dead-code warnings on Windows for helpers that only
  Unix-only tests use (`drain_pid`, `process_id`).
