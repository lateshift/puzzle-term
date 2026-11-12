# Tab and pane status for embedders

Puzzle supplies semantic facts; the host owns tabs, splits, labels and status
presentation. The same facts reach Rust as `HostEvent::Fact` and C as named
JSON events delivered only by `puzzle_app_tick` on the creating thread. Worker
threads never call native UI. Register the host before starting a surface;
closing it discards late events. There is no process polling or OSC parsing in
the host.

`shell/commandStarted` is `{ "command": "cargo build" }`. Puzzle's automatic
shell integration reports the accepted command before output starts. The
optional command is UTF-8 at most **1 KiB including the ellipsis** when truncated;
controls become spaces. A bare/unsupported or malformed metadata field produces
`{ "command": null }`, while still reporting the start. Older null recordings
remain readable. Use the OSC title when no command label is available; metadata
is untrusted plain display text, never a command to execute. Commands may contain
private arguments: normal logging does not log their content.

| Event | Payload | Typical host use |
|---|---|---|
| `shell/promptStarted` | null | Return to the idle shell label |
| `shell/commandStarted` | `{command: string or null}` | Running-command label; host chooses abbreviation |
| `shell/commandFinished` | `{exit: integer or null, duration: null}` | Success/error state; missing status is not success |
| `terminal/cwdChanged` | file URI string | Location label; decode as a URI, never shell code |
| `terminal/progress` | `{state: 0..4, percent: 0..100}` | Progress indicator; preserve the state independently of percent |
| `terminal/bell` | null | Background-tab attention |
| `terminal/titleChanged` | string | Fallback program label |
| `session/exited` | `{code, signal, runtimeMs}` | Exited pane state |
| `surface/colors` | `{background, foreground, dark}` | Match host chrome to the active pane |
Shell metadata uses kitty's existing `OSC 133;C;cmdline_url=...` extension.
Puzzle snippets percent-escape bytes so semicolons, percent signs and controls
cannot change OSC framing. Modern fish supplies this metadata itself; Puzzle
retains its native marks. Bash uses a newly recorded history entry for the whole
accepted list/pipeline and falls back to `BASH_COMMAND` if history policy hides
it, so that fallback can name only its first simple command. Zsh/fish preexec
receive the accepted command. Apple bash 3.2 still requires manual sourcing,
as documented in [shell integration](../shell-integration.md).

Reference checked 2026-10-20: [kitty shell-integration command metadata](https://sw.kovidgoyal.net/kitty/shell-integration/#notes-for-shell-developers).

## Output activity

`surface/activity` has a JSON `null` payload (`HostEvent::Activity` in Rust).
Each surface starts seen. Its first accepted program output queues one event;
further chunks queue no activity until the host calls
`puzzle_surface_activity_seen(surface)` / `Surface::activity_seen()`. There is
no quiet timer and no per-chunk or per-frame notification without another
acknowledgement. Escape sequences count as output too; input, resize, repaint
and empty continuation turns do not.

Call `activity_seen` when showing a previously hidden pane, and acknowledge an
activity callback while the pane remains visible. Clear the host's unread
indicator at that time. Leave a hidden pane unacknowledged to retain its unread
indicator. Focus alone does not define visibility: another split can be visible
while unfocused. Receiving an event does not acknowledge it automatically.

The call queues a control message; it never waits for Session or rendering. The
owner applies it between slices, before subsequently admitted output. It affects
only that surface and emits no synthetic activity or reset event. As with other
host facts, tick delivers the event on the creating thread; closing a surface
discards late callbacks.

For tear-off previews and pane overviews use the portable, caller-owned
[last-presented snapshot](snapshot.md). It needs no new frame or worker query.

