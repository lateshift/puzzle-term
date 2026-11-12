# Coverage-guided fuzzing — M19.2b

This unpublished workspace is separate from the product workspace. Use the
pinned nightly-2026-08-14, the user-approved cargo-fuzz 0.13.2, ASan and debug
assertions. No fuzz dependency is linked into a shipped native or wasm build.
The ordinary workspace check validates source/manifest/CLI rules but never
launches the fuzz engine. All builds use the workspace target directory.

These one-minute commands are the recommended initial campaigns, to be run
and evaluated by the reviewer. The agent has run fixed seeds only (`-runs=0`);
timed campaigns and finding evaluation belong to the reviewer under the
user's 2026-10-13 instruction.

The updated division of work is binding: implementation sessions maintain the
target source but never run `cargo xtask fuzz` or open `target/fuzz/`,
`target/m19-fuzz-runs/` or `fuzz/artifacts/`. The reviewer runs campaigns and
provides findings as plain operation sequences for ordinary regression tests.
The fixed-seed receipts below predate this updated process boundary.

The driver captures all compiler/libFuzzer output in
`target/m19-fuzz-runs/<target>-<unix-nanoseconds>-<pid>.log`. It prints only the
target, requested duration, status and paths; never raw logs or crash inputs.
Coverage, execution totals and final statistics are in those files.
The driver copies missing seeds to `target/fuzz/corpus/<target>`, resumes the
existing corpus and retains every crash under `target/fuzz/artifacts/<target>`.
It never retries or minimizes a failure. Preserve the artifact, add an ordinary
regression test first, then fix the cause. The requested duration is also the
per-input hang guard, not a parser speed budget. Input length reaches §6.4's
64-KiB outer OSC cap. A quiet machine is required for recorded campaign runs.

Fuzz-only settings: Session targets start at 16×8 cells; structured sizing
starts at 12×4 and resizes within 2–80 columns / 1–24 rows. All owners retain
at most 64 history lines / 64 KiB; graphics admission is 4 KiB. These explicit
fixture limits exercise eviction quickly; production defaults are unchanged.
Structured operations stop when the input bytes are exhausted (Arbitrary's
primitive readers can otherwise zero-fill forever). The debug consistency
oracle builds a row-identity index once per check instead of scanning history
once per block. Ordinary product line lookups retain their verified fast path.

Fixed-seed preflight receipts: `target/m19-fuzz-preflight/seed-summary.json`
and `seeds-<target>.log` in the same directory. Each engine replay finishes in
less than one second, with no findings; this is not a timed fuzz campaign.

| Target | Production path and oracle | Seed origin |
|---|---|---|
| vt_stream | Session/VT with every current optional handler (Rex) registered, images enabled; arbitrary split reads; both-screen OSC 66 consistency/cursor/history/storage checks | M18 protocol tests: text-sizing, contexts, colours, clipboard MIME, notifications, dnd; M17 Rex tests; ordinary Unicode/scroll/clear cases |
| text_sizing | Structured all-six-key OSC 66, ordinary printing, ICH/DCH/ECH/EL/ED/IL/DL, margins, scrolling, resize/reflow, alternate screen and RIS; checker after every operation | Existing M18.7 random-operation test families; compact valid rectangle/fractional vectors |
| kitty_graphics | Real APC extractor/Session decode barrier/chunk accumulation/admission/placements; exact raw RGB/RGBA completion, PNG/zlib error completion; decoded-byte and source-size bounds | M15.2 direct/chunked RGBA fixtures; M15.6 virtual placement fixture |
| bridge | Exact I/O-free codec shared by puzzle-term and puzzle-pty-bridge; both directions round-trip, malformed input returns clean-close DecodeError | M7 hello/resize/input and malformed tag fixtures |
Approved, separately locked dependencies: libfuzzer-sys =0.4.13 (only explicit
link_libfuzzer), arbitrary =1.4.2 (defaults off, no derive proc macro). The
libfuzzer-sys package licence is `(MIT OR Apache-2.0) AND NCSA`; arbitrary and
the cargo-fuzz tool are MIT OR Apache-2.0. The separate resolved graph is in
Cargo.lock; raw licence report: `target/m19-fuzz-dependencies.txt`.

Primary sources checked 2026-10-13:
[cargo-fuzz 0.13.2 options](https://github.com/rust-fuzz/cargo-fuzz/blob/0.13.2/src/options.rs),
[Rust Fuzz Book](https://rust-fuzz.github.io/book/cargo-fuzz/tutorial.html),
[libfuzzer-sys 0.4.13 package](https://docs.rs/crate/libfuzzer-sys/0.4.13),
[arbitrary 1.4.2 package](https://docs.rs/crate/arbitrary/1.4.2).
The installed manifests confirm the licences and selected link feature.
Campaign duration/coverage/findings are recorded in docs/history/M19.md.

