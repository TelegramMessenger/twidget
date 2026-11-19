#!/usr/bin/env python3
"""Marks _rust_eh_personality in a prelinked 64-bit Mach-O object as a weak
external definition.

Compact unwind info has room for three personality routines per linked image.
Telegram's binary already uses three: ___objc_personality_v0,
___gxx_personality_v0 and the _rust_eh_personality that the MTProto and wallet
libraries share. A fourth, private copy from Nova does not fit, and ld fails
with "Too many personality routines for compact unwind to encode".

A weak definition merges with another Rust library's _rust_eh_personality, so
the image keeps one Rust personality routine. Linked without another Rust
library, Nova uses its own copy.

    weak_personality.py OBJECT
"""

import struct
import sys

MH_MAGIC_64 = 0xFEEDFACF
MH_OBJECT = 0x1
LC_SYMTAB = 0x2
N_STAB = 0xE0
N_TYPE = 0x0E
N_EXT = 0x01
N_SECT = 0x0E
N_WEAK_DEF = 0x0080
SYMBOL = b"_rust_eh_personality"


def main(path):
    with open(path, "rb") as f:
        data = bytearray(f.read())
    magic, _, _, filetype, ncmds, _, _, _ = struct.unpack_from("<IiiIIIII", data, 0)
    if magic != MH_MAGIC_64 or filetype != MH_OBJECT:
        sys.exit(f"{path}: not a 64-bit Mach-O object")
    offset = 32
    symtab = None
    for _ in range(ncmds):
        cmd, cmdsize = struct.unpack_from("<II", data, offset)
        if cmd == LC_SYMTAB:
            symtab = struct.unpack_from("<IIII", data, offset + 8)
        offset += cmdsize
    if symtab is None:
        sys.exit(f"{path}: no symbol table")
    symoff, nsyms, stroff, strsize = symtab
    matches = []
    for index in range(nsyms):
        entry = symoff + index * 16
        strx, n_type, _, n_desc, _ = struct.unpack_from("<IBBHQ", data, entry)
        if strx >= strsize:
            continue
        start = stroff + strx
        end = data.index(b"\0", start)
        if bytes(data[start:end]) != SYMBOL:
            continue
        if n_type & N_STAB:
            continue
        matches.append((entry, n_type, n_desc))
    if len(matches) != 1:
        sys.exit(f"{path}: expected one {SYMBOL.decode()} symbol, found {len(matches)}")
    entry, n_type, n_desc = matches[0]
    if n_type & N_TYPE != N_SECT or not n_type & N_EXT:
        sys.exit(f"{path}: {SYMBOL.decode()} is not an external definition (n_type {n_type:#x})")
    struct.pack_into("<H", data, entry + 6, n_desc | N_WEAK_DEF)
    with open(path, "wb") as f:
        f.write(data)


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    main(sys.argv[1])
