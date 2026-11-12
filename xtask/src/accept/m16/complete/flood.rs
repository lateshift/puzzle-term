//! Reusable-glyph Flood runs on the ordinary native render actor, not a headless loop.
//!
//! The checked corpus, changing truecolor screen and authored DOOM-style fire
//! run independently through a real PTY. No producer pacing caps their rate.
//! GPU readback overhead and every failed frame attempt remain in the interval.
//!
//! The native observer starts its clock before releasing the child's ready handshake.
//! Startup, cold primary glyphs and the first GPU sample therefore remain charged.
//! A Python reader timestamps child input independently of the output loop.
//! The native key timer posts recorded callbacks through the actual UI adapter.
//! Ctrl+D is one ordinary byte in raw mode, so terminal line editing adds no wait.
//! The child writes that timestamp to the existing probe-owned receipt file.
//! A missing file or reversed clock pair fails instead of becoming zero latency.
//! The ordinary six-second observer owns the process and closes only its window.
//! No personal shell startup or terminal configuration enters these fixtures.
//! Truecolor paints every cell with a changing foreground and a familiar ASCII X.
//! Fire uses one block glyph with a bounded color palette and a reusable heat grid.
//! Each fire step propagates heat upward with a small deterministic lateral decay.
//! A fixed seed makes that authored animation reproducible without imported assets.
//! The corpus path uses the existing checked-in output bytes exactly as supplied.
//! That corpus can paint a black foreground on identical black paper deliberately.
//! GPU contrast observations remain raw evidence; Flood does not require contrast.
//! Unicode stress separately enforces covered-glyph ink in every sampled cell.
//! All producers write continuously: no sleep or desired frame rate masks backpressure.
//! Their actual frames are chosen by the same D-33/D-35 FIFO render scheduler.
//! The fixed PTY pool supplies the existing four-MiB pending-output hard bound.
//! Missing numeric rate or input evidence fails before comparing the unchanged floor.
//! Every workload is measured even after failure, and all raw reports remain on disk.
use std::fs;

/// Reports every workload even if an earlier continuous producer failed.
pub(super) fn run() -> Result<(), String> {
    let root = std::env::current_dir().map_err(|error| error.to_string())?;
    let path = root.join("target/m16-2/flood-producer.py");
    fs::write(
        &path,
        PRODUCER.replace("ROOT", &serde_json::json!(root).to_string()),
    )
    .map_err(|error| error.to_string())?;
    let mut failures = Vec::new();
    for name in ["truecolor", "doom-fire", "flood-corpus"] {
        // The same six-second observer includes cold glyphs, GPU samples and child receipt.
        let result =
            super::window(&path.to_string_lossy(), Some(name), false, name).and_then(|value| {
                // Colored blocks may deliberately match their paper; Unicode ink tests do not
                // define Flood's rate/input criterion. Preserve those GPU observations anyway.
                if value["fps"].as_f64().is_none_or(|fps| fps < 30.0)
                    || value["key_to_child_ms"].as_f64().is_none_or(|ms| ms > 50.0)
                    || value["aborted_builds"] != 0
                {
                    return Err(format!(
                        "{name}: fps={} key_ms={} aborted={}",
                        value["fps"], value["key_to_child_ms"], value["aborted_builds"]
                    ));
                }
                println!(
                    "M16 Flood {name}: fps={} key_ms={} aborted=0",
                    value["fps"], value["key_to_child_ms"]
                );
                Ok(())
            });
        if let Err(error) = result {
            failures.push(error);
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

/// Local neutral output retains a reusable glyph set while every row changes.
const PRODUCER: &str = r#"import os, sys, time, threading, random
from pathlib import Path
os.write(1,b'\x1b]2;unicode-ready\x07')
sys.stdin.buffer.read(1)
def reader():
    while sys.stdin.buffer.read(1):
        Path(sys.argv[1]).write_text(str(time.time_ns()))
threading.Thread(target=reader,daemon=True).start()
name = os.environ['PUZZLE_WORKLOAD']
os.write(1,b'\x1b[?25l')
if name == 'flood-corpus':
    corpus = Path(ROOT,'assets/fixtures/corpus/flood.vt').read_bytes()
    while True: os.write(1,corpus)
elif name == 'truecolor':
    frame = 0
    while True:
        rows = ['\x1b[H']
        for y in range(40):
            rows.append(''.join(f'\x1b[38;2;{(x*3+frame)%256};{(y*6+frame)%256};255mX' for x in range(80)))
            rows.append('\r\n' if y < 39 else '')
        os.write(1,''.join(rows).encode())
        frame += 1
else:
    # A finite palette spreads heat upward; glyph identity remains one solid block.
    rng = random.Random(16)
    heat = [0]*3200
    palette = [f'\x1b[38;2;{min(255,i*12)};{max(0,min(255,(i-12)*16))};0m█'.encode() for i in range(37)]
    while True:
        heat[-80:] = [36]*80
        for y in range(39):
            for x in range(80):
                decay = rng.randrange(3)
                heat[y*80+(x-decay+1)%80] = max(1,heat[(y+1)*80+x]-decay)
        os.write(1,b'\x1b[H'+b'\r\n'.join(b''.join(palette[i] for i in heat[y*80:(y+1)*80]) for y in range(40)))
"#;
