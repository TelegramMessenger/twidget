#!/usr/bin/env python3
"""Writes rust/crates/litehtml/src/gumbo/tables.rs and tag_enum.rs from Gumbo's C sources in litehtml/src/gumbo: the tag
enum, names and sizes (gumbo-include/gumbo/tag_enum.h, tag_strings.h, tag_sizes.h), gperf's tag hash
(tag_gperf.h), and char_ref.c's Ragel machine (its ten tables and what each of its 2242 actions does).
Nothing in the output is typed by hand; tests/gumbo.rs reads the same C files again and checks the result.
Run `python -I tools/gen_gumbo_tables.py` from anywhere.
"""

import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
GUMBO = os.path.join(ROOT, "..", "litehtml", "src", "gumbo")
INCLUDE = os.path.join(ROOT, "..", "litehtml", "gumbo-include", "gumbo")
OUT = os.path.join(ROOT, "crates", "litehtml", "src", "gumbo", "tables.rs")
TAG_ENUM = os.path.join(ROOT, "crates", "litehtml", "src", "gumbo", "tag_enum.rs")


def read(path):
    with open(path, encoding="utf-8") as f:
        return f.read()


def lines_of(text):
    """The lines of a generated header that are not comments."""
    return [line.strip() for line in text.splitlines() if line.strip() and not line.strip().startswith("//")]


def c_array(source, name):
    """The numbers of `static const <type> name[] = { ... };` and the C type."""
    match = re.search(r"static const ([a-z ]+?) " + re.escape(name) + r"\[\] = \{(.*?)\};", source, re.S)
    if not match:
        sys.exit(f"no array {name}")
    return match.group(1), [int(v, 0) for v in match.group(2).replace("\n", " ").split(",") if v.strip()]


RUST_TYPE = {"short": "i16", "char": "i8", "unsigned short": "u16", "unsigned char": "u8"}


def array(out, name, ctype, values, doc):
    rust = RUST_TYPE[ctype]
    out.append(f"/// {doc}")
    out.append(f"pub static {name}: [{rust}; {len(values)}] = [")
    for i in range(0, len(values), 24):
        out.append("    " + ", ".join(str(v) for v in values[i : i + 24]) + ",")
    out.append("];")
    out.append("")


ACTION = re.compile(
    r"\tcase (\d+):\n#line \d+ \"[^\"]+\"\n\t(.*?)\n\tbreak;", re.S)
OUTPUT = re.compile(r"\{ output->first = (0x[0-9a-fA-F]+);(?: output->second = (0x[0-9a-fA-F]+);)? \{p\+\+; goto _out; \} \}")


def actions(source):
    """What each case of the transition actions' switch does: (kind, first, second)."""
    start = source.index("switch ( *(_acts++) )")
    end = source.index("_again:", start)
    body = source[start:end]
    found = {}
    for case, code in ACTION.findall(body):
        case = int(case)
        if code == "{te = p+1;}":
            found[case] = ("TE_NEXT", -1, -1)
            continue
        out = OUTPUT.search(code)
        if not out:
            sys.exit(f"char_ref.c case {case}: {code!r} is none of the shapes this script knows")
        first, second = int(out.group(1), 16), int(out.group(2), 16) if out.group(2) else -1
        if code.startswith("{te = p+1;{"):
            kind = "MATCH_NEXT"
        elif code.startswith("{te = p;p--;{"):
            kind = "MATCH_HERE"
        elif code.startswith("{{p = ((te))-1;}{"):
            kind = "MATCH_TE"
        else:
            sys.exit(f"char_ref.c case {case}: {code!r} is none of the shapes this script knows")
        found[case] = (kind, first, second)
    return found


def main():
    out = [
        "//! Gumbo's tables, written by `tools/gen_gumbo_tables.py` from litehtml/src/gumbo and gumbo-include; do not",
        "//! edit. `tests/gumbo.rs` reads the C sources again and checks every entry.",
        "",
        "use super::char_ref::Action;",
        "use super::*;",
        "",
    ]

    enum = [line.rstrip(",") for line in lines_of(read(os.path.join(INCLUDE, "tag_enum.h")))]
    names = [line.rstrip(",").strip('"') for line in lines_of(read(os.path.join(GUMBO, "tag_strings.h")))]
    sizes = [int(v) for v in " ".join(lines_of(read(os.path.join(GUMBO, "tag_sizes.h")))).replace(",", " ").split()]
    if not (len(enum) == len(names) == len(sizes)):
        sys.exit(f"tag_enum.h has {len(enum)} tags, tag_strings.h {len(names)}, tag_sizes.h {len(sizes)}")
    tags = [
        "//! GumboTag's values, written by `tools/gen_gumbo_tables.py` from gumbo-include/gumbo/tag_enum.h; do not edit.",
        "",
        "use super::GumboTag;",
        "",
    ]
    for i, tag in enumerate(enum):
        tags.append(f"pub const {tag}: GumboTag = {i};")
    tags.append(f"pub const GUMBO_TAG_UNKNOWN: GumboTag = {len(enum)};")
    tags.append(f"pub const GUMBO_TAG_LAST: GumboTag = {len(enum) + 1};")
    tags.append("")
    with open(TAG_ENUM, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(tags))
    out.append("/// kGumboTagNames: tag_strings.h, then \"\" for GUMBO_TAG_UNKNOWN and GUMBO_TAG_LAST.")
    out.append(f"pub static TAG_NAMES: [&[u8]; {len(names) + 2}] = [")
    for name in names + ["", ""]:
        out.append(f'    b"{name}",')
    out.append("];")
    out.append("")
    out.append("/// kGumboTagSizes: tag_sizes.h, then 0 and 0.")
    out.append(f"pub static TAG_SIZES: [u8; {len(sizes) + 2}] = [")
    for i in range(0, len(sizes) + 2, 24):
        out.append("    " + ", ".join(str(v) for v in (sizes + [0, 0])[i : i + 24]) + ",")
    out.append("];")
    out.append("")

    gperf = read(os.path.join(GUMBO, "tag_gperf.h"))
    asso = re.search(r"static unsigned short asso_values\[\] =\s*\{(.*?)\};", gperf, re.S)
    asso = [int(v) for v in asso.group(1).replace("\n", " ").split(",") if v.strip()]
    array(out, "TAG_ASSO_VALUES", "unsigned short", asso, "tag_hash's asso_values (tag_gperf.h).")
    tag_map = re.search(r"static const unsigned char kGumboTagMap\[\] = \{(.*?)\};", gperf, re.S)
    tag_map = [v.strip() for v in tag_map.group(1).split(",") if v.strip()]
    out.append("/// kGumboTagMap (tag_gperf.h).")
    out.append(f"pub static TAG_MAP: [GumboTag; {len(tag_map)}] = [")
    for i in range(0, len(tag_map), 6):
        out.append("    " + ", ".join(tag_map[i : i + 6]) + ",")
    out.append("];")
    out.append("")

    source = read(os.path.join(GUMBO, "char_ref.c"))
    for table in ["actions", "trans_keys", "key_spans", "index_offsets", "indicies", "trans_targs", "trans_actions",
                  "to_state_actions", "from_state_actions", "eof_trans"]:
        ctype, values = c_array(source, f"_char_ref_{table}")
        array(out, f"CHAR_REF_{table.upper()}", ctype, values, f"char_ref.c's _char_ref_{table} ({ctype}).")
    start = int(re.search(r"static const int char_ref_start = (\d+);", source).group(1))
    out.append(f"pub const CHAR_REF_START: i32 = {start};")
    out.append("")
    found = actions(source)
    count = max(found) + 1
    out.append("/// What a case of the transition actions' switch does (none for 0 and 1, which only the state actions")
    out.append("/// run): its kind, then output->first and output->second (-1 when it sets none).")
    out.append(f"pub static CHAR_REF_ACTION_CODE: [(Action, i32, i32); {count}] = [")
    for case in range(count):
        kind, first, second = found.get(case, ("NONE", -1, -1))
        out.append(f"    (Action::{kind}, {first}, {second}),")
    out.append("];")
    out.append("")

    utf8 = read(os.path.join(GUMBO, "utf8.c"))
    body = re.search(r"static const uint8_t utf8d\[\] = \{(.*?)\};", utf8, re.S).group(1)
    body = re.sub(r"//[^\n]*", "", body)
    array(out, "UTF8D", "unsigned char", [int(v) for v in body.replace("\n", " ").split(",") if v.strip()],
          "utf8.c's utf8d: byte classes, then the transitions.")

    pairs = re.search(r"static const CharReplacement kCharReplacements\[\] = \{(.*?)\};", source, re.S).group(1)
    pairs = [(int(a, 0), int(b, 0)) for a, b in re.findall(r"\{(-?0x[0-9a-fA-F]+|-?\d+), (-?0x[0-9a-fA-F]+|-?\d+)\}", pairs)]
    out.append("/// char_ref.c's kCharReplacements, its terminator included.")
    out.append(f"pub static CHAR_REPLACEMENTS: [(i32, i32); {len(pairs)}] = [")
    for a, b in pairs:
        out.append(f"    ({a:#x}, {b:#x}),".replace("-0x1", "-1"))
    out.append("];")
    out.append("")

    parser = read(os.path.join(GUMBO, "parser.c"))
    literal = re.compile(r'"((?:[^"\\]|\\.)*)"')

    def rust_bytes(text):
        return 'b"' + text.replace("\\", "\\\\").replace('"', '\\"') + '"'

    def pieces(name):
        body = re.search(r"static const GumboStringPiece " + name + r"\[\] =\s*\{(.*?)TERMINATOR\};", parser, re.S).group(1)
        return ["".join(literal.findall(entry)) for entry in re.findall(r"GUMBO_STRING\((.*?)\)(?:,|\s*$)", body, re.S)]

    for name in ["kQuirksModePublicIdPrefixes", "kQuirksModePublicIdExactMatches", "kQuirksModeSystemIdExactMatches",
                 "kLimitedQuirksPublicIdPrefixes", "kLimitedQuirksRequiresSystemIdPublicIdPrefixes"]:
        values = pieces(name)
        out.append(f"/// parser.c's {name}, without the terminator.")
        out.append(f"pub static {name}: [&[u8]; {len(values)}] = [")
        for v in values:
            out.append(f"    {rust_bytes(v)},")
        out.append("];")
        out.append("")
    for name in ["kSvgAttributeReplacements", "kSvgTagReplacements"]:
        body = re.search(r"static const ReplacementEntry " + name + r"\[\] = \{(.*?)\};", parser, re.S).group(1)
        values = re.findall(r'REPLACEMENT_ENTRY\("([^"]*)", "([^"]*)"\)', body)
        out.append(f"/// parser.c's {name}: from, to.")
        out.append(f"pub static {name}: [(&[u8], &[u8]); {len(values)}] = [")
        for a, b in values:
            out.append(f"    ({rust_bytes(a)}, {rust_bytes(b)}),")
        out.append("];")
        out.append("")
    body = re.search(r"kForeignAttributeReplacements\[\] = \{(.*?)\};", parser, re.S).group(1)
    values = re.findall(r'\{"([^"]*)",\s*"([^"]*)",\s*(GUMBO_ATTR_NAMESPACE_[A-Z]+)\}', body)
    out.append("/// parser.c's kForeignAttributeReplacements: from, local name, namespace.")
    out.append(f"pub static kForeignAttributeReplacements: [(&[u8], &[u8], GumboAttributeNamespaceEnum); {len(values)}] = [")
    for a, b, ns in values:
        out.append(f"    ({rust_bytes(a)}, {rust_bytes(b)}, {ns}),")
    out.append("];")
    out.append("")

    with open(OUT, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(out))
    print(f"wrote {OUT}: {len(enum)} tags, {count} char_ref actions")


if __name__ == "__main__":
    main()
