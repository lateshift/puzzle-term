# Apple Terminal working-directory hint

Reference: Apple’s installed `/etc/zshrc_Apple_Terminal` (`file:///etc/zshrc_Apple_Terminal`, lines 6–38).

OSC 7 accepts only `file://` URIs, at most 2,048 printable scalars, stores the
hint and emits CwdChanged without resolving a filesystem path.
Code: `puzzle-vt/src/protocols/apple.rs`.

Attribution is based on Apple's installed `/etc/zshrc_Apple_Terminal`, whose
`update_terminal_cwd` emits OSC 7 with a percent-encoded file URI (lines 6–38
on the development Mac). An independently dated first definition was not found;
this primary implementation is the recorded
basis, rather than attributing it to a later implementation.
