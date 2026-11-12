#!/usr/bin/env python3
"""Execute an authored app/harness plan serially through their own diagnostic APIs."""
import argparse
import hashlib
import json
import math
import pathlib
import re
import runpy
import subprocess


def harness_receipt(root, row, primary_height):
    """Decode toolkit readbacks, independent PTY geometry, and original DAT samples."""
    log = re.sub(r"\x1b\[[0-9;]*m", "", (root / "process.log").read_text())
    moved = [line for line in log.splitlines() if 'event="desktop.host_moved"' in line]
    frames = []
    for line in moved:
        # These are Window::outer_position/outer_size observations in physical pixels,
        # not the replay's requested rectangle. Both must appear on the same line.
        match = re.search(r"position=Ok\(PhysicalPosition \{ x: (-?\d+), y: (-?\d+) \}\) outer=PhysicalSize \{ width: (\d+), height: (\d+) \}", line)
        scale = re.search(r"scale: ([0-9.]+)", line)
        display = re.search(r"display: (\d+)", line)
        if match and scale and display:
            factor = float(scale[1])
            x, y, width, height = [float(value) / factor for value in match.groups()]
            frames.append({"winitFrame": [x, y, width, height],
                           "frame": [x, primary_height - y - height, width, height],
                           "scale": factor, "display": int(display[1])})
    if not frames or frames[-1]["frame"] != row["frame"] or frames[-1]["display"] != row["display"]:
        raise ValueError("Missing or mismatched native outer-frame readback: " + str(root))
    grid = [int(value) for value in (root / "pty-grid").read_text().split()][::-1]
    final_grid = [int(value) for value in (root / "pty-grid-final").read_text().split()][::-1]
    marks = [json.loads(line.split("data=", 1)[1]) for line in log.splitlines()
             if 'event="desktop.lifecycle_mark"' in line and "data=" in line]
    if len(marks) != 2 or marks[0]["extent"] != marks[1]["extent"] or grid != final_grid:
        raise ValueError("Unstable or incomplete geometry witnesses: " + str(root))
    raw_bytes = int((root / "bytes").read_text())
    expanded = raw_bytes * math.ceil(1048576 / raw_bytes)
    samples = [float(line) for line in (root / "samples.dat").read_text().splitlines()[1:] if line.strip()]
    result = {**frames[-1], "grid": grid, "finalGrid": final_grid, "fontSize": 15,
              "corpusBytes": raw_bytes, "sampleBytes": expanded, "samples": len(samples),
              "meanMsPerMiB": sum(samples) / len(samples) * 1048576 / expanded,
              "nativeFrameReadbacks": frames, "lifecycleMarks": marks}
    (root / "summary.json").write_text(json.dumps(result, indent=2, sort_keys=True))
    return result


def run_harness(binary, row, primary_height):
    """Use only winit's existing finite lifecycle mechanism; never synthesize OS input."""
    root = pathlib.Path(row["manifest"]).parent
    x, y, width, height = row["frame"]
    # AppKit bottom-left points convert to winit desktop top-left points. The
    # primary screen height is read from an app probe, not inferred from panel pixels.
    native_frame = [x, primary_height - y - height, width, height]
    fixture = [
        {"ms": 500, "action": "magnet", "command": "maximize", "visible": native_frame},
        {"ms": 1500, "action": "mark", "label": "before"},
        {"ms": 2000, "action": "text", "text": row["shellCommand"] + "\r"},
        {"ms": 18000, "action": "mark", "label": "after"},
        {"ms": 18100, "action": "close"},
    ]
    path = root / "lifecycle.json"
    path.write_text(json.dumps(fixture, indent=2))
    # The existing geometry trace target also enables paired render-stage clocks.
    # Keep that observer cost visible in evidence; no byte recording, verification
    # renderer, pressure driver or second GPU workload is installed.
    import os
    environment = {**os.environ, "PUZZLE_LIFECYCLE_SCRIPT": str(path),
                   "PUZZLE_LOG": "warn,puzzle_desktop::resize=trace,puzzle_desktop::window::lifecycle_probe=debug,puzzle_desktop::perf=info"}
    with (root / "process.log").open("w") as output:
        subprocess.run([str(binary), "run", "--set", "font.size=15"], env=environment,
                       stdout=output, stderr=subprocess.STDOUT, timeout=60, check=True)
    if not (root / "done").exists():
        raise ValueError("Harness closed before benchmark completion: " + str(root))
    return harness_receipt(root, row, primary_height)


def main():
    """Verify binary identity, run one host at a time, and preserve every raw receipt."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--plan", required=True, type=pathlib.Path)
    parser.add_argument("--primary-height", required=True, type=float)
    parser.add_argument("--start-at", type=int, default=0, help="Resume only at a fresh, unexecuted row")
    arguments = parser.parse_args()
    plan = json.loads(arguments.plan.read_text())
    helpers = runpy.run_path(str(pathlib.Path(__file__).with_name("probe-app.py")))
    for binary in plan["binaries"].values():
        with open(binary["path"], "rb") as source:
            if hashlib.file_digest(source, "sha256").hexdigest() != binary["sha256"]:
                raise ValueError("Prepared binary changed: " + binary["path"])
    for index, row in enumerate(plan["runs"]):
        if index < arguments.start_at:
            continue
        root = pathlib.Path(row["manifest"]).parent
        if (root / "process.log").exists() or (root / "done").exists():
            raise ValueError("Never overwrite an attempted run: " + str(root))
        print(json.dumps({"starting": index, "benchmark": row["benchmark"], "cohort": row["cohort"]}), flush=True)
        binary = pathlib.Path(plan["binaries"][row["host"]]["path"])
        if row["host"] == "harness":
            result = run_harness(binary, row, arguments.primary_height)
        else:
            with (root / "process.log").open("w") as output:
                subprocess.run([str(binary), "--puzzle-probe", row["manifest"]],
                               stdout=output, stderr=subprocess.STDOUT, timeout=100, check=True)
            result = helpers["summarize"](root)
            if result["frame"] != row["frame"] or result["display"] != row["display"]:
                raise ValueError("App frame/display readback mismatch: " + str(root))
        print(json.dumps({"finished": index, "benchmark": row["benchmark"], "cohort": row["cohort"],
                          "grid": result["grid"], "frame": result["frame"], "meanMsPerMiB": result["meanMsPerMiB"]}), flush=True)


if __name__ == "__main__":
    main()
