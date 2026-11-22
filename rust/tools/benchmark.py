#!/usr/bin/env python3
"""Compare fixed widget renderer workloads in two Release builds."""

import argparse
import hashlib
import json
import math
import os
import platform
import re
import statistics
import subprocess
import sys
import tempfile
from pathlib import Path


SYNTHETIC = (
    ("mixed", 60), ("transform", 60), ("swarm", 60),
    ("css", 60), ("css", 300), ("drag", 60), ("drag-doc", 60), ("canvas", 50),
)
FIELDS = re.compile(r"\b(setup|total|receive|tick|paint|serialize|build)=([\d.]+)(ms|us)\b")
RECORDINGS = Path(__file__).resolve().parents[2] / "tests" / "recordings"


def run(program, *arguments):
    proc = subprocess.run([str(program), *map(str, arguments)], capture_output=True, text=True)
    if proc.returncode:
        raise ValueError(f"{program.name} exited {proc.returncode}\n{proc.stderr[-2000:]}\n{proc.stdout[-1600:]}")
    return proc.stdout.splitlines()


def fingerprint(path):
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def steady(lines):
    return [line for line in lines if not line.startswith(("BENCH mode=", "BENCH stages "))]


def verify_trace(programs, args):
    control_trace = "--trace-original-css" if args[0] == "css" else "--trace"
    expected = steady(run(programs["cpp"], control_trace, *args))
    actual = steady(run(programs["rust"], "--trace", *args))
    if not expected or not any(line.startswith("paint ") for line in expected):
        raise ValueError("the control trace contains no paint")
    if expected != actual:
        first = next((n for n, (a, b) in enumerate(zip(expected, actual)) if a != b), min(len(expected), len(actual)))
        raise ValueError(f"trace differs at line {first + 1}\nC++ {expected[first:first + 1]}\nRust {actual[first:first + 1]}")
    return hashlib.sha256("\n".join(expected).encode()).hexdigest()


def synthetic_metrics(program, args):
    lines = run(program, "--step", *args)
    reported = {name: float(value) * (1000 if unit == "ms" else 1)
                for name, value, unit in FIELDS.findall("\n".join(lines))}
    if set(reported) != {"setup", "total", "receive", "tick", "paint", "serialize", "build"}:
        raise ValueError(f"unexpected benchmark report: {lines[-3:]}")
    counters = [line for line in lines if line.startswith("BENCH layouts=")]
    if len(counters) != 1 or reported["total"] <= 0:
        raise ValueError(f"missing work or counters: {lines[-3:]}")
    return {"frame": reported.pop("total"), **reported}, counters[0]


def replay_metrics(program, args):
    lines = run(program, *args, "--benchmark")
    report = json.loads(lines[-1])
    if report["schema"] != 1 or report["timing"] != "renderer_loopback_ns" or not report["sessions"]:
        raise ValueError("unknown or empty replay report")
    values = {}
    signature = []
    for session in report["sessions"]:
        name = f"{session['id']}-{session['label']}"
        steps = session["steps"]
        kinds = [kind for kind, _ in steps]
        index = session["first_paint_step"]
        signature.append((name, kinds, index, session["frames"]))
        if not steps or any(not math.isfinite(ns) or ns < 0 for _, ns in steps):
            raise ValueError(f"invalid replay timings in {name}")
        if index is not None:
            if not 1 <= index <= len(steps) or session["first_paint_ns"] is None:
                raise ValueError(f"invalid first paint marker in {name}")
            values[name + "/first-paint"] = session["first_paint_ns"] / 1000
            if index < len(steps):
                values[name + "/after-first-paint"] = sum(ns for _, ns in steps[index:]) / 1000
            repaints = [steps[step - 1][1] for step, painted, commands in session["frames"]
                        if step > index and painted and commands]
            if repaints:
                values[name + "/active-repaint"] = statistics.mean(repaints) / 1000
        values[name + "/session"] = sum(ns for _, ns in steps) / 1000
        for kind in sorted(set(kinds)):
            samples = [ns for k, ns in steps if k == kind]
            values[f"{name}/step/{kind}"] = sum(samples) / (1000 * len(samples))
    return values, signature


def measure(programs, read, args, samples):
    pairs = []
    signature = None
    for turn in range(samples + 1):
        pair = {}
        for arm in (("cpp", "rust") if turn % 2 == 0 else ("rust", "cpp")):
            values, work = read(programs[arm], args)
            if signature is None:
                signature = work
            if work != signature:
                raise ValueError(f"work or operation counts changed for {arm} on run {turn}")
            if any(not math.isfinite(value) or value < 0 for value in values.values()):
                raise ValueError(f"invalid timing for {arm} on run {turn}")
            pair[arm] = values
        if pair["cpp"].keys() != pair["rust"].keys():
            raise ValueError("the two renderers reported different measurements")
        if turn:
            pairs.append(pair)
    result = {}
    for metric in pairs[0]["cpp"]:
        cpp = [p["cpp"][metric] for p in pairs]
        rust = [p["rust"][metric] for p in pairs]
        ratios = [r / c for r, c in zip(rust, cpp)] if all(cpp) else []
        median = statistics.median(ratios) if ratios else None
        result[metric] = {"cpp_us": cpp, "rust_us": rust, "cpp_median_us": statistics.median(cpp),
                          "rust_median_us": statistics.median(rust), "paired_ratio": median,
                          "ratio_mad": statistics.median(abs(r - median) for r in ratios) if ratios else None}
    return result


def write_report(path, report):
    with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=path.parent, delete=False) as file:
        json.dump(report, file, indent=2, allow_nan=False)
        file.write("\n")
    os.replace(file.name, path)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for tool in ("cpp-bench", "rust-bench", "cpp-replay", "rust-replay"):
        parser.add_argument("--" + tool, type=Path)
    parser.add_argument("--recording", action="append", metavar="SOURCE=FILE",
                        help="override the default recordings; repeat to select multiple verified recordings")
    parser.add_argument("--samples", type=int, default=7, help="measured pairs after a warm-up pair")
    parser.add_argument("--frames", type=int, default=120)
    parser.add_argument("--case", action="append", help="run only this full case name; repeat to select more")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.samples < 3 or args.frames < 1:
        parser.error("at least three measured pairs and one frame are required")
    cases, binaries = {}, {}
    for family, cpp, rust in [("synthetic", args.cpp_bench, args.rust_bench), ("replay", args.cpp_replay, args.rust_replay)]:
        if bool(cpp) != bool(rust):
            parser.error(f"both {family} programs are required")
        if not cpp:
            continue
        programs = {"cpp": cpp.resolve(strict=True), "rust": rust.resolve(strict=True)}
        if programs["cpp"].samefile(programs["rust"]):
            parser.error(f"{family} needs distinct control and candidate executables")
        binaries[family] = {arm: {"path": str(path), "sha256": fingerprint(path)} for arm, path in programs.items()}
        if family == "synthetic":
            for mode, count in SYNTHETIC:
                cases[f"synthetic/{mode}/{count}"] = (family, programs, [mode, count, args.frames])
        else:
            defaults = sorted((RECORDINGS / "ordinary").glob("*.twrec"))
            recordings = args.recording or [f"{path.stem}={path}" for path in defaults]
            for item in recordings:
                source, sep, value = item.partition("=")
                if not sep or not source:
                    parser.error("recordings take the form SOURCE=FILE")
                path = Path(value).resolve(strict=True)
                name = "replay/" + path.stem
                if name in cases:
                    parser.error(f"duplicate recording name {name}")
                cases[name] = (family, programs, [str(path), "--source", source])
    if not cases or (args.recording and not args.cpp_replay):
        parser.error("supply a program pair for each selected benchmark family")
    selected = set(args.case or cases)
    if selected - cases.keys():
        parser.error("unknown cases: " + ", ".join(sorted(selected - cases.keys())))
    cpu = platform.processor()
    if sys.platform == "darwin":
        cpu = subprocess.check_output(["sysctl", "-n", "machdep.cpu.brand_string"], text=True).strip()
    misses = []
    report = {"schema": 1, "complete": False, "selected": sorted(selected), "at_or_over_two": misses,
              "processor": cpu, "machine": platform.machine(), "platform": platform.platform(), "samples": args.samples,
              "frames": args.frames, "binaries": binaries, "cases": {}}
    write_report(args.output, report)
    for name, (family, programs, operands) in cases.items():
        if name not in selected:
            continue
        evidence = {}
        if family == "synthetic":
            evidence["trace_sha256"] = verify_trace(programs, operands)
        else:
            evidence["recording_sha256"] = fingerprint(Path(operands[0]))
        metrics = measure(programs, synthetic_metrics if family == "synthetic" else replay_metrics, operands, args.samples)
        evidence["metrics"] = metrics
        report["cases"][name] = evidence
        for key, row in metrics.items():
            if family == "synthetic" and key != "frame":
                continue
            if family == "replay" and "/step/" in key:
                continue
            ratio = row["paired_ratio"]
            if ratio is None:
                raise ValueError(f"{name} {key}: unmeasurable control duration")
            print(f"{name} {key}: {ratio:.3f}x (C++ {row['cpp_median_us']:.2f} us; Rust {row['rust_median_us']:.2f} us)", flush=True)
            if ratio >= 2:
                misses.append((name, key))
        write_report(args.output, report)
    report["complete"] = True
    write_report(args.output, report)
    return 1 if misses else 0


if __name__ == "__main__":
    sys.exit(main())
