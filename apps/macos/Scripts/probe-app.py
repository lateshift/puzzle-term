#!/usr/bin/env python3
"""Measure vtebench through the real app/login PTY; never launch the Rust harness."""
import argparse
import hashlib
import json
import math
import pathlib
import shlex
import subprocess
import time


def prepare(root, benchmark_root, benchmark, seconds, frame):
    """Wrap only corpus loading to record its byte size, preserving timed stdout bytes."""
    root.mkdir(parents=True, exist_ok=False)
    fixture = root / "corpus" / benchmark
    fixture.mkdir(parents=True)
    original = benchmark_root / benchmark
    # vtebench loads this script once outside its timing loop, in the actual terminal.
    # Capture that generated payload to count bytes rather than guessing a 1-MiB size.
    script = fixture / "benchmark"
    quote = lambda value: shlex.quote(str(value))
    # Darwin ps pads tty names with trailing whitespace. The external checkout's
    # unquoted redirection treats that padding as part of /dev/ttysNNN and generates
    # only 26 cursor-home escapes. Trim that observation in this temporary fixture;
    # preserve all corpus generation, tput geometry and benchmark timing unchanged.
    source = (original / "benchmark").read_text().replace(
        "ps -o tty= -p $$", "ps -o tty= -p $$ | tr -d '[:space:]'"
    )
    script.write_text(
        "#!/bin/sh\n/bin/sh -c " + quote(source) + " " + quote(original / "benchmark")
        + " > " + quote(root / "payload") + " 2> " + quote(root / "loader.stderr") + "\n"
        "/usr/bin/wc -c < " + quote(root / "payload") + " > " + quote(root / "bytes") + "\n"
        "/usr/bin/touch " + quote(root / "loaded") + "\n"
        "/bin/cat " + quote(root / "payload") + "\n"
    )
    script.chmod(0o755)
    if (original / "setup").exists():
        (fixture / "setup").symlink_to(original / "setup")
    # Only completion and DAT samples go to files; flood output still traverses the PTY/GPU.
    command = (
        "/bin/stty size > " + quote(root / "pty-grid") + "; /usr/local/bin/vtebench -b " + quote(fixture) + " --max-secs " + str(seconds)
        + " --dat " + quote(root / "samples.dat")
        + "; /bin/stty size > " + quote(root / "pty-grid-final")
        + "; /usr/bin/touch " + quote(root / "done")
    )
    manifest = {"directory": str(root), "command": command, "frame": frame}
    path = root / "manifest.json"
    path.write_text(json.dumps(manifest))
    return path


def summarize(root):
    """Normalize original integer-ms vtebench samples by their actual expanded byte count."""
    report = json.loads((root / "app.json").read_text())
    # Keep failed receipts on disk, but never silently compare a moved or incomplete window.
    if not report["completed"] or not report.get("stableGeometry") or len(report.get("grid", [])) != 2:
        raise ValueError("Incomplete run or unstable/missing terminal geometry: " + str(root))
    # Cross-check the runtime observation against the child PTY before and after
    # the flood. A stable host cache alone would not prove the child used that grid.
    grid = [int(value) for value in (root / "pty-grid").read_text().split()][::-1]
    final_grid = [int(value) for value in (root / "pty-grid-final").read_text().split()][::-1]
    if grid != report["grid"] or final_grid != grid:
        raise ValueError("Host/PTY grid mismatch or resize during benchmark: " + str(root))
    raw_bytes = int((root / "bytes").read_text())
    expanded = raw_bytes * math.ceil(1048576 / raw_bytes)
    samples = [float(line.strip()) for line in (root / "samples.dat").read_text().splitlines()[1:] if line.strip()]
    seconds = report["elapsed"]
    report.update({
        "corpusBytes": raw_bytes, "sampleBytes": expanded, "samples": len(samples),
        "ptyGrid": grid, "finalPtyGrid": final_grid,
        "meanMsPerMiB": sum(samples) / len(samples) * 1048576 / expanded,
        "wakeupsPerSecond": report["wakeups"] / seconds,
        "tickBlocksPerSecond": report["executedTickBlocks"] / seconds,
        "ticksPerSecond": report["ticks"] / seconds,
        "hostBlocksPerSecond": report["hostBlocks"] / seconds,
        "eventsPerTick": {name: count / max(1, report["ticks"]) for name, count in report["events"].items()},
    })
    (root / "summary.json").write_text(json.dumps(report, indent=2, sort_keys=True))
    return report


def prepare_comparison(arguments):
    """Write an A/B/A/B plan only; starting either GPU host requires a later explicit run."""
    if not arguments.prepare_only or arguments.harness is None or arguments.display is None:
        raise ValueError("--plan-abab requires --prepare-only, --harness and --display")
    rows = []
    binaries = {}
    for name, path in [("app", arguments.app), ("harness", arguments.harness)]:
        with path.open("rb") as binary:
            binaries[name] = {"path": str(path.resolve()), "sha256": hashlib.file_digest(binary, "sha256").hexdigest()}
    for benchmark in arguments.benchmarks:
        for label, host in [("A1", "harness"), ("B1", "app"), ("A2", "harness"), ("B2", "app")]:
            root = arguments.output.resolve() / benchmark / label
            manifest = prepare(root, arguments.source.resolve(), benchmark, arguments.seconds, arguments.frame)
            command = json.loads(manifest.read_text())["command"]
            # Harness runs use their own winit lifecycle API; the serial runner applies
            # and reads back the native frame before sending text through the product API.
            rows.append({"benchmark": benchmark, "cohort": label, "host": host,
                         "manifest": str(manifest), "shellCommand": command,
                         "gridReceipt": str(root / "pty-grid"), "frame": arguments.frame,
                         "display": arguments.display, "fontSize": 15})
    plan = arguments.output.resolve() / "plan.json"
    plan.write_text(json.dumps({"binaries": binaries, "runs": rows}, indent=2))
    print(str(plan), flush=True)


def main():
    """Run sequentially so CPU/GPU workloads never overlap; retain every raw result."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", required=True, type=pathlib.Path)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    parser.add_argument("--benchmarks", nargs="+", default=["dense_cells", "unicode"])
    parser.add_argument("--source", type=pathlib.Path, default=pathlib.Path("/home/puzzle/Developer/rust/vtebench/benchmarks"))
    parser.add_argument("--seconds", type=int, default=10)
    parser.add_argument("--frame", nargs=4, type=float, required=True, help="Confirmed AppKit frame: x y width height in points")
    parser.add_argument("--prepare-only", action="store_true", help="Write fixtures and manifests without launching any GPU workload")
    parser.add_argument("--plan-abab", action="store_true", help="Prepare an interleaved harness/app comparison; never launch it")
    parser.add_argument("--harness", type=pathlib.Path, help="Normal Release harness executable, for the comparison plan")
    parser.add_argument("--display", type=int, help="Confirmed CGDirectDisplayID for the comparison plan")
    parser.add_argument("--xcode", action="store_true", help="Prepare one manifest; launch it using Xcode Run")
    arguments = parser.parse_args()
    if arguments.plan_abab:
        prepare_comparison(arguments)
        return
    for benchmark in arguments.benchmarks:
        root = arguments.output.resolve() / benchmark
        manifest = prepare(root, arguments.source.resolve(), benchmark, arguments.seconds, arguments.frame)
        if arguments.prepare_only:
            print(str(manifest), flush=True)
            continue
        if arguments.xcode:
            # Xcode's transient scheme argument uses this stable path; its selected Debug
            # diagnostics/debugger remain exactly those of Xcode Run, not a CLI substitute.
            pathlib.Path("/tmp/PuzzleShell-active-probe.json").write_text(manifest.read_text())
            print("READY: Xcode Run --puzzle-probe /tmp/PuzzleShell-active-probe.json", flush=True)
            deadline = time.monotonic() + 120
            while not (root / "app.json").exists() and time.monotonic() < deadline:
                time.sleep(0.1)
        else:
            with (root / "process.log").open("w") as output:
                subprocess.run([str(arguments.app.resolve()), "--puzzle-probe", str(manifest)],
                               stdout=output, stderr=subprocess.STDOUT, timeout=100, check=True)
        report = summarize(root)
        print(json.dumps({"benchmark": benchmark, **report}), flush=True)


if __name__ == "__main__":
    main()
