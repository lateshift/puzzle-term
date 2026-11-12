# iTerm2 notifications

Reference: [iTerm2 escape codes](https://iterm2.com/documentation-escape-codes.html).

OSC 9 body becomes a notification with an empty title and at most 1,024
printable Unicode scalars. `9;4` routes to the ConEmu family instead.
Session owns the shared one-per-second policy and host delivery.
Code: `puzzle-vt/src/protocols/iterm2.rs`.

Synchronized-output design began with iTerm2's DCS proposal; Puzzle implements
Contour's mode-2026 form, documented under Contour. iTerm2 OSC 1337 images and
the original DCS synchronized-output spelling are unsupported.
