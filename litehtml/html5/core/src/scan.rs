use std::cell::RefCell;

use html5ever::tendril::StrTendril;
use html5ever::tokenizer::{BufferQueue, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts};

use crate::sink::{Mark, MarkKind, Raw};
use crate::Span;

#[derive(Debug, Default)]
pub struct TagSpans {
    pub tag: Span,
    pub attrs: Vec<(String, Span)>,
}

#[derive(Debug, Default)]
pub struct Spans {
    pub ok: bool,
    pub tags: Vec<Option<TagSpans>>,
    pub runs: Vec<Span>,
}

fn span(start: usize, end: usize) -> Span {
    Span { start: start as u32, len: (end - start) as u32 }
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0c)
}

enum Found {
    Tag { start: usize, end: usize, end_tag: bool, name: String, attrs: Vec<(String, Span)> },
    Comment { start: usize, end: usize, text_start: usize, text_end: usize },
    Doctype { start: usize, end: usize },
}

struct Scanner<'a> {
    s: &'a [u8],
    pos: usize,
}

impl<'a> Scanner<'a> {
    fn find_byte(&self, from: usize, b: u8) -> usize {
        self.s[from.min(self.s.len())..].iter().position(|&c| c == b).map(|p| from + p).unwrap_or(self.s.len())
    }

    fn starts_ci(&self, at: usize, pat: &[u8]) -> bool {
        at + pat.len() <= self.s.len() && self.s[at..at + pat.len()].eq_ignore_ascii_case(pat)
    }

    fn tag(&self, start: usize, end_tag: bool) -> Option<Found> {
        let s = self.s;
        let mut i = start + if end_tag { 2 } else { 1 };
        let name_start = i;
        while i < s.len() && !is_space(s[i]) && s[i] != b'/' && s[i] != b'>' {
            i += 1;
        }
        let name = String::from_utf8_lossy(&s[name_start..i]).to_ascii_lowercase();
        let mut attrs = Vec::new();
        loop {
            while i < s.len() && (is_space(s[i]) || s[i] == b'/') {
                i += 1;
            }
            if i >= s.len() {
                return None;
            }
            if s[i] == b'>' {
                return Some(Found::Tag { start, end: i + 1, end_tag, name, attrs });
            }
            let an_start = i;
            i += 1;
            while i < s.len() && !is_space(s[i]) && s[i] != b'/' && s[i] != b'>' && s[i] != b'=' {
                i += 1;
            }
            let an = String::from_utf8_lossy(&s[an_start..i]).to_ascii_lowercase();
            while i < s.len() && is_space(s[i]) {
                i += 1;
            }
            let mut value = Span::NONE;
            if i < s.len() && s[i] == b'=' {
                i += 1;
                while i < s.len() && is_space(s[i]) {
                    i += 1;
                }
                if i >= s.len() {
                    return None;
                }
                if s[i] == b'"' || s[i] == b'\'' {
                    let close = self.find_byte(i + 1, s[i]);
                    if close >= s.len() {
                        return None;
                    }
                    value = span(i, close + 1);
                    i = close + 1;
                } else if s[i] != b'>' {
                    let v = i;
                    while i < s.len() && !is_space(s[i]) && s[i] != b'>' {
                        i += 1;
                    }
                    value = span(v, i);
                }
            }
            attrs.push((an, value));
        }
    }

    fn markup_at(&self, at: usize, cdata_is_text: bool) -> Option<Found> {
        let s = self.s;
        if s[at] != b'<' || at + 1 >= s.len() {
            return None;
        }
        let c = s[at + 1];
        if c.is_ascii_alphabetic() {
            return self.tag(at, false);
        }
        if c == b'/' {
            let d = *s.get(at + 2)?;
            if d.is_ascii_alphabetic() {
                return self.tag(at, true);
            }
            if d == b'>' {
                return None;
            }
            let e = self.find_byte(at + 2, b'>');
            return Some(Found::Comment { start: at, end: (e + 1).min(s.len()), text_start: at + 2, text_end: e });
        }
        if c == b'?' {
            let e = self.find_byte(at + 1, b'>');
            return Some(Found::Comment { start: at, end: (e + 1).min(s.len()), text_start: at + 1, text_end: e });
        }
        if c == b'!' {
            if self.starts_ci(at, b"<!--") {
                let body = at + 4;
                if self.starts_ci(body, b">") {
                    return Some(Found::Comment { start: at, end: body + 1, text_start: body, text_end: body });
                }
                if self.starts_ci(body, b"->") {
                    return Some(Found::Comment { start: at, end: body + 2, text_start: body, text_end: body });
                }
                let mut i = body;
                while i < s.len() {
                    if s[i..].starts_with(b"-->") {
                        return Some(Found::Comment { start: at, end: i + 3, text_start: body, text_end: i });
                    }
                    if s[i..].starts_with(b"--!>") {
                        return Some(Found::Comment { start: at, end: i + 4, text_start: body, text_end: i });
                    }
                    i += 1;
                }
                return Some(Found::Comment { start: at, end: s.len(), text_start: body, text_end: s.len() });
            }
            if self.starts_ci(at, b"<!doctype") {
                let e = self.find_byte(at, b'>');
                return Some(Found::Doctype { start: at, end: (e + 1).min(s.len()) });
            }
            if s[at..].starts_with(b"<![CDATA[") && cdata_is_text {
                return None;
            }
            let e = self.find_byte(at + 2, b'>');
            return Some(Found::Comment { start: at, end: (e + 1).min(s.len()), text_start: at + 2, text_end: e });
        }
        None
    }

    fn next_data(&self, cdata_is_text: bool) -> Option<Found> {
        let mut i = self.pos;
        loop {
            i = self.find_byte(i, b'<');
            if i >= self.s.len() {
                return None;
            }
            if self.s[i..].starts_with(b"<![CDATA[") && cdata_is_text {
                let close = self.s[i..].windows(3).position(|w| w == b"]]>").map(|p| i + p + 3).unwrap_or(self.s.len());
                i = close;
                continue;
            }
            if let Some(found) = self.markup_at(i, cdata_is_text) {
                return Some(found);
            }
            i += 1;
        }
    }

    fn next_raw(&self, name: &str) -> Option<Found> {
        let mut i = self.pos;
        loop {
            i = self.find_byte(i, b'<');
            if i >= self.s.len() {
                return None;
            }
            if self.s.get(i + 1) == Some(&b'/') && self.starts_ci(i + 2, name.as_bytes()) {
                let after = i + 2 + name.len();
                if after >= self.s.len() || is_space(self.s[after]) || self.s[after] == b'/' || self.s[after] == b'>' {
                    return self.tag(i, true);
                }
            }
            i += 1;
        }
    }
}

pub fn align(source: &str, marks: &[Mark]) -> Spans {
    let mut out = Spans { ok: true, tags: Vec::with_capacity(marks.len()), runs: Vec::with_capacity(marks.len() + 1) };
    let mut sc = Scanner { s: source.as_bytes(), pos: 0 };
    let mut raw: Option<(Raw, String)> = None;
    for (index, mark) in marks.iter().enumerate() {
        let cdata_is_text = mark.kind != MarkKind::Comment || !mark.comment.starts_with("[CDATA[");
        let found = match &raw {
            Some((Raw::Plaintext, _)) => None,
            Some((_, name)) => sc.next_raw(name),
            None => sc.next_data(cdata_is_text),
        };
        let Some(found) = found else {
            out.ok = false;
            break;
        };
        let (start, end, ok, tag) = match found {
            Found::Tag { start, end, end_tag, name, attrs } => {
                let kind = if end_tag { MarkKind::End } else { MarkKind::Start };
                let mut ok = kind == mark.kind && name == mark.name;
                if ok && kind == MarkKind::Start {
                    ok = mark.attrs.iter().all(|a| attrs.iter().any(|(n, _)| n == a));
                }
                (start, end, ok, Some(TagSpans { tag: span(start, end), attrs }))
            }
            Found::Comment { start, end, text_start, text_end } => {
                let text = &source.as_bytes()[text_start.min(text_end)..text_end];
                let ok = mark.kind == MarkKind::Comment && (text == mark.comment.as_bytes() || mark.comment.contains('\u{FFFD}'));
                (start, end, ok, None)
            }
            Found::Doctype { start, end } => (start, end, mark.kind == MarkKind::Doctype, None),
        };
        if !ok {
            out.ok = false;
            break;
        }
        out.runs.push(span(sc.pos, start));
        sc.pos = end;
        out.tags.push(if mark.kind == MarkKind::Start { tag } else { None });
        raw = match (mark.kind, mark.raw) {
            (MarkKind::Start, Raw::Data) => None,
            (MarkKind::Start, kind) => Some((kind, mark.name.clone())),
            (MarkKind::End, _) => None,
            _ => raw,
        };
        let _ = index;
    }
    if out.ok {
        out.runs.push(span(sc.pos, source.len()));
    } else {
        out.tags.clear();
        out.runs.clear();
    }
    out
}

struct Collect {
    text: RefCell<String>,
}

impl TokenSink for Collect {
    type Handle = ();
    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        match token {
            Token::CharacterTokens(t) => self.text.borrow_mut().push_str(&t),
            Token::NullCharacterToken => self.text.borrow_mut().push('\u{FFFD}'),
            Token::EOFToken | Token::ParseError(_) => {}
            _ => self.text.borrow_mut().push('\u{1}'),
        }
        TokenSinkResult::Continue
    }
}

pub fn text_span(source: &str, s: Span, text: &str) -> Option<Span> {
    let start = s.start as usize;
    let end = start + s.len as usize;
    let raw = source.get(start..end)?;
    if raw == text {
        return Some(s);
    }
    let skip = Span { start: s.start + 1, len: s.len.saturating_sub(1) };
    if raw.len() > 1 && raw.starts_with('\n') && &raw[1..] == text {
        return Some(skip);
    }
    if !raw.contains('&') || raw.contains('<') {
        return None;
    }
    let tokenizer = Tokenizer::new(Collect { text: RefCell::new(String::new()) }, TokenizerOpts::default());
    let queue = BufferQueue::default();
    queue.push_back(StrTendril::from_slice(raw));
    let _ = tokenizer.feed(&queue);
    tokenizer.end();
    let decoded = tokenizer.sink.text.into_inner();
    if decoded == text {
        Some(s)
    } else if decoded.starts_with('\n') && &decoded[1..] == text {
        Some(skip)
    } else {
        None
    }
}
