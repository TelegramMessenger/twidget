#!/usr/bin/env python3
"""Compare two signed macOS desktop widget helpers over their normal hosting socket.

Pass --cpp and --rust Release binaries, --bundles the desktop WidgetJS directory,
and --output a JSON path. An optional --candidate joins the round-robin measurement.
All helpers must accept the same bundles and build version. The first run of each
case checks validated DOM operations. Timed runs launch new processes with warmed
filesystem caches, wait for normal host notifications and compare every paint and
canvas message with that control. Host event-to-paint times exclude display vsync.
"""

import argparse
from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path
import platform
import socket
from statistics import median
import struct
import time


CASES = {"badge": 1, "bar-chart": 16, "canvas-chart": 8}
FRAME_STATE = struct.Struct("<BBQdiBBdB")
FILL = struct.Struct("<BI4f4f4B")
CLOCK = time.perf_counter_ns


def check(condition, reason):
    if not condition:
        raise RuntimeError(reason)


def string(value):
    if isinstance(value, str):
        value = value.encode()
    return struct.pack("<I", len(value)) + value


def json_string(value):
    return string(json.dumps(value, separators=(",", ":")))


def payload(data):
    size, = struct.unpack_from("<I", data)
    check(size == len(data) - 4, "invalid string size")
    return data[4:]


def send(sock, route, kind, fields=b""):
    sock.sendall(string(struct.pack("<IB", route, kind) + fields))


def receive(stream):
    size, = struct.unpack("<I", stream.read(4))
    check(5 <= size <= 34 << 20, "invalid message size")
    message = stream.read(size)
    check(len(message) == size, "helper closed its stream early")
    route, kind = struct.unpack_from("<IB", message)
    return route, kind, message[5:]


def elapsed(start, end):
    return (end - start) / 1_000_000


@contextmanager
def launch(binary):
    parent, child = socket.socketpair()
    with parent, child:
        parent.settimeout(15)
        actions = [(os.POSIX_SPAWN_DUP2, child.fileno(), 3)]
        start = CLOCK()
        pid = os.posix_spawn(str(binary), [str(binary), "3"], {}, file_actions=actions)
        child.close()
        try:
            with parent.makefile("rb") as stream:
                yield parent, stream, start
        finally:
            parent.close()
            waited, status = os.waitpid(pid, 0)
            check(waited == pid and os.waitstatus_to_exitcode(status) == 0, "helper failed to exit cleanly")


def document(case, bars):
    if case == "canvas-chart":
        return """<!doctype html><html><body><canvas width="96" height="36" id="chart"></canvas><script>
const chart = document.getElementById('chart');
const c = chart.getContext('2d');
function show(n) {
    c.clearRect(0, 0, 96, 36);
    for (let i = 0; i < 8; i++) {
        const h = 4 + ((n * 7 + i * 3) % 25);
        c.fillStyle = 'rgb(' + (32 + h) + ',75,121)';
        c.fillRect(i * 12, 36 - h, 9, h);
    }
    document.body.setAttribute('data-bench', 'update-' + n);
}
TelegramWidget.onEvent('chatDataChanged', () => show(Number(TelegramWidget.chatData)));
show(0);
</script></body></html>"""
    cells = "".join(f'<div id="b{i}"></div>' for i in range(bars))
    return f"""<!doctype html><html><body>{cells}<script>
const count = {bars};
function show(n) {{
    for (let i = 0; i < count; ++i) {{
        const bar = document.getElementById('b' + i);
        const x = (n * 7 + i * 3) % 160;
        bar.style.cssText = 'width:' + (48 + x) + 'px;height:4px;background-color:rgb(' + (32 + x) + ',75,121)';
    }}
    document.body.setAttribute('data-bench', 'update-' + n);
}}
TelegramWidget.onEvent('chatDataChanged', () => show(Number(TelegramWidget.chatData)));
show(0);
</script></body></html>"""


def check_frame(frame, step, bars, canvas, previous):
    kind, version, sequence, width, height, _, _, _, _ = FRAME_STATE.unpack_from(frame)
    check((version, sequence, width) == (1, step + 1, 320.0), f"wrong frame at update {step}")
    at = FRAME_STATE.size
    if kind == 73:
        base, size, runs, tag, offset, count = struct.unpack_from("<QIIBII", frame, at)
        check(canvas and previous and (base, size, runs, tag, offset, count) ==
              (step, len(previous), 1, 0, 0, len(previous)), f"unexpected painted delta at update {step}")
        payload(frame[at + struct.calcsize("<QIIBII"):])
        display = previous
    else:
        painted, size = struct.unpack_from("<BI", frame, at)
        check((kind, painted) == (64, 1), f"unpainted frame at update {step}")
        display = frame[at + 5:at + 5 + size]
        payload(frame[at + 5 + size:])
    if canvas:
        check(display[:6] == struct.pack("<BBI", 1, 1, 1), f"wrong canvas display list at update {step}")
        kind, mask, _, _, w, h, _, _, _, _, node = struct.unpack("<BI4f4fi", display[6:])
        check((kind, mask, w, h, node) == (5, 0x80d, 96, 36, 5),
              f"wrong painted canvas at update {step}")
        return display
    check(display[:2] == bytes((1, 1)) and len(display) == 6 + bars * FILL.size,
          f"incomplete display list at update {step}")
    check(struct.unpack_from("<I", display, 2)[0] == bars and height > 0, "wrong painted work")
    for i in range(bars):
        kind, mask, _, _, w, h, _, _, _, _, *color = FILL.unpack_from(display, 6 + i * FILL.size)
        x = (step * 7 + i * 3) % 160
        check((kind, mask, w, h, color) == (0, 29, 48 + x, 4, [32 + x, 75, 121, 255]),
              f"wrong painted bar {i} at update {step}")
    return display


def check_canvas(messages, step):
    batches = []
    for m in messages:
        node, w, h = struct.unpack_from("<iii", m, 2)
        batch = payload(m[14:])
        check((node, w, h, batch[:2]) == (5, 96, 36, bytes((3, 1))), "wrong canvas batch")
        batches.append((struct.unpack_from("<I", batch, 2)[0], batch))
    check([count for count, _ in batches] == ([1, 9] if step == 0 else [9]), f"missing canvas work at update {step}")
    for i in range(8):
        h = 4 + ((step * 7 + i * 3) % 25)
        x, y = 12 * i, 36 - h
        geometry = struct.pack("<8f", x, y, x + 9, y, x + 9, 36, x, 36)
        check(geometry in batches[-1][1] and bytes((32 + h, 75, 121, 255)) in batches[-1][1],
              f"missing painted canvas bar {i} at update {step}")


def measure(binary, opening, case, updates, observe_dom):
    bars = CASES[case]
    startup = (string(document(case, bars)) + json_string(dict(innerWidth=320, innerHeight=120,
               devicePixelRatio=1, theme="light", visible=True)) + json_string(dict(
               theme=dict(colorScheme="light", params={}), stoppedLabel="Stopped", validatedOps=observe_dom)))
    ops = []
    frames = []
    canvas_runs = []
    warm = []
    with launch(binary) as (sock, stream, start):
        send(sock, 0, 32, opening)
        route, kind, answer = receive(stream)
        ready = CLOCK()
        check((route, kind, answer) == (0, 64, struct.pack("<q", 1)), f"hosting handshake failed: {kind}")
        send(sock, 1, 2)
        send(sock, 1, 33, startup)
        send(sock, 1, 35, string(bytes((255, 1)) + struct.pack("<d", 1)))
        renderer = False
        fonts = 0
        for step in range(updates + 1):
            marker = False
            render_pending = False
            frame_requested = False
            batch = []
            canvas_batch = []
            if step:
                data = json_string({"chatData": str(step), "publicData": None})
                event_start = CLOCK()
                send(sock, 1, 11, data)
            deadline = time.monotonic() + 15
            while True:
                check(time.monotonic() < deadline, f"renderer did not paint update {step}")
                route, kind, data = receive(stream)
                arrived = CLOCK()
                check(route == 1 and kind != 86, f"widget stopped or wrong route ({route}, {kind}): {data[:160]!r}")
                if kind == 99:
                    renderer = True
                elif kind == 100 and step == 0:
                    render_pending = True
                elif kind == 98:
                    parsed = json.loads(payload(data))
                    batch.extend(parsed)
                    marker |= any(op.get("name") == "data-bench" and op.get("value") == f"update-{step}" for op in parsed)
                elif kind == 97:
                    request = payload(data)
                    check(request[:2] == bytes((96, 1)), "unexpected platform request")
                    fonts += 1
                    metrics = bytes((4, 1)) + struct.pack("<7f", 12, 4, 16, 8, 8, 12, 4)
                    send(sock, 1, 36, string(bytes((32, 1)) + struct.pack("<i", fonts) + string(metrics)))
                elif kind == 96:
                    message = payload(data)
                    check(message[:2] != bytes((255, 66)), "renderer stopped")
                    if step and message[:2] == bytes((65, 1)):
                        render_pending |= bool(message[14])
                    if message[:2] == bytes((71, 1)):
                        canvas_batch.append(message)
                    if message[:2] in (bytes((64, 1)), bytes((73, 1))):
                        check(frame_requested, "unsolicited frame")
                        if step:
                            warm.append(elapsed(event_start, arrived))
                        else:
                            first = arrived
                        frames.append(message)
                        ops.append(batch)
                        canvas_runs.append(canvas_batch)
                        break
                if renderer and (marker if observe_dom else render_pending) and not frame_requested:
                    send(sock, 1, 35, string(struct.pack("<BBQBB", 1, 1, step + 1, 0, 2)))
                    frame_requested = True
        send(sock, 1, 21)
    previous = None
    for step, frame in enumerate(frames):
        canvas = case == "canvas-chart"
        previous = check_frame(frame, step, bars, canvas, previous)
        if canvas:
            check_canvas(canvas_runs[step], step)
    encoded_ops = json.dumps(ops, sort_keys=True, separators=(",", ":")).encode()
    output = {"validated_dom_sha256": hashlib.sha256(encoded_ops).hexdigest(),
              "paint_sha256": hashlib.sha256(b"".join(string(f) for f in frames)).hexdigest(),
              "frames": len(frames), "painted_bars": bars * len(frames),
              "validated_operations": sum(map(len, ops))}
    if case == "canvas-chart":
        output["canvas_sha256"] = hashlib.sha256(b"".join(string(m) for batch in canvas_runs for m in batch)).hexdigest()
        output["canvas_commands"] = 1 + 9 * len(frames)
    return {"ready_ms": elapsed(start, ready), "first_paint_ms": elapsed(start, first),
            "warm_ms": warm, "warm_median_ms": median(warm), "output": output}


def run(args):
    def bundle(name):
        return (args.bundles / (name + "-bundle.js")).read_bytes()
    version = args.build_version
    if version is None:
        for directory in args.bundles.resolve().parents:
            file = directory / "build/version"
            if file.is_file():
                version = next(line.split()[1] for line in file.read_text().splitlines()
                               if line.startswith("AppVersion "))
                break
    check(version, "supply --build-version when bundles are outside a Desktop checkout")
    bundles = {name: bundle(name) for name in ("sandbox", "validator", "renderer")}
    opening = (struct.pack("<q", 1) + string(version) + string(bundles["sandbox"])
               + string(bundles["validator"]) + string(bundles["renderer"].splitlines()[0]))
    binaries = {"cpp": args.cpp.resolve(), "rust": args.rust.resolve()}
    if args.candidate:
        binaries["candidate"] = args.candidate.resolve()
        orders = [("cpp", "rust", "candidate"), ("rust", "candidate", "cpp"), ("candidate", "cpp", "rust"),
                  ("candidate", "rust", "cpp"), ("rust", "cpp", "candidate"), ("cpp", "candidate", "rust")]
    else:
        orders = [("cpp", "rust"), ("rust", "cpp")]
    report = {"platform": platform.platform(), "machine": platform.machine(),
              "build_version": version, "updates": args.updates, "pairs": args.pairs, "timed_dom_observer": args.observe_dom,
              "binaries": {k: {"path": str(v), "sha256": hashlib.sha256(v.read_bytes()).hexdigest()}
                           for k, v in binaries.items()},
              "bundles": {k: hashlib.sha256(v).hexdigest() for k, v in bundles.items()}, "cases": {}}
    for case in args.cases:
        validated = None
        expected = None
        pairs = []
        for i in range(args.pairs + 1):
            order = orders[(i - 1) % len(orders)]
            row = {"order": list(order)}
            for name in order:
                row[name] = measure(binaries[name], opening, case, args.updates, i == 0 or args.observe_dom)
                output = dict(row[name]["output"])
                dom = {k: output.pop(k) for k in ("validated_dom_sha256", "validated_operations")}
                if i == 0 or args.observe_dom:
                    if validated is None:
                        validated = dom
                    check(dom == validated and dom["validated_operations"] > 0, f"validated DOM mismatch: {dom}")
                else:
                    check(dom["validated_operations"] == 0, "normal host unexpectedly received validated ops")
                if expected is None:
                    expected = output
                check(output == expected, f"output mismatch for {name} {case} pair {i}: {output} != {expected}")
            if i:
                pairs.append(row)
        summary = {}
        for key in ("ready_ms", "first_paint_ms", "warm_median_ms"):
            rates = [p["rust"][key] / p["cpp"][key] for p in pairs]
            summary[key] = {"cpp_median": median(p["cpp"][key] for p in pairs),
                            "rust_median": median(p["rust"][key] for p in pairs),
                            "median_paired_ratio": median(rates), "paired_ratios": rates}
            if args.candidate:
                item = summary[key]
                item["candidate_median"] = median(p["candidate"][key] for p in pairs)
                for reference in ("cpp", "rust"):
                    ratios = [p["candidate"][key] / p[reference][key] for p in pairs]
                    item["median_candidate_vs_" + reference + "_ratio"] = median(ratios)
                    item["candidate_vs_" + reference + "_ratios"] = ratios
        report["cases"][case] = {"output": expected, "validated_control": validated, "summary": summary, "samples": pairs}
        print(case, flush=True)
        for key, stats in summary.items():
            times = ", ".join(f"{name} {stats[name + '_median']:.3f} ms" for name in binaries)
            print(f"  {key}: {times}", flush=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cpp", type=Path, required=True)
    parser.add_argument("--rust", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, help="measure a third binary in balanced order")
    parser.add_argument("--bundles", type=Path, required=True)
    parser.add_argument("--build-version")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--observe-dom", action="store_true", help="also time the optional validated-ops observer")
    parser.add_argument("--pairs", type=int, default=12)
    parser.add_argument("--updates", type=int, default=30)
    parser.add_argument("--cases", nargs="+", choices=CASES, default=list(CASES))
    args = parser.parse_args()
    if min(args.updates, args.pairs) < 1:
        parser.error("--updates and --pairs must be positive")
    run(args)
