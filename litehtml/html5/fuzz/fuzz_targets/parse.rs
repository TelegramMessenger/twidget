#![no_main]

use libfuzzer_sys::fuzz_target;
use lh_html5::{parse, Limits, Span, NONE};

fn check_span(s: Span, limit: usize) {
    if s.start != NONE {
        assert!((s.start as usize) + (s.len as usize) <= limit, "span out of range");
    }
}

fn check(input: &[u8], fragment: bool, limits: Limits) {
    let flat = parse(input, fragment, limits);
    assert!(!flat.nodes.is_empty());
    assert!(flat.nodes.len() <= limits.max_nodes);
    assert!(flat.strings.len() <= limits.max_strings);
    let strings = flat.strings.len();
    for (i, n) in flat.nodes.iter().enumerate() {
        if i > 0 {
            assert!((n.parent as usize) < i, "parent after child");
        }
        assert!((n.first_child as usize) + (n.child_count as usize) <= flat.children.len());
        assert!((n.first_attr as usize) + (n.attr_count as usize) <= flat.attrs.len());
        assert!(n.attr_count as usize <= limits.max_attrs);
        check_span(n.name, strings);
        check_span(n.text, strings);
        if flat.positions {
            check_span(n.source, input.len());
        }
    }
    for a in &flat.attrs {
        check_span(a.name, strings);
        check_span(a.value, strings);
        if flat.positions {
            check_span(a.source, input.len());
        }
    }
    for &c in &flat.children {
        assert!((c as usize) < flat.nodes.len());
    }
}

fuzz_target!(|data: &[u8]| {
    let (limits, body) = match data.split_first() {
        Some((&b, rest)) if b & 0x80 != 0 => (
            Limits {
                max_input: 1 << 20,
                max_nodes: 4 + (b as usize & 0x0f) * 16,
                max_depth: 2 + (b as usize >> 4 & 0x7) * 4,
                max_attrs: 1 + (b as usize & 0x3),
                max_strings: 64 + (b as usize & 0x1f) * 64,
            },
            rest,
        ),
        _ => (Limits { max_input: 1 << 20, max_nodes: 20_000, max_depth: 512, max_attrs: 256, max_strings: 8 << 20 }, data),
    };
    check(body, false, limits);
    check(body, true, limits);
});
