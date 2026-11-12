# Puzzle Terminal

Puzzle is an embeddable, GPU-rendered terminal engine written in Rust. It combines
an xterm-compatible terminal core with Unicode grapheme handling, font fallback,
colour emoji, ligatures, kitty graphics, shell integration, search and overlays.
Text rendering uses a WGSL implementation of Eric Lengyel's Slug algorithm,
with a bitmap cache for untransformed grid glyphs.

Native embedders use **puzzle-embed** (safe Rust) or **puzzle-ffi** (C ABI). The
host owns its windows, tabs, panes, input and native UI; Puzzle owns terminal
sessions, rendering and, by default, the PTY child. A host can supply a byte
transport instead. The repository includes a winit harness and an AppKit example
application. The shared core also builds for WebAssembly/WebGPU; native macOS
is the actively exercised desktop integration.

## Build and run

Install Rust with rustup and the components/target in `rust-toolchain.toml`.
The pinned toolchain is selected automatically. On macOS, install Xcode and its
command-line tools. Run commands from the repository root:

```sh
cargo xtask check
cargo build --release -p puzzle-desktop
cargo xtask run
```

`cargo xtask run` rebuilds as needed before launching the harness with your login
shell. Configure it with `--config PATH`, `--profile NAME` or `--set key=value`.
See [settings](docs/SETTINGS.md), [shell integration](docs/shell-integration.md)
and the [protocol support matrix](docs/protocols/support.md).

## Embed Puzzle

Use `puzzle-embed::App` and `Surface` from Rust. Worker notifications wake the
host; the host calls `App::tick` on its UI thread to receive events. Input and
settings become messages to each Session's owner. Hosts never lock a Session.

For other languages, build the static library and use the checked C header:

```sh
cargo build --release -p puzzle-ffi
```

The interface is [puzzle.h](crates/puzzle-ffi/include/puzzle.h). The independent
[C host fixture](crates/puzzle-ffi/tests/host.c) demonstrates ownership, input,
transport credit and synchronous teardown. Embedding contracts cover
[search](docs/embedding/search.md), [input timestamps](docs/embedding/input.md),
[colours](docs/embedding/colors.md), [native UI occlusion](docs/embedding/occlusion.md),
[tab facts](docs/embedding/tabs.md) and [snapshots](docs/embedding/snapshot.md).
See [native presentation](docs/native-presentation.md) for macOS layer ownership.

## macOS example app

The AppKit application in `apps/macos` consumes the C ABI. It demonstrates
native windows, tabs, split panes, IME, clipboard, terminal search and display changes.
With Xcode installed:

```sh
cargo xtask app macos
```

This builds the release static library in `target/apps-macos/release`, runs
`xcodebuild` with Puzzle enabled, and executes the app's test scripts. The native
tests create windows and require an active macOS desktop. Build products stay
under `target/`; the working tree needs no developer-specific paths.

## Development

`cargo xtask check` runs formatting, native and WebAssembly Clippy, workspace
unit/integration tests, C-header checks and source/dependency lints. Native GPU
tests require a usable adapter; display-link tests require an awake display.
`cargo xtask golden` compares deterministic GPU scenes with their references.
The isolated `fuzz/` workspace contains parser robustness targets; it is not part
of shipped libraries or WebAssembly. Do not commit recordings containing private
terminal input or output.

## Thanks

Puzzle stands on the shoulders of the terminal community. Heartfelt thanks to the
authors of the terminal emulators that came before, and to every contributor who
helped build, document and refine them. Their open specifications, careful design
and years of shared experience made a compatible terminal engine possible.

- **xterm**: Thomas E. Dickey
- **kitty**: Kovid Goyal
- **Ghostty**: Mitchell Hashimoto
- **iTerm2**: George Nachman
- **Contour**: Christian Parpart
- **WezTerm**: Wez Furlong
- **Alacritty**: Joe Wilm and Christian Duerr
- **rxvt-unicode**: Marc Lehmann
- **vaxis** (in-band resize proposal): Tim Culverhouse

## License and credits

Puzzle is [MIT licensed](LICENSE). Slug reference shaders, bundled fonts, colour
palettes and dependencies retain their respective licences; see
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) and
[dependency notes](docs/dependencies.md).
