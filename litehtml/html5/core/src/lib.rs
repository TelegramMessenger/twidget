#![forbid(unsafe_code)]

mod scan;
mod sink;
mod tags;

use std::collections::HashMap;
use std::sync::OnceLock;

pub use tags::TAGS;

pub const NODE_DOCUMENT: u8 = 0;
pub const NODE_ELEMENT: u8 = 1;
pub const NODE_TEXT: u8 = 2;
pub const NODE_CDATA: u8 = 3;
pub const NODE_COMMENT: u8 = 4;
pub const NODE_WHITESPACE: u8 = 5;
pub const NODE_TEMPLATE: u8 = 6;

pub const NS_HTML: u8 = 0;
pub const NS_SVG: u8 = 1;
pub const NS_MATHML: u8 = 2;

pub const QUIRKS_NO: u8 = 0;
pub const QUIRKS_YES: u8 = 1;
pub const QUIRKS_LIMITED: u8 = 2;

pub const NONE: u32 = u32::MAX;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_input: usize,
    pub max_nodes: usize,
    pub max_depth: usize,
    pub max_attrs: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_input: 32 << 20, max_nodes: 200_000, max_depth: 512, max_attrs: 256 }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Span {
    pub start: u32,
    pub len: u32,
}

impl Span {
    pub const NONE: Span = Span { start: NONE, len: 0 };
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FlatNode {
    pub kind: u8,
    pub ns: u8,
    pub tag: u16,
    pub parent: u32,
    pub first_child: u32,
    pub child_count: u32,
    pub first_attr: u32,
    pub attr_count: u32,
    pub name: Span,
    pub text: Span,
    pub source: Span,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FlatAttr {
    pub ns: u32,
    pub name: Span,
    pub value: Span,
    pub source: Span,
}

#[derive(Debug, Default)]
pub struct Flat {
    pub nodes: Vec<FlatNode>,
    pub children: Vec<u32>,
    pub attrs: Vec<FlatAttr>,
    pub strings: Vec<u8>,
    pub root: u32,
    pub quirks: u8,
    pub truncated: bool,
    pub positions: bool,
}

impl Flat {
    pub fn str(&self, s: Span) -> &str {
        if s.start == NONE {
            return "";
        }
        let a = s.start as usize;
        std::str::from_utf8(&self.strings[a..a + s.len as usize]).unwrap_or("")
    }

    fn push_str(&mut self, s: &str) -> Span {
        let start = self.strings.len() as u32;
        for b in s.bytes() {
            self.strings.push(b);
        }
        self.strings.push(0);
        Span { start, len: s.len() as u32 }
    }
}

pub fn tag_index(local: &str) -> u16 {
    static MAP: OnceLock<HashMap<&'static str, u16>> = OnceLock::new();
    let map = MAP.get_or_init(|| TAGS.iter().enumerate().map(|(i, t)| (*t, i as u16)).collect());
    let unknown = TAGS.len() as u16;
    if local.bytes().any(|b| b.is_ascii_uppercase()) {
        map.get(local.to_ascii_lowercase().as_str()).copied().unwrap_or(unknown)
    } else {
        map.get(local).copied().unwrap_or(unknown)
    }
}

pub fn parse(input: &[u8], fragment: bool, limits: Limits) -> Flat {
    let (text, exact) = match std::str::from_utf8(input) {
        Ok(s) => (std::borrow::Cow::Borrowed(s), true),
        Err(_) => (String::from_utf8_lossy(input), false),
    };
    let mut truncated = false;
    let text: &str = if text.len() > limits.max_input {
        truncated = true;
        let mut end = limits.max_input;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        &text[..end]
    } else {
        &text
    };
    let tree = sink::run(text, fragment, limits);
    let spans = if exact && !truncated { scan::align(text, &tree.marks) } else { scan::Spans::default() };
    let mut flat = sink::flatten(&tree, &spans, text, limits);
    flat.truncated |= truncated;
    flat.positions = exact && !truncated && spans.ok;
    flat
}
