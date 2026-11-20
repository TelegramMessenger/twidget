#!/usr/bin/env python3
"""The gate every stage passes (rust/PLAN.md): the C++ oracle built from twidget's sources, the tests
with the comparison against it required, clippy with warnings as errors, rustfmt, and the `unsafe`
report. Run `python -I tools/gate.py` from anywhere; cargo and cmake come from PATH (on the Telegram
machines RUSTUP_HOME and CARGO_HOME point at ThirdParty/rust). --release also runs the tests optimized.
"""

import os
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ORACLE = os.path.join(ROOT, "oracle")
BUILD = os.path.join(ORACLE, "build")


def step(name, command, env=None):
    started = time.monotonic()
    print(f"== {name}: {' '.join(command)}", flush=True)
    result = subprocess.run(command, cwd=ROOT, env=env)
    print(f"== {name}: {'ok' if result.returncode == 0 else 'FAILED'} in {time.monotonic() - started:.1f} s", flush=True)
    return result.returncode == 0


def oracle_dir():
    for candidate in (os.path.join(BUILD, "Release"), BUILD):
        if os.path.isfile(os.path.join(candidate, "wire_oracle.exe" if os.name == "nt" else "wire_oracle")):
            return candidate
    return None


def main():
    failed = []
    configure = ["cmake", "-S", ORACLE, "-B", BUILD]
    if os.name != "nt":
        configure += ["-DCMAKE_BUILD_TYPE=Release"]
    elif not os.path.isdir(BUILD):
        configure += ["-A", "x64"]
    if not (step("oracle configure", configure) and step("oracle build", ["cmake", "--build", BUILD, "--config", "Release", "--parallel"])):
        print("GATE FAIL: the oracle does not build")
        return 1
    directory = oracle_dir()
    if not directory:
        print("GATE FAIL: the oracle built, but no program is where the tests look")
        return 1
    env = dict(os.environ, TWIDGET_ORACLE_DIR=directory, TWIDGET_ORACLE_REQUIRED="1")
    checks = [
        ("fmt", ["cargo", "fmt", "--all", "--check"], None),
        ("test", ["cargo", "test", "--workspace", "--locked"], env),
        ("clippy", ["cargo", "clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings"], None),
        ("unsafe", [sys.executable, "-I", os.path.join(ROOT, "tools", "unsafe_report.py")], None),
    ]
    if "--release" in sys.argv:
        checks.insert(2, ("test release", ["cargo", "test", "--workspace", "--locked", "--release"], env))
    for name, command, extra in checks:
        if not step(name, command, extra):
            failed.append(name)
    if failed:
        print(f"GATE FAIL: {', '.join(failed)}")
        return 1
    print("GATE OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
