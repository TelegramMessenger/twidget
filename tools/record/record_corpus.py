#!/usr/bin/env python3
"""Records widget-sandbox's corpus with twidget-record (tools/record): every widget through a recorder process of
its own into a recording of its own, <out>/<folder>/<file>.twrec, with --keep, so that a widget that does not load,
render or paint, throws or dies is reported and its session kept. A recording the recorder could not finish is
removed and its log kept. Prints the counts by corpus folder and writes them with every widget's outcome to
<out>/summary.json; exits 1 when a recording failed.

  python -I tools/record/record_corpus.py --recorder build-record/Release/twidget-record.exe [--out DIR] [--jobs N] [folder or file ...]
"""

import argparse
import concurrent.futures
import json
import os
import re
import subprocess
import sys
import time

TWIDGET = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

TROUBLES = [
    ("died loading", "the widget died before it loaded"),
    ("did not load", "the widget did not load"),
    ("died rendering", "the widget died before it rendered"),
    ("did not render", "the widget did not render"),
    ("painted nothing", "the renderer painted nothing"),
    ("threw", "the widget threw"),
    ("died", "the widget died"),
]


def widgets_of(sandbox, wanted):
    corpus = os.path.join(sandbox, "corpus")
    found = []
    for root, dirs, files in os.walk(corpus):
        dirs.sort()
        for name in sorted(files):
            if name.endswith(".html"):
                found.append(os.path.relpath(os.path.join(root, name), corpus).replace(os.sep, "/"))
    if not wanted:
        return found
    chosen = [w for w in found if any(w == p or w.startswith(p.rstrip("/") + "/") for p in wanted)]
    missing = [p for p in wanted if not any(w == p or w.startswith(p.rstrip("/") + "/") for w in found)]
    if missing:
        sys.exit("record_corpus: nothing in the corpus at " + ", ".join(missing))
    return chosen


def kind_of(why):
    for kind, start in TROUBLES:
        if why.startswith(start):
            return kind
    return "other"


def record(recorder, out, widget, timeout):
    recording = os.path.join(out, widget + ".twrec")
    log = os.path.join(out, widget + ".log")
    os.makedirs(os.path.dirname(recording), exist_ok=True)
    for path in (recording, log):
        if os.path.exists(path):
            os.remove(path)
    started = time.monotonic()
    try:
        done = subprocess.run([recorder, "--keep", "--out", recording, widget], capture_output=True, timeout=timeout)
        code, text = done.returncode, (done.stdout + done.stderr).decode("utf-8", "replace")
    except subprocess.TimeoutExpired as e:
        code, text = "timeout", ((e.stdout or b"") + (e.stderr or b"")).decode("utf-8", "replace")
    text = text.replace("\r\n", "\n")
    with open(log, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)
    entry = {"widget": widget, "exit": code, "seconds": round(time.monotonic() - started, 1)}
    counts = re.search(r"^recorded (\d+) sessions, (\d+) steps$", text, re.M)
    if counts:
        entry["sessions"], entry["steps"] = int(counts.group(1)), int(counts.group(2))
    shown = re.search(r"^" + re.escape(widget) + r": steps \d+ to \d+; (\d+) painted frames \((\d+) before the input\), (\d+) engine frames", text, re.M)
    if shown:
        entry["painted"], entry["painted before input"], entry["engine frames"] = (int(g) for g in shown.groups())
    trouble = re.search(r"^trouble " + re.escape(widget) + r": (.*)$", text, re.M)
    if trouble:
        entry["trouble"] = trouble.group(1).strip()
        entry["kind"] = kind_of(entry["trouble"])
    if code == 0 and os.path.exists(recording):
        entry["bytes"] = os.path.getsize(recording)
    else:
        failed = re.search(r"^(?:recording failed|twidget-record): (.*)$", text, re.M)
        entry["failure"] = failed.group(1).strip() if failed else "exit " + str(code)
        if os.path.exists(recording):
            os.remove(recording)
    return entry


def summary(entries):
    folders = {}
    for e in entries:
        f = folders.setdefault(e["widget"].split("/")[0], {"widgets": 0, "recorded": 0, "steps": 0, "bytes": 0, "failed": 0})
        f["widgets"] += 1
        if "failure" in e:
            f["failed"] += 1
            continue
        f["recorded"] += 1
        f["steps"] += e.get("steps", 0)
        f["bytes"] += e.get("bytes", 0)
        if "kind" in e:
            f[e["kind"]] = f.get(e["kind"], 0) + 1
    return folders


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--recorder", required=True, help="twidget-record, built from tools/record")
    parser.add_argument("--sandbox", default=os.environ.get("TWIDGET_WIDGET_SANDBOX") or os.path.join(os.path.dirname(TWIDGET), "widget-sandbox"),
                        help="the widget-sandbox the recorder was configured with")
    parser.add_argument("--out", default=os.path.join(TWIDGET, "build-record", "recordings"))
    parser.add_argument("--jobs", type=int, default=1, help="recorders at once; each waits on real time, so a loaded machine records fewer frames")
    parser.add_argument("--timeout", type=int, default=300, help="seconds a recorder may take")
    parser.add_argument("widgets", nargs="*", help="corpus folders or files, relative to <sandbox>/corpus; all by default")
    args = parser.parse_args()
    recorder = os.path.abspath(args.recorder)
    widgets = widgets_of(os.path.abspath(args.sandbox), args.widgets)
    out = os.path.abspath(args.out)
    print(f"recording {len(widgets)} widgets into {out}, {args.jobs} at once", flush=True)
    entries = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, args.jobs)) as pool:
        for e in pool.map(lambda w: record(recorder, out, w, args.timeout), widgets):
            entries.append(e)
            state = e.get("failure") and "FAILED: " + e["failure"] or e.get("trouble") and "kept, " + e["trouble"] or "recorded"
            print(f"{e['widget']}: {state}; {e.get('steps', 0)} steps, {e.get('bytes', 0)} bytes, {e['seconds']} s", flush=True)
    folders = summary(entries)
    with open(os.path.join(out, "summary.json"), "w", encoding="utf-8", newline="\n") as f:
        json.dump({"recorder": recorder, "folders": folders, "widgets": entries}, f, indent=1)
    print("by folder: widgets, recorded, steps, bytes, failed; troubles kept")
    for name, f in sorted(folders.items()):
        kept = ", ".join(f"{f[k]} {k}" for k, _ in TROUBLES + [("other", "")] if f.get(k))
        print(f"  {name}: {f['widgets']}, {f['recorded']}, {f['steps']}, {f['bytes']}, {f['failed']}; {kept or 'none'}")
    return 1 if any(f["failed"] for f in folders.values()) else 0


if __name__ == "__main__":
    sys.exit(main())
