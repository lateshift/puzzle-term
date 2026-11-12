# VTE hyperlinks

Reference: [Egmont Koblinger's OSC 8 proposal](https://gist.github.com/egmontkob/eb114294efbcd5adb1944c9f3cb5feda).

OSC 8 opens/closes hyperlink spans; `id=` groups disconnected spans. URI
≤2 KiB, table ≤4,096 with generation-safe recency eviction. Scheme and
modifier-click policy remain in Session; VT never opens a URL.
Code: `puzzle-vt/src/protocols/vte.rs`; link storage stays shared.
