#!/usr/bin/env python3
"""What the helper's Rust build leaves to trust: `unsafe` in the workspace and in every crate it links
(proc-macro crates and what only they use run in the compiler, not in the process, and are left out).

Counts the `unsafe` keyword outside comments and literals in each package's .rs files (the workspace's
own in all its targets, a dependency's without tests/, benches/ and examples/), checks that every
workspace crate root carries #![forbid(unsafe_code)], and fails when any package but the C ABI crates in
ALLOWED has `unsafe` or a root lacks the attribute. Run with `python -I tools/unsafe_report.py` from
anywhere; --files lists the places.
"""

import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# The only crates that may hold `unsafe`: the C ABI between the C++ OS glue and the helper (rust/PLAN.md).
ALLOWED = {"twidget_helper_ffi"}

FORBID = re.compile(r"#!\[\s*forbid\s*\(\s*unsafe_code\s*\)\s*\]")
WORD = re.compile(r"\bunsafe\b")


def strip(source):
    """The source with comments, strings and char literals blanked, newlines kept."""
    out = []
    i, n = 0, len(source)
    while i < n:
        c = source[i]
        if source.startswith("//", i):
            j = source.find("\n", i)
            j = n if j < 0 else j
            i = j
            continue
        if source.startswith("/*", i):
            depth, j = 1, i + 2
            while j < n and depth:
                if source.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif source.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            out.append("\n" * source.count("\n", i, j))
            i = j
            continue
        raw = re.match(r"b?r(#*)\"", source[i:i + 260])
        if raw and (i == 0 or not (source[i - 1].isalnum() or source[i - 1] == "_")):
            end = '"' + raw.group(1)
            j = source.find(end, i + raw.end())
            j = n if j < 0 else j + len(end)
            out.append("\n" * source.count("\n", i, j))
            i = j
            continue
        if c == '"' or (c == "b" and source.startswith('b"', i)):
            j = i + (2 if c == "b" else 1)
            while j < n and source[j] != '"':
                j += 2 if source[j] == "\\" else 1
            out.append("\n" * source.count("\n", i, j))
            i = j + 1
            continue
        if c == "'":
            lit = re.match(r"'(\\(x[0-9a-fA-F]{2}|u\{[0-9a-fA-F]+\}|.)|[^\\'])'", source[i:i + 14])
            if lit:
                i += lit.end()
                continue
        out.append(c)
        i += 1
    return "".join(out)


def rust_files(directory, skip_tests):
    for base, dirs, files in os.walk(directory):
        dirs[:] = [d for d in dirs if d not in ("target", ".git") and not (skip_tests and d in ("tests", "benches", "examples"))]
        for name in files:
            if name.endswith(".rs"):
                yield os.path.join(base, name)


def count(directory, skip_tests):
    total, places = 0, []
    for path in rust_files(directory, skip_tests):
        with open(path, encoding="utf-8", errors="replace") as f:
            text = strip(f.read())
        for number, line in enumerate(text.split("\n"), 1):
            hits = len(WORD.findall(line))
            if hits:
                total += hits
                places.append((path, number))
    return total, places


def linked_packages(metadata):
    """The workspace and what its crates link: normal dependencies, not dev or build ones."""
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    packages = {p["id"]: p for p in metadata["packages"]}
    seen, todo = set(), list(metadata["workspace_members"])
    while todo:
        pid = todo.pop()
        if pid in seen:
            continue
        # A proc-macro and what only it uses run in the compiler, not in the process (nova-safe's safe/TCB.md method).
        if any("proc-macro" in t["kind"] for t in packages[pid]["targets"]):
            continue
        seen.add(pid)
        for dep in nodes[pid]["deps"]:
            kinds = [k["kind"] for k in dep["dep_kinds"]]
            if pid in metadata["workspace_members"] or any(k is None for k in kinds):
                if any(k is None for k in kinds):
                    todo.append(dep["pkg"])
    return [packages[p] for p in sorted(seen)]


def shown(path):
    try:
        return os.path.relpath(path, ROOT)
    except ValueError:
        return path


def main():
    show_files = "--files" in sys.argv
    metadata = json.loads(subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked"],
        cwd=ROOT, capture_output=True, text=True, check=True).stdout)
    members = set(metadata["workspace_members"])
    failed = False
    rows = []
    for package in linked_packages(metadata):
        directory = os.path.dirname(package["manifest_path"])
        own = package["id"] in members
        total, places = count(directory, skip_tests=not own)
        missing = []
        if own:
            for target in package["targets"]:
                if set(target["kind"]) & {"lib", "bin", "staticlib", "cdylib", "rlib"}:
                    with open(target["src_path"], encoding="utf-8") as f:
                        if not FORBID.search(f.read()) and package["name"] not in ALLOWED:
                            missing.append(shown(target["src_path"]))
        allowed = package["name"] in ALLOWED
        bad = (total and not allowed) or missing
        failed = failed or bool(bad)
        source = "workspace" if own else ("registry" if (package.get("source") or "").startswith("registry") else "path")
        rows.append((package["name"], package["version"], source, total, "allowed" if allowed else ("FAIL" if bad else "ok")))
        for root in missing:
            print(f"{package['name']}: {root} has no #![forbid(unsafe_code)]")
        if show_files or (total and not allowed):
            for path, number in places:
                print(f"  {shown(path)}:{number}")
    width = max(len(r[0]) for r in rows)
    print(f"{'crate':<{width}}  version   source     unsafe  state")
    for name, version, source, total, state in rows:
        print(f"{name:<{width}}  {version:<9} {source:<10} {total:>6}  {state}")
    linked = sum(1 for r in rows if r[2] != "workspace")
    print(f"{len(rows)} crates ({linked} linked from outside the workspace), "
          f"{sum(r[3] for r in rows if r[4] != 'allowed')} unsafe outside {sorted(ALLOWED)}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
