# M18 protocol review (approved 2026-10-13)

Run each check in a freshly rebuilt Rust harness and in the rebuilt macOS app.
The macOS clipboard, notifications and drag flows were user-confirmed on
2026-10-12 (apps/macos/README.md). The user approved M18 in both hosts on
2026-10-13, including text sizing/copy, fresh-zsh automatic integration without
duplicate marks, contexts, selection colours, Shift capture and all ten
reference images. Reviewer comments are approved at 292e4be. These scripts
remain regression/reproduction steps after acceptance. Do not enable general clipboard reads
for the default-denial check.

## MIME clipboard writes (OSC 5522)

This Python snippet preserves the tty state, consumes the acknowledgement and
writes a neutral plain-text clipboard item. Paste into a normal editor and
expect `Puzzle MIME clipboard`. The reply must contain `status=DONE`.

```sh
python3 - <<'PY'
import os, select, termios, tty
fd = os.open('/dev/tty', os.O_RDWR)
saved = termios.tcgetattr(fd)
try:
    tty.setraw(fd)
    os.write(fd, b'\x1b]5522;type=write:id=review\x1b\\'
                 b'\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;UHV6emxlIE1JTUUgY2xpcGJvYXJk\x1b\\'
                 b'\x1b]5522;type=wdata\x1b\\')
    reply = bytearray()
    while b'status=DONE:id=review\x1b\\' not in reply:
        if not select.select([fd], [], [], 10)[0]:
            raise RuntimeError('clipboard host did not complete')
        reply.extend(os.read(fd, 4096))
finally:
    termios.tcsetattr(fd, termios.TCSANOW, saved)
    os.close(fd)
print('clipboard write acknowledged')
PY
```

For an image, this creates a neutral red/blue 2×2 PNG in memory and sends it
through the same acknowledged write path. Paste into an image-capable native
app and verify both colours. Unsupported MIME combinations must return ENOSYS
and leave the previous clipboard intact.

```sh
python3 - <<'IMAGE'
import base64, os, select, struct, termios, tty, zlib

def chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))
png = (b'\x89PNG\r\n\x1a\n'
       + chunk(b'IHDR', struct.pack('>IIBBBBB', 2, 2, 8, 2, 0, 0, 0))
       + chunk(b'IDAT', zlib.compress(b'\0\xff\0\0\0\0\xff' * 2))
       + chunk(b'IEND', b''))
fd = os.open('/dev/tty', os.O_RDWR)
saved = termios.tcgetattr(fd)
try:
    tty.setraw(fd)
    os.write(fd, b'\x1b]5522;type=write:id=image\x1b\\')
    for offset in range(0, len(png), 4095):
        os.write(fd, b'\x1b]5522;type=wdata:mime=aW1hZ2UvcG5n;'
                 + base64.b64encode(png[offset:offset+4095]) + b'\x1b\\')
    os.write(fd, b'\x1b]5522;type=wdata\x1b\\')
    reply = bytearray()
    while b'status=DONE:id=image\x1b\\' not in reply:
        if not select.select([fd], [], [], 10)[0]:
            raise RuntimeError('image write failed or host did not complete')
        reply.extend(os.read(fd, 4096))
finally:
    termios.tcsetattr(fd, termios.TCSANOW, saved)
    os.close(fd)
print('PNG write acknowledged')
IMAGE
```

## Paste-event grants

Use a terminal client enabling `CSI ? 5522 h`, then perform an actual paste
shortcut. Expect an OSC 5522 MIME listing, not ordinary text/bracket markers.
The client can redeem its supplied password once for the same clipboard
location/revision. A second read, a read after input/reset/another paste, or a
read after changing and restoring the clipboard must be denied. Keep passwords
out of screenshots and logs. Disable mode 5522 when finished.

Automated wire, security-expiry, PNG and C-host witnesses are recorded in
`docs/history/M18.md`. Native clipboard confirmation was recorded 2026-10-12; final M18 user approval
was recorded 2026-10-13.

<a id="m184-notifications-pending"></a>

## M18.4 notifications (approved 2026-10-13)

In the harness (`cargo xtask run`), run the following commands at least one
second apart. Expect a toast with title/body, a red urgency accent, and a ×.
Click its body to focus its Session; repeat then click × to dismiss. Use
`printf '\033]99;i=review:p=close;\033\\'` to remove it without a rate-limit delay.

```sh
printf '\033]99;i=review:u=2:a=report:c=1:d=0;Build finished\033\\'
printf '\033]99;i=review:p=body;Click to activate; x dismisses\033\\'
```

For exact replies without a shell line editor consuming them, run this in either
host, then activate or dismiss within 30 seconds (the script restores tty mode):

```sh
python3 - <<'PY'
import os, select, sys, termios, tty
fd = os.open("/dev/tty", os.O_RDWR)
old = termios.tcgetattr(fd)
try:
    tty.setraw(fd)
    os.write(sys.stdout.fileno(), b'\x1b]99;i=review:a=report:c=1;Click or dismiss\x1b\\')
    if select.select([fd], [], [], 30)[0]:
        data = os.read(fd, 4096)
    else:
        data = b''
finally:
    termios.tcsetattr(fd, termios.TCSADRAIN, old)
print(repr(data))
PY
```

Activation reports `OSC 99;i=review;ST` followed by `OSC 99;i=review:p=close;ST`;
dismissal reports only close. Query `printf '\033]99;i=query:p=?;\033\\'` before
DA1 in a raw reader to inspect the supported subset. OSC 9 and 777 should use the
same harness toast. A burst admits at most one per second. In the macOS app,
session 2 implements the same callbacks with Notification Center; check posting,
activation/focus, dismissal and explicit removal after its host work lands.
Screenshot candidate: `target/harness/notification-toasts/001-notifications-2x.png`.
Native-host approval was recorded 2026-10-12; the final M18 approval was
recorded 2026-10-13.


<a id="m185-drag-and-drop-pending"></a>

## M18.5 drag and drop (approved 2026-10-13)

Mechanical full-flow checks (synthetic MIME offers and both real C attachment
kinds) are independent of winit's file-only OS interface:

```sh
cargo xtask accept M18 --task dnd
cargo xtask harness assets/scenarios/drag-drop.pzs --check
```

In each host, run the following neutral receiver. It opens no dropped file;
it displays the offered names, position and captured URI/plain-text bytes.
Session 2 must first implement the native callbacks listed in README.md.

```sh
mkdir -p target/m18
cat > target/m18/drop-review.py <<'PY'
import os, re, termios, tty, base64
fd = os.open('/dev/tty', os.O_RDWR)
saved = termios.tcgetattr(fd)
packet = re.compile(rb'\x1b\]72;([^;]*);(.*?)(?:\x1b\\|\x07)', re.S)
def send(meta, data=b''):
    os.write(fd, b'\x1b]72;' + meta + b';' + data + b'\x1b\\')
def show(data):
    os.write(fd, data + b'\r\n')
buffer, captured = b'', bytearray()
try:
    tty.setraw(fd)
    send(b't=a:i=7', b'text/uri-list text/plain')
    show(b'READY: drag a neutral file; Ctrl+C quits')
    while True:
        chunk = os.read(fd, 8192)
        if b'\x03' in chunk:
            break
        buffer += chunk
        while (match := packet.search(buffer)) is not None:
            meta, data = match.groups()
            buffer = buffer[match.end():]
            fields = dict(pair.split(b'=', 1) for pair in meta.split(b':') if b'=' in pair)
            kind = fields.get(b't')
            if kind == b'm':
                show(b'OFFER/LEAVE ' + meta + b' ' + data)
                if fields.get(b'x') != b'-1':
                    send(b't=m:o=1', b'text/uri-list text/plain')
            elif kind == b'M':
                show(b'DROP ' + meta + b' ' + data)
                captured.clear()
                send(b't=r:x=1')
            elif kind == b'r':
                if data:
                    captured.extend(base64.b64decode(data, validate=True))
                else:
                    show(b'DATA ' + repr(bytes(captured)).encode())
                    send(b't=r:o=1')
            elif kind == b'R':
                show(b'ERROR ' + meta + b' ' + data)
finally:
    send(b't=A')
    termios.tcsetattr(fd, termios.TCSANOW, saved)
    os.close(fd)
PY
python3 target/m18/drop-review.py
```

Check enter, movement, leave without drop, and a completed drop. Repeat with two
panes: only the creating pane receives data. To test rejection, replace the
receiver's `t=m:o=1` with `t=m:o=0`: a registered rejected drop must produce no
DATA and must never paste a path. After Ctrl+C unregisters, the host's ordinary
shell-quoted paste-path fallback applies. RIS and child exit also unregister.

The harness offers only file names as text/uri-list and text/plain, uses its
latest pointer point, and cannot change the OS acceptance cursor or observe
native drag-position updates through winit. Those are documented harness
limitations. The product C seam supports positioned enter/move, ordered MIME
preferences, copy/move/reject, actual drop bytes and leave. The user confirmed
the app flow on 2026-10-12, including two-pane isolation, rejection and fallback;
see apps/macos/README.md. Final M18 user approval was recorded 2026-10-13; reviewer comments are approved at 292e4be.


<a id="m186-contexts-pending"></a>

## M18.6 contexts (approved 2026-10-13)

There is no new core visual UI. Review the origin parser/state and the host
fact contract in [uapi.md](protocols/uapi.md); run
`cargo xtask accept M18 --task context` for hierarchy, bounds, cancellation,
RIS and session lifetime, wire replay, and callbacks through both C hosts.

In each host the following must leave ordinary output intact. A host observer
of `terminal/contextsChanged` sees remote as a child of shell, then shell alone;
RIS between the starts and end must not end either context. The core does not
add a breadcrumb display; that host feature belongs to M12.

```sh
printf '\033]3008;start=review-shell;type=shell\033\\'
printf '\033]3008;start=review-remote;type=remote;targethost=example\033\\'
printf '\033c'
printf 'context reset survived\n'
printf '\033]3008;end=review-remote\033\\'
printf '\033]3008;end=review-shell\033\\'
```

Closing/ending the session clears the final path. This metadata is descriptive,
never authenticated privilege or permission for a host action.

<a id="m187-text-sizing-pending"></a>

## M18.7 text sizing (approved 2026-10-13)

Run in both the rebuilt harness and the rebuilt macOS app. The following uses
an alternate screen so the shell's prompt redraw does not overwrite the scene.
Press Return to return to the shell. Observe headings at integer scales 2/3,
fractional ink aligned within its reserved cells, and ordinary text beside a
scaled run. Select `Heading` across its lower fragments and copy: the clipboard
must contain the text once, with no continuation spaces or duplicate lines.

```sh
printf '\033[?1049h\033[2J\033[H\033[?25l'
printf '\033]66;s=2;Heading\033\\'
printf '\033[4;1H\033]66;s=3;ABC\033\\'
printf '\033[8;1H\033]66;s=2:n=1:d=2:h=2:v=2;Fraction\033\\'
printf '\033[11;1Hplain \033]66;s=2:w=2;mix\033\\ BC'
read -r _puzzle_review_done
printf '\033[?25h\033[?1049l'
```

For exact 8→5→8-column reflow in each live host, temporarily increase font size
(e.g. 160 points, using the existing configuration/zoom controls) so that five
columns fit above the host's minimum window width. First run `stty size` and
resize until its column value is 8. Then run this scene, shrink to 5 columns and
grow to 8 while it waits for Return. `A` stays one two-row rectangle; `B` and `C`
remain ordinary text in logical order without duplication. Restore the font
size afterward. Hosts retain their minimum-size policy; no window-resize escape
sequence is introduced.

```sh
python3 - <<'REFLOW'
import os, termios, tty
fd = os.open('/dev/tty', os.O_RDWR)
saved = termios.tcgetattr(fd)
try:
    tty.setraw(fd)
    os.write(fd, b'\x1b[2J\x1b[H\x1b]66;s=2;A\x1b\\BC')
    os.read(fd, 1)  # Resize 8→5→8, then press a key; no prompt redraw in between.
finally:
    termios.tcsetattr(fd, termios.TCSANOW, saved)
    os.close(fd)
print()
REFLOW
```

This primary-screen test exercises reflow; the alternate screen deliberately
keeps its existing no-reflow rule (§6.6). The deterministic harness scene
`assets/scenarios/text-sizing.pzs` supplies the same checkpoints at both DPRs:
`cargo xtask harness assets/scenarios/text-sizing.pzs --check`.
The eight images in `assets/scenarios/text-sizing/` are user-approved golden
references (2026-10-13); mechanical ink/clipping/cold equality remains separate.


<a id="m188-automatic-shell-integration-pending"></a>

## M18.8 automatic shell integration (approved 2026-10-13)

In both rebuilt native hosts, open a fresh zsh/fish session with the default
`shellIntegration=detect`, without adding manual sourcing. Your ordinary startup
output and prompt must remain intact. Run `printf 'hello\n'`, `false`, then
`printf 'first\nsecond\n'`; check command blocks, failure status, prompt
navigation and Copy output (only the command output, never the prompt). Change
directory to one containing spaces/Unicode and confirm the cwd fact/host title
tracks it. Repeat with an existing manual source line: marks must not duplicate.

Repeat with an installed ENV-capable GNU bash if available. Apple /bin/bash
3.2 intentionally retains manual integration and true login behavior; use the
[manual snippet](shell-integration.md). With `shellIntegration=none`, open a
new session and confirm there is no automatic integration. Unknown configured
shell names get nothing; existing manual integrations still work. No host Swift
change is required: both hosts use the same native PTY constructor.
