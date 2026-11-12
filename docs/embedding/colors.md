# Match host UI to the terminal

Each surface emits `surface/colors` through the ordinary tick callback:

```json
{"background":[14,18,26],"foreground":[229,233,240],"dark":true}
```

The arrays are encoded sRGB bytes (0–255), not linear-light values or display-P3
components. They describe effective default paper and ink, including theme and
appearance changes, OSC 10/11 overrides and their resets. Indexed cell colours,
selection colours and the current cell's SGR colours do not change these defaults.
Theme transitions report their sampled defaults; OSC overrides take precedence.
`dark` is derived from the background's relative luminance: true when white has
at least as much WCAG contrast against it as black. A theme's name/appearance
label does not override this test.

Use the event to match per-pane native find controls, title bars or other host
chrome to the terminal. It is a value
notification, not a request to change the terminal theme. No synchronous Session
query or host-thread colour lookup is needed.

The initial effective value is delivered after surface creation on the first
`puzzle_app_tick` / `App::tick` that drains owner work, then only when those values
change. The callback runs on the creating thread, like other surface events;
never update native UI from the worker-safe wakeup callback. Rust receives
`AppEvent::Host(HostEvent::Colors { background, foreground, dark })`. Keep the
latest value per surface, and discard it when that surface closes. Queued events
for a closed surface are suppressed. A view/window move preserves its colours.
