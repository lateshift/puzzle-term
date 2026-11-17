# Linux

The desktop app builds and runs on Linux through a Unix PTY and wgpu (Vulkan
or GL). It was tested under WSL2 with WSLg (Ubuntu 26.04). This page records
the Linux-specific setup, the GL backend fixes and the WSLg limitations.

## Building

```bash
sudo apt install build-essential pkg-config libfontconfig-dev
cargo build --profile live -p puzzle-desktop
./target/live/puzzle-desktop run
```

The binary links only fontconfig and freetype. winit loads X11, Wayland and
xkbcommon at runtime, and wgpu loads the Vulkan and EGL drivers the same way, so
those need runtime packages only. GTK is not needed.

| Runtime package | Needed for |
|---|---|
| `libxkbcommon0` | Keyboard handling (both X11 and Wayland) |
| `libxkbcommon-x11-0` | X11. Without it the app panics with "Library libxkbcommon-x11.so could not be loaded". |
| `libwayland-client0`, `libx11-6`, `libxcb1` | The respective display server |
| `mesa-vulkan-drivers` or a vendor Vulkan driver | The Vulkan backend |
| `libegl1` and a Mesa GL driver | The GL backend |

## Default shell

The app starts `SHELL` as a login shell (`-l`), falling back to `/bin/sh`
(`crates/puzzle-embed/src/pty/login.rs`).

## GPU backend

No backend is pinned. wgpu prefers Vulkan and falls back to GL. The renderer
logs its choice once at startup:

```
event="render.adapter" backend=Vulkan name=llvmpipe (LLVM 21.1.8, 256 bits) driver=llvmpipe
```

`WGPU_BACKEND=vulkan` or `WGPU_BACKEND=gl` overrides it (see
[windows.md](windows.md#gpu-backend) for how `App` reads the variable).

Use `PUZZLE_LOG`, not `RUST_LOG`, to change the log filter, for example
`PUZZLE_LOG=info,puzzle_desktop::resize=trace`. Trace output needs a build
without the default `release-logs` feature
(`--no-default-features`), which otherwise compiles trace events out.

### GL backend

Three things kept GL from working on Linux. All three are fixed:

- **No display handle.** EGL needs the host's display connection when the
  instance is created. `AppConfig::display` now carries it: puzzle-desktop
  passes winit's `OwnedDisplayHandle`, and hosts without one (the FFI CALayer
  host, tests) pass `None`. Before, `WGPU_BACKEND=gl` found no adapter at all.
- **Compute limits.** `DeviceDescriptor::default()` asks for WebGPU's default
  limits, including compute and storage limits. GL contexts without compute
  shaders report zero for those, so device creation failed. The renderer uses
  no compute or storage resources, so `device_descriptor()` in
  `crates/puzzle-render/src/renderer.rs` asks for zero there and keeps every
  other default.
- **Invisible text.** wgpu's GL backend picks the GL texture target from the
  layer count alone: a one-layer texture becomes `GL_TEXTURE_2D`, and the
  raster shader's `texture_2d_array` then samples zero coverage, so no glyph
  appeared. On GL, the glyph atlas now starts with an unused second layer
  (`physical_layers()` in `crates/puzzle-render/src/raster_pipeline/pages.rs`).
  The glyph budget still counts requested pages (`mask_pages`, `color_pages`),
  so the minimum budget still admits its first page. The padding costs about
  20 MB of GPU memory, on GL only, until the atlas grows past one page.

## WSL2 / WSLg

### Wayland crashes WSLg's compositor

Under WSLg the app defaults to Wayland, and WSLg's weston (9.0) segfaults as
soon as the window appears. The app sees "Broken pipe". Use X11 through
XWayland instead:

```bash
env -u WAYLAND_DISPLAY ./target/live/puzzle-desktop run
```

After a weston crash, new windows from any X11 or Wayland app may stop
reaching the Windows desktop. `wsl --shutdown` from Windows restarts WSLg.

### GPU acceleration

WSL has no Vulkan driver for the host GPU unless Mesa's `dzn` driver is
installed, so Vulkan falls back to `llvmpipe` (software). Mesa's `d3d12` GL
driver reaches the real GPU through `/dev/dxg`:

```bash
env -u WAYLAND_DISPLAY GALLIUM_DRIVER=d3d12 WGPU_BACKEND=gl ./target/live/puzzle-desktop run
```

On a machine with two GPUs, Mesa picks the integrated one, which also fits the
renderer's `LowPower` preference. To use a discrete card, name it:

```bash
env -u WAYLAND_DISPLAY MESA_D3D12_DEFAULT_ADAPTER_NAME=NVIDIA GALLIUM_DRIVER=d3d12 WGPU_BACKEND=gl ./target/live/puzzle-desktop run
```

Tested adapters on a Dell XPS 15:

| Command | Adapter |
|---|---|
| default | Vulkan, llvmpipe |
| `WGPU_BACKEND=gl` | GL, llvmpipe |
| `GALLIUM_DRIVER=d3d12 WGPU_BACKEND=gl` | GL, D3D12 (Intel UHD Graphics) |
| `MESA_D3D12_DEFAULT_ADAPTER_NAME=NVIDIA GALLIUM_DRIVER=d3d12 WGPU_BACKEND=gl` | GL, D3D12 (NVIDIA GeForce RTX 4050 Laptop GPU) |

The libEGL warnings about DRI3 and `/dev/dri` permissions are harmless:
XWayland under WSLg has no DRI3, and EGL falls back to a path that still
presents.

### Resizing stretches the text

While a window is being resized, WSLg stretches its last image to the new
window size until a newer one arrives, so text briefly looks squashed or
enlarged. xterm shows the same effect, and native Windows does not. The app
reconfigures its surface on every size step (about 1–2 ms per turn on the
RTX 4050), so this is WSLg's presentation, not ours.

## Known gaps

- Wayland is untested outside WSLg, whose compositor crashes.
- Linux input latency has not been measured. The Windows probe
  (`xtask/scripts/windows-keylat-probe.ps1`) has no Linux counterpart.
- `cargo test -p puzzle-render` alone does not compile, because the crate
  relies on its dependents to enable wgpu's `wgsl` feature. Add
  `--features wgpu/wgsl,wgpu/vulkan` when testing it on its own.
