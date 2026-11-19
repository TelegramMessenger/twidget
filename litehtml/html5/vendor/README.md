# html5ever without unsafe code

litehtml parses HTML with this copy of html5ever. Every crate here and in `../core` is `#![forbid(unsafe_code)]`,
and none of them depends on a crate outside this tree, at run time or at build time, so the only trusted code
is the Rust standard library. `../ffi` holds the two `unsafe` blocks the C boundary needs.

| Directory | Upstream | Changes |
|---|---|---|
| `markup5ever` | markup5ever 0.35.0 | `tendril.rs` replaces the tendril crate with a safe `StrTendril`: owned `String`, or a shared `Rc<str>` window so input slices are not copied. `atom.rs` replaces string_cache: static atoms are indices into generated tables, dynamic ones are `Rc<str>`, so `local_name!`/`ns!` still work in patterns. web_atoms' tables live in `generated/` (named entities as a sorted table for binary search instead of phf). `log` is gone. |
| `html5ever` | html5ever 0.35.0 | The SSE2/NEON data-state fast path is replaced by a safe 8-byte SWAR scan. `tree_builder/rules.rs` is generated from `rules.in.rs` with `match_token!` expanded, so no proc macro (syn, quote, proc-macro2) is built. `log` calls are no-ops; the `TendrilSink` driver is an inherent `process`/`finish`/`one`. |

Regenerate after editing the inputs:

```sh
python3 -I vendor/tools/gen.py
cargo run --release --manifest-path vendor/tools/expand-rules/Cargo.toml -- \
    vendor/html5ever/src/tree_builder/rules.in.rs vendor/html5ever/src/tree_builder/rules.rs
```

`tools/expand-rules` is a separate workspace with its own dependencies; it is never part of the parser build.
`python3 -I tools/unsafe_report.py` checks the claims above.

The parser output matches upstream html5ever 0.35.0 byte for byte on the widget corpus, LiteHTMLLab traces and
the html5lib tree-construction inputs, in document, fragment and position modes.

Licenses: MIT or Apache-2.0, as upstream (`LICENSE-*` in each directory; web_atoms' in `tools/`).
