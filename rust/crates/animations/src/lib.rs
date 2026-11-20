//! TwidgetCore's animator (`core/src/animations.h`, `animations.cpp`): CSS animations and transitions,
//! and the effects of Web Animations (`Element.animate()`), sampled into per-element style overrides.
//! The renderer reads each element's declared animation and transition properties out of the document,
//! hands them to the [`Animator`] after every build, and writes what [`Animator::sample`] returns back as
//! inline declarations. Times are seconds on the renderer's clock unless a name says milliseconds. Text
//! is bytes, as the C++ `std::string`s hold it; every double is where the C++ has it.
//!
//! One thing differs from the C++ by design (rust/PLAN.md, rule 6). The C++ keeps two caches per thread
//! (parsed timing functions, tokenised value pairs); here they are a [`Caches`] object that the helper
//! shares between its animators as the C++ threads share theirs, with the same bounds and the same keys.
//! The per-element maps are ordered by element id on both sides, which is the order of events and motions.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use twidget_css as css;
use twidget_json::Json;
use twidget_paint::{AnimationPaint, BoxPaint};

const INF: f64 = f64::INFINITY;
/// A retargeted transition starts from its interpolated value only while that is this short: one mixed by weight holds both its ends.
const MAX_RETARGET_VALUE: usize = 16384;

/// (property, value) in the order they apply; a later entry of the same property wins.
pub type StyleValues = Vec<(Vec<u8>, Vec<u8>)>;
/// Animated declarations per widget node id.
pub type StyleOverrides = BTreeMap<i64, StyleValues>;
/// An element's declared values of the watched properties, as written.
pub type Declared = BTreeMap<Vec<u8>, Vec<u8>>;

// `std::max`, `std::min` and `std::clamp` as the C++ has them: the first argument inside a tie, a NaN
// where the comparisons leave it.
fn max(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}

fn min(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if hi < v {
        hi
    } else if v < lo {
        lo
    } else {
        v
    }
}

fn is_digit(c: u8) -> bool {
    c.is_ascii_digit()
}

fn is_letter(c: u8) -> bool {
    c.is_ascii_alphabetic()
}

fn find(s: &[u8], c: u8) -> Option<usize> {
    s.iter().position(|&x| x == c)
}

fn find_from(s: &[u8], c: u8, from: usize) -> Option<usize> {
    s.get(from..)?.iter().position(|&x| x == c).map(|p| p + from)
}

fn rfind(s: &[u8], c: u8) -> Option<usize> {
    s.iter().rposition(|&x| x == c)
}

fn concat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

fn number_text(value: f64, digits: i32) -> Vec<u8> {
    css::format_number(value, digits).into_bytes()
}

/// MSVC's `std::hash` of a string: FNV-1a over its bytes (libstdc++ and libc++ hash otherwise; only
/// equality of the results matters to the renderer).
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = 14695981039346656037u64;
    for &c in bytes {
        h = (h ^ u64::from(c)).wrapping_mul(1099511628211);
    }
    h
}

fn initial_value(property: &[u8]) -> Option<&'static [u8]> {
    Some(match property {
        b"opacity" => b"1",
        b"transform" => b"none",
        b"translate" => b"0px",
        b"scale" => b"1",
        b"rotate" => b"0deg",
        b"filter" => b"none",
        b"letter-spacing" => b"0px",
        _ => return None,
    })
}

fn merge(into: &mut StyleValues, property: &[u8], value: Vec<u8>) {
    into.retain(|(p, _)| p.as_slice() != property);
    into.push((property.to_vec(), value));
}

fn last_value<'a>(values: &'a StyleValues, property: &[u8]) -> Option<&'a Vec<u8>> {
    values.iter().rev().find(|(p, _)| p.as_slice() == property).map(|(_, v)| v)
}

fn bezier_at(x1: f64, y1: f64, x2: f64, y2: f64, x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let cx = 3.0 * x1;
    let bx = 3.0 * (x2 - x1) - cx;
    let ax = 1.0 - cx - bx;
    let cy = 3.0 * y1;
    let by = 3.0 * (y2 - y1) - cy;
    let ay = 1.0 - cy - by;
    let sample_x = |t: f64| ((ax * t + bx) * t + cx) * t;
    let sample_y = |t: f64| ((ay * t + by) * t + cy) * t;
    let derivative_x = |t: f64| (3.0 * ax * t + 2.0 * bx) * t + cx;
    let mut t = x;
    for _ in 0..8 {
        let error = sample_x(t) - x;
        if error.abs() < 1e-6 {
            return sample_y(t);
        }
        let d = derivative_x(t);
        if d.abs() < 1e-6 {
            break;
        }
        t -= error / d;
    }
    let mut lo = 0.0;
    let mut hi = 1.0;
    t = x;
    for _ in 0..30 {
        let value = sample_x(t);
        if (value - x).abs() < 1e-6 {
            break;
        }
        if value < x {
            lo = t;
        } else {
            hi = t;
        }
        t = (lo + hi) / 2.0;
    }
    sample_y(t)
}

fn linear_function(body: &[u8]) -> Option<Timing> {
    struct Point {
        output: f64,
        input: Option<f64>,
    }
    let mut points: Vec<Point> = Vec::new();
    for item in css::split_top_level(body, b',') {
        let mut parts: Vec<Vec<u8>> = Vec::new();
        for p in css::split_top_level(&item, b' ') {
            let t = css::trim(&p);
            if !t.is_empty() {
                parts.push(t);
            }
        }
        if parts.is_empty() {
            return None;
        }
        let output = css::number(&parts[0])?;
        if parts.len() == 1 {
            points.push(Point { output, input: None });
        }
        for part in &parts[1..] {
            if !part.ends_with(b"%") {
                return None;
            }
            let input = css::number(&part[..part.len() - 1])?;
            points.push(Point { output, input: Some(input / 100.0) });
        }
        if points.len() > 10000 {
            return None;
        }
    }
    let mut t = Timing { kind: Kind::Points, ..Timing::default() };
    if points.is_empty() {
        return None;
    }
    if points.len() == 1 {
        t.points = vec![(0.0, points[0].output), (1.0, points[0].output)];
        return Some(t);
    }
    if points[0].input.is_none() {
        points[0].input = Some(0.0);
    }
    let last = points.len() - 1;
    if points[last].input.is_none() {
        let mut largest = 1.0;
        for p in &points {
            if let Some(input) = p.input {
                largest = max(largest, input);
            }
        }
        points[last].input = Some(largest);
    }
    let mut largest = points[0].input.unwrap_or(0.0);
    for p in &mut points {
        if let Some(input) = p.input {
            largest = max(largest, input);
            p.input = Some(largest);
        }
    }
    let mut i = 0;
    while i < points.len() {
        if points[i].input.is_some() {
            i += 1;
            continue;
        }
        let start = i - 1;
        let mut end = i;
        while points[end].input.is_none() {
            end += 1;
        }
        let from = points[start].input.unwrap_or(0.0);
        let to = points[end].input.unwrap_or(0.0);
        for k in start + 1..end {
            points[k].input = Some(from + (to - from) * (k - start) as f64 / (end - start) as f64);
        }
        i = end + 1;
    }
    t.points = points.iter().map(|p| (p.input.unwrap_or(0.0), p.output)).collect();
    Some(t)
}

// ── interpolation tokens ──

#[derive(Clone, Debug, Default, PartialEq)]
struct Token {
    number: bool,
    value: f64,
    /// The unit of a number, or the text between numbers.
    text: Vec<u8>,
}

fn tokenize(s: &[u8]) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut pending: Vec<u8> = Vec::new();
    let n = s.len();
    let mut i = 0;
    while i < n {
        let c = s[i];
        let starts = is_digit(c)
            || (c == b'.' && i + 1 < n && is_digit(s[i + 1]))
            || ((c == b'-' || c == b'+') && i + 1 < n && (is_digit(s[i + 1]) || s[i + 1] == b'.'));
        let previous = if i > 0 { s[i - 1] } else { 0 };
        let ident_part = i > 0 && (is_letter(previous) || (previous == b'-' && i > 1 && is_letter(s[i - 2])));
        if starts && !(ident_part && c != b'-') && previous != b'#' {
            let mut j = i + 1;
            while j < n
                && (is_digit(s[j])
                    || s[j] == b'.'
                    || ((s[j] == b'e' || s[j] == b'E') && j + 1 < n && (is_digit(s[j + 1]) || s[j + 1] == b'-')))
            {
                if s[j] == b'e' || s[j] == b'E' {
                    j += 1;
                }
                j += 1;
            }
            let mut k = j;
            while k < n && (is_letter(s[k]) || s[k] == b'%') {
                k += 1;
            }
            if let Some(v) = css::number(&s[i..j]) {
                if !pending.is_empty() {
                    tokens.push(Token { number: false, value: 0.0, text: std::mem::take(&mut pending) });
                }
                tokens.push(Token { number: true, value: v, text: s[j..k].to_vec() });
                i = k;
                continue;
            }
        }
        pending.push(c);
        i += 1;
    }
    if !pending.is_empty() {
        tokens.push(Token { number: false, value: 0.0, text: pending });
    }
    tokens
}

/// (name lowercased, whole `name(args)` text) of each function in a list.
type Functions = Vec<(Vec<u8>, Vec<u8>)>;

/// The functions of a transform list, and what follows them.
fn functions(transform: &[u8]) -> (Functions, Vec<u8>) {
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(open) = find_from(transform, b'(', pos) {
        // The matching parenthesis: an argument may hold calc() and its own parentheses.
        let mut close = open + 1;
        let mut depth = 1;
        while close < transform.len() {
            if transform[close] == b'(' {
                depth += 1;
            } else if transform[close] == b')' {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            close += 1;
        }
        if close == transform.len() {
            break;
        }
        out.push((css::lower(&css::trim(&transform[pos..open])), css::trim(&transform[pos..close + 1])));
        pos = close + 1;
    }
    (out, transform[pos..].to_vec())
}

/// Each function's arguments.
fn arguments(f: &[u8]) -> Vec<Vec<u8>> {
    let (Some(open), Some(close)) = (find(f, b'('), rfind(f, b')')) else { return Vec::new() };
    if close < open {
        return Vec::new();
    }
    css::split_top_level(&f[open + 1..close], b',').iter().map(|a| css::trim(a)).collect()
}

/// One-argument shorthands rewritten to their two-argument forms, so `translateX(4px)` pairs with `translate(0, 8px)`.
fn primitive(transform: &[u8]) -> Option<Vec<u8>> {
    let (list, rest) = functions(transform);
    if list.is_empty() || !css::trim(&rest).is_empty() {
        return None;
    }
    let mut out: Vec<u8> = Vec::new();
    for (name, whole) in &list {
        let args = arguments(whole);
        let mut item: Vec<u8> = Vec::new();
        if args.len() == 1 {
            let a = &args[0][..];
            item = match name.as_slice() {
                b"translatex" | b"translate" => concat(&[b"translate(", a, b", 0)"]),
                b"translatey" => concat(&[b"translate(0, ", a, b")"]),
                b"scalex" => concat(&[b"scale(", a, b", 1)"]),
                b"scaley" => concat(&[b"scale(1, ", a, b")"]),
                b"scale" => concat(&[b"scale(", a, b", ", a, b")"]),
                b"skewx" | b"skew" => concat(&[b"skew(", a, b", 0)"]),
                b"skewy" => concat(&[b"skew(0, ", a, b")"]),
                _ => Vec::new(),
            };
        }
        if item.is_empty() {
            item = name.clone();
            item.push(b'(');
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    item.extend_from_slice(b", ");
                }
                item.extend_from_slice(arg);
            }
            item.push(b')');
        }
        if !out.is_empty() {
            out.push(b' ');
        }
        out.extend_from_slice(&item);
    }
    Some(out)
}

/// The identity of each function in a transform or filter list: `rotate(30deg)` → `rotate(0deg)`.
fn identity(transform: &[u8]) -> Vec<u8> {
    const ONES: [&[u8]; 8] = [b"scale", b"scalex", b"scaley", b"scale3d", b"brightness", b"contrast", b"opacity", b"saturate"];
    let (list, _) = functions(transform);
    if list.is_empty() {
        return transform.to_vec();
    }
    let mut out: Vec<u8> = Vec::new();
    for (name, whole) in &list {
        let open = find(whole, b'(').unwrap_or(0);
        let close = rfind(whole, b')').unwrap_or(whole.len());
        let original = &whole[..open];
        let inner = whole.get(open + 1..close).unwrap_or_default();
        let mut args: Vec<u8> = Vec::new();
        let mut first = true;
        for arg in css::split_top_level(inner, b',') {
            let mut lead = 0;
            while lead < arg.len() && (arg[lead] == b' ' || arg[lead] == b'\t') {
                lead += 1;
            }
            let value: Vec<u8> = if ONES.contains(&name.as_slice()) {
                b"1".to_vec()
            } else if find(&arg, b'(').is_some() {
                b"0".to_vec()
            } else {
                let t = css::trim(&arg);
                let mut u = 0;
                while u < t.len() && (is_digit(t[u]) || t[u] == b'.' || t[u] == b'-' || t[u] == b'+') {
                    u += 1;
                }
                concat(&[b"0", &t[u..]])
            };
            if !first {
                args.push(b',');
            }
            args.extend_from_slice(&arg[..lead]);
            args.extend_from_slice(&value);
            first = false;
        }
        if !out.is_empty() {
            out.push(b' ');
        }
        out.extend_from_slice(original);
        out.push(b'(');
        out.extend_from_slice(&args);
        out.push(b')');
    }
    out
}

/// A shorter transform list padded with the identities of the longer one's extra functions.
fn padded(a: &[u8], b: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let (x, _) = functions(a);
    let (y, _) = functions(b);
    let a_short = x.len() < y.len();
    let (shorter, longer) = if a_short { (&x, &y) } else { (&y, &x) };
    if shorter.is_empty() && longer.is_empty() {
        return None;
    }
    for i in 0..shorter.len() {
        if shorter[i].0 != longer[i].0 {
            return None;
        }
    }
    let mut extra: Vec<u8> = Vec::new();
    for item in &longer[shorter.len()..] {
        if !extra.is_empty() {
            extra.push(b' ');
        }
        extra.extend_from_slice(&identity(&item.1));
    }
    if a_short {
        return Some((concat(&[a, b" ", &extra]), b.to_vec()));
    }
    Some((a.to_vec(), concat(&[b, b" ", &extra])))
}

fn translation_list(text: &[u8]) -> bool {
    let t = css::lower(&css::trim(text));
    let (list, rest) = functions(&t);
    if list.is_empty() || !css::trim(&rest).is_empty() {
        return false;
    }
    list.iter().all(|(name, _)| matches!(name.as_slice(), b"translate" | b"translatex" | b"translatey"))
}

fn scaled_tokens(text: &[u8], factor: f64) -> Vec<u8> {
    let mut out = Vec::new();
    for t in tokenize(text) {
        if t.number {
            out.extend_from_slice(&number_text(t.value * factor, 4));
        }
        out.extend_from_slice(&t.text);
    }
    out
}

/// A translation list `factor` times as long: a length has its number scaled, math is scaled inside a
/// calc() (scaling its numbers would scale a `*` operand too).
fn scaled_translations(text: &[u8], factor: f64) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    for (_, whole) in functions(text).0 {
        let mut args: Vec<u8> = Vec::new();
        for arg in arguments(&whole) {
            let scaled = if find(&arg, b'(').is_none() {
                scaled_tokens(&arg, factor)
            } else {
                concat(&[b"calc((", &arg, b") * ", &number_text(factor, 4), b")"])
            };
            if !args.is_empty() {
                args.extend_from_slice(b", ");
            }
            args.extend_from_slice(&scaled);
        }
        if !out.is_empty() {
            out.push(b' ');
        }
        let open = find(&whole, b'(').unwrap_or(whole.len());
        out.extend_from_slice(&whole[..open]);
        out.push(b'(');
        out.extend_from_slice(&args);
        out.push(b')');
    }
    out
}

type TokenPair = (Vec<Token>, Vec<Token>);

/// The C++'s per-thread cache of tokenised value pairs (`pair_tokens`): keyed by `a`, a 0x01 byte and
/// `b`, so two pairs that join to the same key share an entry, as they do there; cleared at 2048
/// entries or 256 KiB of keys, a key past 4096 bytes never kept.
#[derive(Debug, Default)]
pub struct PairCache {
    pairs: HashMap<Vec<u8>, Option<TokenPair>>,
    bytes: usize,
}

impl PairCache {
    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    /// The bytes of the keys it holds.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    fn tokens(&mut self, a: &[u8], b: &[u8]) -> Option<TokenPair> {
        let key = concat(&[a, b"\x01", b]);
        if let Some(found) = self.pairs.get(&key) {
            return found.clone();
        }
        let mut from = css::normalize_colors(a);
        let mut to = css::normalize_colors(b);
        if css::iequals(&from, b"none") && find(&to, b'(').is_some() {
            from = identity(&to);
        } else if css::iequals(&to, b"none") && find(&from, b'(').is_some() {
            to = identity(&from);
        }
        let mut left = tokenize(&from);
        let mut right = tokenize(&to);
        if left.len() != right.len()
            && let (Some(pa), Some(pb)) = (primitive(&from), primitive(&to))
        {
            left = tokenize(&pa);
            right = tokenize(&pb);
            if left.len() != right.len()
                && let Some((p, q)) = padded(&pa, &pb)
            {
                left = tokenize(&p);
                right = tokenize(&q);
            }
        }
        let result = (left.len() == right.len()).then_some((left, right));
        if key.len() <= 4096 {
            // Its tokens take some forty times a pair's text: the cache holds those of 256 K of pairs at most.
            if self.pairs.len() >= 2048 || self.bytes + key.len() > 256 * 1024 {
                self.pairs.clear();
                self.bytes = 0;
            }
            self.bytes += key.len();
            self.pairs.insert(key, result.clone());
        }
        result
    }
}

/// The C++'s per-thread cache of parsed timing functions (`Timing::parse`): a document repeats a handful;
/// cleared at 256.
#[derive(Debug, Default)]
pub struct TimingCache {
    parsed: HashMap<Vec<u8>, Option<Timing>>,
}

impl TimingCache {
    pub fn len(&self) -> usize {
        self.parsed.len()
    }

    pub fn is_empty(&self) -> bool {
        self.parsed.is_empty()
    }
}

/// What the C++ keeps per thread for every animator on it.
#[derive(Debug, Default)]
pub struct Caches {
    pub timings: TimingCache,
    pub pairs: PairCache,
}

/// [`Caches`] as the helper shares them between its animators.
pub type SharedCaches = Arc<Mutex<Caches>>;

fn lock(caches: &Mutex<Caches>) -> MutexGuard<'_, Caches> {
    caches.lock().unwrap_or_else(PoisonError::into_inner)
}

// ── Web Animations composite (add / accumulate) ──

fn web_tokenize(s: &[u8]) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut pending: Vec<u8> = Vec::new();
    let n = s.len();
    let mut i = 0;
    while i < n {
        let c = s[i];
        let next = if i + 1 < n { s[i + 1] } else { 0 };
        let previous = if i > 0 { s[i - 1] } else { 0 };
        let starts = is_digit(c) || (c == b'.' && is_digit(next)) || ((c == b'-' || c == b'+') && (is_digit(next) || next == b'.'));
        let in_word = i > 0 && (is_letter(previous) || previous == b'#' || (previous == b'-' && c != b'-'));
        if starts && !in_word {
            let mut j = i + 1;
            while j < n
                && (is_digit(s[j])
                    || s[j] == b'.'
                    || ((s[j] == b'e' || s[j] == b'E') && j + 1 < n && (is_digit(s[j + 1]) || s[j + 1] == b'-')))
            {
                j += if s[j] == b'e' || s[j] == b'E' { 2 } else { 1 };
            }
            let mut k = j;
            while k < n && (is_letter(s[k]) || s[k] == b'%') {
                k += 1;
            }
            if let Some(v) = css::number(&s[i..j]) {
                if !pending.is_empty() {
                    tokens.push(Token { number: false, value: 0.0, text: std::mem::take(&mut pending) });
                }
                tokens.push(Token { number: true, value: v, text: s[j..k].to_vec() });
                i = k;
                continue;
            }
        }
        pending.push(c);
        i += 1;
    }
    if !pending.is_empty() {
        tokens.push(Token { number: false, value: 0.0, text: pending });
    }
    tokens
}

fn web_render(tokens: &[Token]) -> Vec<u8> {
    let mut out = Vec::new();
    for t in tokens {
        if t.number {
            out.extend_from_slice(&number_text(t.value, 6));
        }
        out.extend_from_slice(&t.text);
    }
    out
}

/// The function a text token opens: `"rotate("` → `rotate`, `") scale("` → `scale`.
fn opened_function(text: &[u8]) -> Option<Vec<u8>> {
    let open = rfind(text, b'(')?;
    let before = &text[..open];
    let start = before.iter().rposition(|&c| c == b' ' || c == b',');
    Some(css::lower(match start {
        None => before,
        Some(s) => &before[s + 1..],
    }))
}

fn unit_scale(f: &[u8]) -> bool {
    matches!(f, b"scale" | b"scalex" | b"scaley" | b"scale3d")
}

fn web_combine(a: &[u8], b: &[u8], scale_aware: bool) -> Option<Vec<u8>> {
    let left = web_tokenize(&css::normalize_colors(a));
    let right = web_tokenize(&css::normalize_colors(b));
    if left.len() != right.len() {
        return None;
    }
    let mut out = Vec::with_capacity(left.len());
    let mut function: Vec<u8> = Vec::new();
    let mut color_channel: i32 = -1;
    for (x, y) in left.iter().zip(&right) {
        if !x.number && !y.number && css::iequals(&x.text, &y.text) {
            out.push(x.clone());
            if let Some(f) = opened_function(&x.text) {
                function = f;
                color_channel = if function == b"rgba" { 0 } else { -1 };
            } else if color_channel >= 0 {
                color_channel += 1;
            }
        } else if x.number && y.number && (x.text == y.text || x.value == 0.0 || y.value == 0.0) {
            let mut value = x.value + y.value;
            if scale_aware && unit_scale(&function) {
                value = x.value + y.value - 1.0;
            }
            if color_channel >= 0 {
                value = if color_channel < 3 { clamp(value, 0.0, 255.0) } else { clamp(value, 0.0, 1.0) };
            }
            out.push(Token { number: true, value, text: if x.text.is_empty() { y.text.clone() } else { x.text.clone() } });
        } else {
            return None;
        }
    }
    Some(web_render(&out))
}

fn web_add(underlying: &[u8], value: &[u8], property: &[u8], accumulate: bool) -> Vec<u8> {
    if property == b"transform" {
        let u = css::trim(underlying);
        let v = css::trim(value);
        if u.is_empty() || css::iequals(&u, b"none") {
            return v;
        }
        if v.is_empty() || css::iequals(&v, b"none") {
            return u;
        }
        if accumulate && let Some(c) = web_combine(&u, &v, true) {
            return c;
        }
        return concat(&[&u, b" ", &v]);
    }
    if let Some(c) = web_combine(underlying, value, false) {
        return c;
    }
    value.to_vec()
}

fn web_scaled(value: &[u8], factor: f64) -> Vec<u8> {
    let mut function: Vec<u8> = Vec::new();
    let mut tokens = web_tokenize(&css::normalize_colors(value));
    for t in &mut tokens {
        if !t.number {
            if let Some(f) = opened_function(&t.text) {
                function = f;
            }
        } else if unit_scale(&function) {
            t.value = (t.value - 1.0) * factor + 1.0;
        } else {
            t.value *= factor;
        }
    }
    web_render(&tokens)
}

// ── Web Animations timing ──

#[derive(Clone, Copy, Debug, Default)]
struct Computed {
    before: bool,
    after: bool,
    active_time: Option<f64>,
    progress: f64,
    iteration: f64,
}

fn compute(a: &WebAnimation, local_time: Option<f64>, forwards: bool) -> Option<Computed> {
    let local = local_time?;
    if !local.is_finite() {
        return None;
    }
    let active = if a.duration == 0.0 || a.iterations == 0.0 { 0.0 } else { a.duration * a.iterations };
    let end = max(a.delay + active + a.end_delay, 0.0);
    let before_active = max(min(a.delay, end), 0.0);
    let active_after = max(min(a.delay + active, end), 0.0);
    let fills_backwards = a.fill == b"backwards" || a.fill == b"both";
    let fills_forwards = a.fill == b"forwards" || a.fill == b"both";
    let mut r = Computed::default();
    if local < before_active || (!forwards && local == before_active) {
        r.before = true;
    } else if local > active_after || (forwards && local == active_after) {
        r.after = true;
    }
    r.active_time = if r.before {
        fills_backwards.then(|| max(local - a.delay, 0.0))
    } else if r.after {
        fills_forwards.then(|| max(min(local - a.delay, active), 0.0))
    } else {
        Some(local - a.delay)
    };
    let Some(active_time) = r.active_time else { return Some(r) };
    let mut overall = if a.duration == 0.0 { if r.before { 0.0 } else { a.iterations } } else { active_time / a.duration };
    overall += a.iteration_start;
    let mut simple = if overall.is_infinite() { a.iteration_start % 1.0 } else { overall % 1.0 };
    if simple == 0.0 && !r.before && active_time == active && a.iterations != 0.0 {
        simple = 1.0;
    }
    let iteration = if r.after && a.iterations.is_infinite() {
        INF
    } else if simple == 1.0 {
        overall.floor() - 1.0
    } else {
        overall.floor()
    };
    r.iteration = iteration;
    let mut forwards_direction = true;
    if a.direction == b"reverse" {
        forwards_direction = false;
    } else if (a.direction == b"alternate" || a.direction == b"alternate-reverse") && iteration.is_finite() {
        let d = iteration + if a.direction == b"alternate-reverse" { 1.0 } else { 0.0 };
        forwards_direction = d % 2.0 == 0.0;
    }
    let directed = if forwards_direction { simple } else { 1.0 - simple };
    let before_flag = (forwards_direction && r.before) || (!forwards_direction && r.after);
    r.progress = a.easing.web(directed, before_flag);
    Some(r)
}

fn local_time(a: &WebAnimation, now: f64) -> Option<f64> {
    if let Some(hold) = a.hold_ms {
        return Some(hold);
    }
    let start = a.start_ms?;
    Some((now * 1000.0 - start) * a.rate)
}

// web_value's implicit keyframes ease linearly; only their curve is read, never their text.
static LINEAR: Timing =
    Timing { kind: Kind::Linear, jump: Jump::JumpEnd, x1: 0.0, y1: 0.0, x2: 1.0, y2: 1.0, steps: 1, points: Vec::new(), text: Vec::new() };

fn web_value(
    pairs: &mut PairCache,
    a: &WebAnimation,
    property: &[u8],
    progress: f64,
    iteration: f64,
    underlying: &dyn Fn(&[u8]) -> Vec<u8>,
) -> Option<Vec<u8>> {
    struct F<'a> {
        offset: f64,
        value: Option<&'a [u8]>,
        easing: &'a Timing,
        composite: &'a [u8],
    }
    let mut frames: Vec<F> = Vec::new();
    for frame in &a.frames {
        if let Some(v) = last_value(&frame.values, property) {
            frames.push(F {
                offset: frame.offset,
                value: Some(v),
                easing: &frame.easing,
                composite: if frame.composite.is_empty() { &a.composite } else { &frame.composite },
            });
        }
    }
    if frames.is_empty() {
        return None;
    }
    if !frames.iter().any(|f| f.offset == 0.0) {
        frames.insert(0, F { offset: 0.0, value: None, easing: &LINEAR, composite: b"replace" });
    }
    if !frames.iter().any(|f| f.offset == 1.0) {
        frames.push(F { offset: 1.0, value: None, easing: &LINEAR, composite: b"replace" });
    }
    let base = underlying(property);
    let last = frames.iter().rev().find(|f| f.offset == 1.0).and_then(|f| f.value);
    let resolve = |index: usize| -> Vec<u8> {
        let frame = &frames[index];
        let Some(own) = frame.value else { return base.clone() };
        let mut value = own.to_vec();
        if a.iteration_composite == b"accumulate"
            && iteration > 0.0
            && iteration.is_finite()
            && let Some(last) = last
        {
            value = web_add(&web_scaled(last, iteration), &value, property, true);
        }
        if frame.composite == b"add" {
            return web_add(&base, &value, property, false);
        }
        if frame.composite == b"accumulate" {
            return web_add(&base, &value, property, true);
        }
        value
    };
    let zeros: Vec<usize> = (0..frames.len()).filter(|&i| frames[i].offset == 0.0).collect();
    let ones: Vec<usize> = (0..frames.len()).filter(|&i| frames[i].offset == 1.0).collect();
    if progress < 0.0 && zeros.len() > 1 {
        return Some(resolve(zeros[0]));
    }
    if progress >= 1.0 && ones.len() > 1 {
        return Some(resolve(ones[ones.len() - 1]));
    }
    let mut start = 0;
    if progress < 0.0 {
        start = 0;
    } else if progress >= 1.0 {
        start = frames.len() - 2;
    } else {
        for (i, f) in frames.iter().enumerate() {
            if f.offset <= progress && f.offset < 1.0 {
                start = i;
            }
        }
    }
    let end = (start + 1).min(frames.len() - 1);
    if start == end {
        return Some(resolve(start));
    }
    let (fa, fb) = (&frames[start], &frames[end]);
    let span = fb.offset - fa.offset;
    let local = if span == 0.0 { 0.0 } else { (progress - fa.offset) / span };
    let from = resolve(start);
    let to = resolve(end);
    Some(interpolate(pairs, &from, &to, fa.easing.web(local, false)))
}

// ───────────────────────── Timing ─────────────────────────

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Linear,
    Bezier,
    Steps,
    Points,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
pub enum Jump {
    #[default]
    JumpEnd,
    JumpStart,
    JumpBoth,
    JumpNone,
}

/// An easing function. [`Timing::css`] follows CSS animations and transitions (clamped to [0, 1]
/// input); [`Timing::web`] is the Web Animations form with the before flag and extrapolated
/// cubic-béziers.
#[derive(Clone, Debug, PartialEq)]
pub struct Timing {
    pub kind: Kind,
    pub jump: Jump,
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub steps: i32,
    /// linear(): (input, output), inputs non-decreasing.
    pub points: Vec<(f64, f64)>,
    pub text: Vec<u8>,
}

impl Default for Timing {
    fn default() -> Timing {
        Timing {
            kind: Kind::Linear,
            jump: Jump::JumpEnd,
            x1: 0.0,
            y1: 0.0,
            x2: 1.0,
            y2: 1.0,
            steps: 1,
            points: Vec::new(),
            text: b"linear".to_vec(),
        }
    }
}

impl Timing {
    pub fn ease() -> Timing {
        Timing { kind: Kind::Bezier, x1: 0.25, y1: 0.1, x2: 0.25, y2: 1.0, text: b"ease".to_vec(), ..Timing::default() }
    }

    /// [`Timing::parse_uncached`] through the cache, as the C++ parses each text once per thread.
    pub fn parse(cache: &mut TimingCache, raw: &[u8]) -> Option<Timing> {
        if let Some(found) = cache.parsed.get(raw) {
            return found.clone();
        }
        if cache.parsed.len() >= 256 {
            cache.parsed.clear();
        }
        let timing = Timing::parse_uncached(raw);
        cache.parsed.insert(raw.to_vec(), timing.clone());
        timing
    }

    pub fn parse_uncached(raw: &[u8]) -> Option<Timing> {
        let t = css::lower(&css::trim(raw));
        let mut out = Timing { text: t.clone(), ..Timing::default() };
        let bezier = |mut out: Timing, a: f64, b: f64, c: f64, d: f64| {
            out.kind = Kind::Bezier;
            (out.x1, out.y1, out.x2, out.y2) = (a, b, c, d);
            Some(out)
        };
        match t.as_slice() {
            b"linear" => {
                out.kind = Kind::Linear;
                return Some(out);
            }
            b"ease" => return bezier(out, 0.25, 0.1, 0.25, 1.0),
            b"ease-in" => return bezier(out, 0.42, 0.0, 1.0, 1.0),
            b"ease-out" => return bezier(out, 0.0, 0.0, 0.58, 1.0),
            b"ease-in-out" => return bezier(out, 0.42, 0.0, 0.58, 1.0),
            b"step-start" | b"step-end" => {
                out.kind = Kind::Steps;
                out.steps = 1;
                out.jump = if t == b"step-start" { Jump::JumpStart } else { Jump::JumpEnd };
                return Some(out);
            }
            _ => {}
        }
        if t.starts_with(b"cubic-bezier(") && t.ends_with(b")") {
            let parts = css::split_top_level(&t[13..t.len() - 1], b',');
            if parts.len() != 4 {
                return None;
            }
            let mut v = [0.0; 4];
            for i in 0..4 {
                v[i] = css::number(&parts[i])?;
            }
            return bezier(out, v[0], v[1], v[2], v[3]);
        }
        if t.starts_with(b"steps(") && t.ends_with(b")") {
            let parts = css::split_top_level(&t[6..t.len() - 1], b',');
            if parts.is_empty() || parts.len() > 2 {
                return None;
            }
            let n = css::number(&parts[0])?;
            if n < 1.0 || n != n.floor() || n > 100000.0 {
                return None;
            }
            let position = if parts.len() > 1 { css::trim(&parts[1]) } else { b"end".to_vec() };
            out.kind = Kind::Steps;
            out.steps = n as i32;
            out.jump = match position.as_slice() {
                b"start" | b"jump-start" => Jump::JumpStart,
                b"end" | b"jump-end" => Jump::JumpEnd,
                b"jump-both" => Jump::JumpBoth,
                b"jump-none" => Jump::JumpNone,
                _ => return None,
            };
            return Some(out);
        }
        if t.starts_with(b"linear(") && t.ends_with(b")") {
            let mut f = linear_function(&t[7..t.len() - 1])?;
            f.text = t;
            return Some(f);
        }
        None
    }

    pub fn css(&self, x: f64) -> f64 {
        match self.kind {
            Kind::Linear => x,
            Kind::Bezier => bezier_at(self.x1, self.y1, self.x2, self.y2, x),
            Kind::Steps => {
                let n = f64::from(self.steps);
                let step = if self.jump == Jump::JumpStart { (x * n).ceil() } else { (x * n).floor() };
                clamp(step / n, 0.0, 1.0)
            }
            Kind::Points => {
                let p = &self.points;
                // linear() always has two points or more; a Timing made by hand with fewer is the identity.
                if p.len() < 2 {
                    return x;
                }
                let segment = |a: usize, b: usize| {
                    if p[b].0 == p[a].0 {
                        return if x <= p[a].0 { p[a].1 } else { p[b].1 };
                    }
                    p[a].1 + (p[b].1 - p[a].1) * (x - p[a].0) / (p[b].0 - p[a].0)
                };
                if x <= p[0].0 {
                    return if p[1].0 == p[0].0 { p[0].1 } else { segment(0, 1) };
                }
                for k in 1..p.len() {
                    if x <= p[k].0 {
                        return if p[k].0 == p[k - 1].0 { p[k].1 } else { segment(k - 1, k) };
                    }
                }
                let n = p.len();
                if p[n - 1].0 == p[n - 2].0 { p[n - 1].1 } else { segment(n - 2, n - 1) }
            }
        }
    }

    pub fn web(&self, x: f64, before: bool) -> f64 {
        if self.kind == Kind::Steps {
            let n = f64::from(self.steps);
            let mut step = (x * n).floor();
            if self.jump == Jump::JumpStart || self.jump == Jump::JumpBoth {
                step += 1.0;
            }
            if before && (x * n) % 1.0 == 0.0 {
                step -= 1.0;
            }
            if x >= 0.0 && step < 0.0 {
                step = 0.0;
            }
            let jumps = match self.jump {
                Jump::JumpBoth => n + 1.0,
                Jump::JumpNone => n - 1.0,
                _ => n,
            };
            if x <= 1.0 && step > jumps {
                step = jumps;
            }
            return step / jumps;
        }
        if self.kind == Kind::Bezier {
            // Outside [0, 1] a cubic-bézier continues along its end tangents.
            let (x1, y1, x2, y2) = (self.x1, self.y1, self.x2, self.y2);
            if x < 0.0 {
                let slope = if x1 > 0.0 {
                    y1 / x1
                } else if y1 == 0.0 && x2 > 0.0 {
                    y2 / x2
                } else {
                    0.0
                };
                return slope * x;
            }
            if x > 1.0 {
                let slope = if x2 < 1.0 {
                    (y2 - 1.0) / (x2 - 1.0)
                } else if y2 == 1.0 && x1 < 1.0 {
                    (y1 - 1.0) / (x1 - 1.0)
                } else {
                    0.0
                };
                return 1.0 + slope * (x - 1.0);
            }
        }
        self.css(x)
    }
}

// ───────────────────────── keyframes and specs ─────────────────────────

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Keyframe {
    pub offset: f64,
    pub declarations: StyleValues,
    pub timing: Option<Timing>,
}

pub type KeyframeRules = HashMap<Vec<u8>, Vec<Keyframe>>;

/// Collects every `@keyframes` (and `@-webkit-keyframes`) block of a style sheet.
pub fn parse_keyframes(timings: &mut TimingCache, css_text: &[u8], rules: &mut KeyframeRules) {
    let mut pos = 0;
    loop {
        let mut at = None;
        let mut i = pos;
        while i + 9 <= css_text.len() {
            if css::iequals(&css_text[i..i + 9], b"keyframes") {
                at = Some(i);
                break;
            }
            i += 1;
        }
        let Some(at) = at else { return };
        let before = &css_text[..at];
        let open = find_from(css_text, b'{', at + 9);
        let Some(open) = open.filter(|_| before.ends_with(b"@") || before.ends_with(b"@-webkit-")) else {
            pos = at + 9;
            continue;
        };
        let mut name = css::trim(&css_text[at + 9..open]);
        let lead = name.iter().take_while(|&&c| c == b'"' || c == b'\'').count();
        name.drain(..lead);
        while name.last().is_some_and(|&c| c == b'"' || c == b'\'') {
            name.pop();
        }
        let mut depth = 0;
        let mut close = None;
        for (i, &c) in css_text.iter().enumerate().skip(open) {
            if c == b'{' {
                depth += 1;
            } else if c == b'}' {
                depth -= 1;
                if depth == 0 {
                    close = Some(i);
                    break;
                }
            }
        }
        let Some(close) = close else { return };
        let body = &css_text[open + 1..close];
        let mut frames: Vec<Keyframe> = Vec::new();
        let mut p = 0;
        while let Some(fo) = find_from(body, b'{', p) {
            let Some(fc) = find_from(body, b'}', fo) else { break };
            let mut declarations: StyleValues = Vec::new();
            let mut timing: Option<Timing> = None;
            for d in css::parse_declarations(&body[fo + 1..fc]) {
                if d.name == b"animation-timing-function" {
                    timing = Timing::parse(timings, &d.value);
                } else {
                    declarations.push((d.name, d.value));
                }
            }
            for raw in css::split_top_level(&body[p..fo], b',') {
                let selector = css::lower(&css::trim(&raw));
                let offset = if selector == b"from" {
                    Some(0.0)
                } else if selector == b"to" {
                    Some(1.0)
                } else if selector.ends_with(b"%") {
                    css::number(&selector[..selector.len() - 1]).map(|v| v / 100.0)
                } else {
                    None
                };
                if let Some(offset) = offset {
                    frames.push(Keyframe { offset, declarations: declarations.clone(), timing: timing.clone() });
                }
            }
            p = fc + 1;
        }
        frames.sort_by(|a, b| {
            if a.offset < b.offset {
                std::cmp::Ordering::Less
            } else if b.offset < a.offset {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        rules.insert(name, frames);
        pos = close + 1;
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnimationSpec {
    pub name: Vec<u8>,
    pub duration: f64,
    pub timing_text: Vec<u8>,
    pub delay: f64,
    pub iterations: f64,
    pub direction: Vec<u8>,
    pub fill: Vec<u8>,
    pub paused: bool,
}

impl Default for AnimationSpec {
    fn default() -> AnimationSpec {
        AnimationSpec {
            name: b"none".to_vec(),
            duration: 0.0,
            timing_text: b"ease".to_vec(),
            delay: 0.0,
            iterations: 1.0,
            direction: b"normal".to_vec(),
            fill: b"none".to_vec(),
            paused: false,
        }
    }
}

impl AnimationSpec {
    pub fn timing(&self, timings: &mut TimingCache) -> Timing {
        Timing::parse(timings, &self.timing_text).unwrap_or_else(Timing::ease)
    }

    pub fn list(timings: &mut TimingCache, declarations: &Declared) -> Vec<AnimationSpec> {
        let mut specs: Vec<AnimationSpec> = Vec::new();
        if let Some(value) = declarations.get(&b"animation"[..]) {
            for item in css::split_top_level(value, b',') {
                let mut spec = AnimationSpec::default();
                let mut saw_duration = false;
                for token in css::split_top_level(&css::trim(&item), b' ') {
                    if token.is_empty() {
                        continue;
                    }
                    let t = css::lower(&token);
                    if let Some(time) = css::parse_time(&t) {
                        if saw_duration {
                            spec.delay = time;
                        } else {
                            spec.duration = time;
                            saw_duration = true;
                        }
                    } else if Timing::parse(timings, &t).is_some() {
                        spec.timing_text = t;
                    } else if t == b"infinite" {
                        spec.iterations = INF;
                    } else if let Some(n) = css::number(&t) {
                        spec.iterations = n;
                    } else if matches!(t.as_slice(), b"normal" | b"reverse" | b"alternate" | b"alternate-reverse") {
                        spec.direction = t;
                    } else if matches!(t.as_slice(), b"forwards" | b"backwards" | b"both") {
                        spec.fill = t;
                    } else if t == b"paused" || t == b"running" {
                        spec.paused = t == b"paused";
                    } else {
                        spec.name = token;
                    }
                }
                specs.push(spec);
            }
        }
        let mut longhand = |key: &[u8], apply: &dyn Fn(&mut AnimationSpec, &[u8])| {
            let Some(value) = declarations.get(key) else { return };
            let items: Vec<Vec<u8>> = css::split_top_level(value, b',').iter().map(|s| css::trim(s)).collect();
            if items.is_empty() {
                return;
            }
            if specs.is_empty() {
                specs.resize(items.len(), AnimationSpec::default());
            }
            for (i, spec) in specs.iter_mut().enumerate() {
                apply(spec, &items[i % items.len()]);
            }
        };
        longhand(b"animation-name", &|s, v| s.name = v.to_vec());
        longhand(b"animation-duration", &|s, v| s.duration = css::parse_time(v).unwrap_or(0.0));
        longhand(b"animation-timing-function", &|s, v| s.timing_text = v.to_vec());
        longhand(b"animation-delay", &|s, v| s.delay = css::parse_time(v).unwrap_or(0.0));
        longhand(b"animation-iteration-count", &|s, v| s.iterations = if v == b"infinite" { INF } else { css::number(v).unwrap_or(1.0) });
        longhand(b"animation-direction", &|s, v| s.direction = v.to_vec());
        longhand(b"animation-fill-mode", &|s, v| s.fill = v.to_vec());
        longhand(b"animation-play-state", &|s, v| s.paused = v == b"paused");
        specs.retain(|s| s.name != b"none" && !s.name.is_empty());
        specs
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TransitionSpec {
    pub property: Vec<u8>,
    pub duration: f64,
    pub timing: Timing,
    pub delay: f64,
}

impl Default for TransitionSpec {
    fn default() -> TransitionSpec {
        TransitionSpec { property: Vec::new(), duration: 0.0, timing: Timing::ease(), delay: 0.0 }
    }
}

impl TransitionSpec {
    pub fn list(timings: &mut TimingCache, declarations: &Declared) -> Vec<TransitionSpec> {
        let mut specs: Vec<TransitionSpec> = Vec::new();
        if let Some(value) = declarations.get(&b"transition"[..]) {
            for item in css::split_top_level(value, b',') {
                let mut spec = TransitionSpec { property: b"all".to_vec(), ..TransitionSpec::default() };
                let mut saw_duration = false;
                for token in css::split_top_level(&css::trim(&item), b' ') {
                    if token.is_empty() {
                        continue;
                    }
                    if let Some(time) = css::parse_time(&token) {
                        if saw_duration {
                            spec.delay = time;
                        } else {
                            spec.duration = time;
                            saw_duration = true;
                        }
                    } else if let Some(timing) = Timing::parse(timings, &token) {
                        spec.timing = timing;
                    } else {
                        spec.property = css::lower(&token);
                    }
                }
                specs.push(spec);
            }
        }
        let items = |key: &[u8]| -> Vec<Vec<u8>> {
            match declarations.get(key) {
                Some(value) => css::split_top_level(value, b',').iter().map(|s| css::trim(s)).collect(),
                None => Vec::new(),
            }
        };
        let properties = items(b"transition-property");
        let durations = items(b"transition-duration");
        if !properties.is_empty() {
            let timings_list = items(b"transition-timing-function");
            let delays = items(b"transition-delay");
            let mut next = Vec::with_capacity(properties.len());
            for (i, property) in properties.iter().enumerate() {
                let base = if i < specs.len() {
                    specs[i].clone()
                } else {
                    TransitionSpec { property: property.clone(), duration: 0.0, timing: Timing::ease(), delay: 0.0 }
                };
                let s = TransitionSpec {
                    property: css::lower(property),
                    duration: if durations.is_empty() {
                        base.duration
                    } else {
                        css::parse_time(&durations[i % durations.len()]).unwrap_or(0.0)
                    },
                    timing: if timings_list.is_empty() {
                        base.timing
                    } else {
                        Timing::parse(timings, &timings_list[i % timings_list.len()]).unwrap_or_else(Timing::ease)
                    },
                    delay: if delays.is_empty() { base.delay } else { css::parse_time(&delays[i % delays.len()]).unwrap_or(0.0) },
                };
                next.push(s);
            }
            specs = next;
        } else if !durations.is_empty() {
            for (i, spec) in specs.iter_mut().enumerate() {
                spec.duration = css::parse_time(&durations[i % durations.len()]).unwrap_or(0.0);
            }
        }
        specs.retain(|s| !(s.duration <= 0.0 || s.property == b"none"));
        specs
    }
}

// ───────────────────────── interpolation ─────────────────────────

/// Interpolates two CSS values number by number when they have the same shape (colours normalised,
/// `none` transforms expanded to identities, shorter transform lists padded); otherwise flips at 0.5.
pub fn interpolate(pairs: &mut PairCache, a: &[u8], b: &[u8], t: f64) -> Vec<u8> {
    if t == 0.0 || !t.is_finite() {
        return a.to_vec();
    }
    if t == 1.0 {
        return b.to_vec();
    }
    let flip = || if t < 0.5 { a.to_vec() } else { b.to_vec() };
    // Mixed units, or math on one side only: two translation lists still compose by weight.
    let weighted = || {
        let from = if css::iequals(&css::trim(a), b"none") { identity(b) } else { a.to_vec() };
        let to = if css::iequals(&css::trim(b), b"none") { identity(a) } else { b.to_vec() };
        if !translation_list(&from) || !translation_list(&to) {
            return flip();
        }
        concat(&[&scaled_translations(&css::trim(&from), 1.0 - t), b" ", &scaled_translations(&css::trim(&to), t)])
    };
    let Some((left, right)) = pairs.tokens(a, b) else { return weighted() };
    let mut out = Vec::new();
    for (x, y) in left.iter().zip(&right) {
        if x.number && y.number && (x.text == y.text || x.value == 0.0 || y.value == 0.0) {
            out.extend_from_slice(&number_text(x.value + (y.value - x.value) * t, 4));
            out.extend_from_slice(if x.text.is_empty() { &y.text } else { &x.text });
        } else if !x.number && !y.number && x.text == y.text {
            out.extend_from_slice(&x.text);
        } else {
            return weighted();
        }
    }
    out
}

// ───────────────────────── Web Animations records ─────────────────────────

/// One keyframe of a [`WebAnimation`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WebFrame {
    pub offset: f64,
    pub easing: Timing,
    /// Empty: the effect's.
    pub composite: Vec<u8>,
    pub values: StyleValues,
}

/// One `Element.animate()` effect as the sandbox reports it. Times in milliseconds; `start_ms` is
/// already on the renderer's clock.
#[derive(Clone, Debug, PartialEq)]
pub struct WebAnimation {
    pub aid: i64,
    pub target: i64,
    pub frames: Vec<WebFrame>,
    pub duration: f64,
    pub delay: f64,
    pub end_delay: f64,
    pub iterations: f64,
    pub iteration_start: f64,
    pub direction: Vec<u8>,
    pub fill: Vec<u8>,
    pub easing: Timing,
    pub composite: Vec<u8>,
    pub iteration_composite: Vec<u8>,
    pub start_ms: Option<f64>,
    pub hold_ms: Option<f64>,
    pub rate: f64,
    pub properties: Vec<Vec<u8>>,
    /// What it adds to its box while it runs (paint policy, rule 5).
    pub heavy: Option<BoxPaint>,
}

impl Default for WebAnimation {
    fn default() -> WebAnimation {
        WebAnimation {
            aid: 0,
            target: 0,
            frames: Vec::new(),
            duration: 0.0,
            delay: 0.0,
            end_delay: 0.0,
            iterations: 1.0,
            iteration_start: 0.0,
            direction: b"normal".to_vec(),
            fill: b"none".to_vec(),
            easing: Timing::default(),
            composite: b"replace".to_vec(),
            iteration_composite: b"replace".to_vec(),
            start_ms: None,
            hold_ms: None,
            rate: 1.0,
            properties: Vec::new(),
            heavy: None,
        }
    }
}

impl WebAnimation {
    pub fn running(&self) -> bool {
        self.hold_ms.is_none() && self.start_ms.is_some() && self.rate != 0.0
    }

    /// Validated like twidget's WebAnimation.parse; None drops the record.
    pub fn parse(timings: &mut TimingCache, o: &Json, clock_offset_ms: f64) -> Option<WebAnimation> {
        const MAX_MS: f64 = 86400.0 * 1000.0;
        const MAX_KEYFRAMES: usize = 64;
        const MAX_VALUES: usize = 64;
        let aid = o.find("aid")?;
        let target = o.find("target")?;
        let frames = o.find("keyframes")?;
        if !aid.is_number() || !target.is_number() || !frames.is_array() || frames.arr().len() > MAX_KEYFRAMES {
            return None;
        }
        let mut a = WebAnimation { aid: i64::from(aid.as_int(0)), target: i64::from(target.as_int(0)), ..WebAnimation::default() };
        let timing = o.get("timing");
        let ms = |key: &str, fallback: f64| -> Option<f64> {
            let Some(v) = timing.find(key) else { return Some(fallback) };
            if v.is_null() {
                return Some(fallback);
            }
            let n = v.as_number(0.0);
            if !v.is_number() || !n.is_finite() || n.abs() > MAX_MS {
                return None;
            }
            Some(n)
        };
        let (duration, delay, end_delay) = (ms("duration", 0.0), ms("delay", 0.0), ms("endDelay", 0.0));
        let (Some(duration), Some(delay), Some(end_delay)) = (duration, delay, end_delay) else { return None };
        if duration < 0.0 {
            return None;
        }
        a.duration = duration;
        a.delay = delay;
        a.end_delay = end_delay;
        if let Some(it) = timing.find("iterations") {
            if it.is_null() {
                a.iterations = INF;
            } else if !it.is_number() || it.as_number(0.0) < 0.0 || it.as_number(0.0) > 1e9 {
                return None;
            } else {
                a.iterations = it.as_number(0.0);
            }
        }
        if let Some(start) = timing.find("iterationStart")
            && start.is_number()
        {
            if start.as_number(0.0) < 0.0 || start.as_number(0.0) > 1e9 {
                return None;
            }
            a.iteration_start = start.as_number(0.0);
        }
        let direction = if timing.get("direction").is_string() { timing.get("direction").as_str().to_vec() } else { b"normal".to_vec() };
        let fill = if timing.get("fill").is_string() { timing.get("fill").as_str().to_vec() } else { b"none".to_vec() };
        if !matches!(direction.as_slice(), b"normal" | b"reverse" | b"alternate" | b"alternate-reverse") {
            return None;
        }
        if !matches!(fill.as_slice(), b"none" | b"forwards" | b"backwards" | b"both" | b"auto") {
            return None;
        }
        a.direction = direction;
        a.fill = if fill == b"auto" { b"none".to_vec() } else { fill };
        let mut parse_easing = |v: &Json| -> Option<Timing> {
            let text: &[u8] = if v.is_string() { v.as_str() } else { b"linear" };
            if text.is_empty() || text.len() > 4096 {
                return None;
            }
            let t = Timing::parse(timings, text)?;
            // Stricter than CSS: control points stay in x ∈ [0, 1], and linear() is bounded.
            if t.kind == Kind::Bezier && (t.x1 < 0.0 || t.x1 > 1.0 || t.x2 < 0.0 || t.x2 > 1.0) {
                return None;
            }
            if t.kind == Kind::Steps && t.jump == Jump::JumpNone && t.steps < 2 {
                return None;
            }
            if t.kind == Kind::Points && t.points.len() > 256 {
                return None;
            }
            Some(t)
        };
        a.easing = parse_easing(timing.get("easing"))?;
        let mut previous = 0.0;
        for raw in frames.arr() {
            let offset = raw.find("offset")?;
            if !offset.is_number() || offset.as_number(0.0) < previous || offset.as_number(0.0) > 1.0 {
                return None;
            }
            let frame_easing = parse_easing(raw.get("easing"))?;
            previous = offset.as_number(0.0);
            let mut f = WebFrame { offset: previous, easing: frame_easing, ..WebFrame::default() };
            if let Some(c) = raw.find("composite")
                && c.is_string()
            {
                if !matches!(c.as_str(), b"replace" | b"add" | b"accumulate") {
                    return None;
                }
                f.composite = c.as_str().to_vec();
            }
            let values = raw.get("values").obj();
            if values.len() > MAX_VALUES {
                return None;
            }
            let mut sorted: Vec<(Vec<u8>, Vec<u8>)> =
                values.iter().filter(|(_, value)| value.is_string()).map(|(name, value)| (name.clone(), value.as_str().to_vec())).collect();
            sorted.sort();
            for (name, value) in sorted {
                if !a.properties.contains(&name) {
                    a.properties.push(name.clone());
                }
                f.values.push((name, value));
            }
            a.frames.push(f);
        }
        a.composite = if o.get("composite").is_string() { o.get("composite").as_str().to_vec() } else { b"replace".to_vec() };
        a.iteration_composite =
            if o.get("iterationComposite").is_string() { o.get("iterationComposite").as_str().to_vec() } else { b"replace".to_vec() };
        if !matches!(a.composite.as_slice(), b"replace" | b"add" | b"accumulate") {
            return None;
        }
        if !matches!(a.iteration_composite.as_slice(), b"replace" | b"accumulate") {
            return None;
        }
        if let Some(s) = o.find("startTime")
            && s.is_number()
        {
            if !s.as_number(0.0).is_finite() {
                return None;
            }
            a.start_ms = Some(s.as_number(0.0) + clock_offset_ms);
        }
        if let Some(h) = o.find("holdTime")
            && h.is_number()
        {
            if !h.as_number(0.0).is_finite() || h.as_number(0.0).abs() > 1e12 {
                return None;
            }
            a.hold_ms = Some(h.as_number(0.0));
        }
        a.rate = if o.get("playbackRate").is_number() { o.get("playbackRate").as_number(0.0) } else { 1.0 };
        if !a.rate.is_finite() || a.rate.abs() > 1000.0 || (a.start_ms.is_none() && a.hold_ms.is_none()) {
            return None;
        }
        Some(a)
    }
}

/// The four events an animator reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    AnimationStart,
    AnimationIteration,
    AnimationEnd,
    TransitionEnd,
}

impl EventKind {
    /// The event's DOM type.
    pub fn name(self) -> &'static str {
        match self {
            EventKind::AnimationStart => "animationstart",
            EventKind::AnimationIteration => "animationiteration",
            EventKind::AnimationEnd => "animationend",
            EventKind::TransitionEnd => "transitionend",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnimationEvent {
    pub kind: EventKind,
    pub target: i64,
    /// animationName, or the transition's propertyName.
    pub name: Vec<u8>,
    pub elapsed: f64,
}

/// What `plan` hands a compositor for an element's animations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plan {
    /// False while an animation still waits out its delay: nothing to hand over before `wake`.
    pub ready: bool,
    /// The span to bake, on the renderer clock: up to the first end, or one loop when the animations
    /// repeat forever.
    pub start: f64,
    pub duration: f64,
    pub repeat: bool,
    /// When the core must sample the element again: an end, a delay running out, or with `iterations`
    /// the next animationiteration.
    pub wake: f64,
}

impl Default for Plan {
    fn default() -> Plan {
        Plan { ready: true, start: 0.0, duration: 0.0, repeat: false, wake: 0.0 }
    }
}

/// Rule 5 of the heavy-box count (paint policy): an animation that runs on element `id` and changes
/// what makes a box heavy. `kind` b'a' is a CSS animation, from the end of its delay to its end, paused
/// or not; b't' a transition likewise; b'w' a Web Animation while it runs, neither held nor finished.
/// `name` the animation's or the transition's property (empty for b'w'), `aid` the Web Animation's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Motion {
    pub id: i64,
    pub kind: u8,
    pub name: Vec<u8>,
    pub aid: i64,
    pub paint: BoxPaint,
}

#[derive(Clone, Debug, PartialEq)]
struct Running {
    spec: AnimationSpec,
    start: f64,
    iteration: i32,
    ended: bool,
    // animation-play-state: paused stops the timeline where it is; running resumes it from there.
    paused_at: Option<f64>,
    paused_duration: f64,
}

impl Running {
    fn new(spec: AnimationSpec, start: f64, iteration: i32) -> Running {
        Running { spec, start, iteration, ended: false, paused_at: None, paused_duration: 0.0 }
    }

    fn timeline_start(&self) -> f64 {
        self.start + self.paused_duration
    }

    fn local_time(&self, now: f64) -> f64 {
        self.paused_at.unwrap_or(now) - self.timeline_start()
    }
}

#[derive(Clone, Debug)]
struct Transition {
    from: Vec<u8>,
    to: Vec<u8>,
    start: f64,
    spec: TransitionSpec,
    heavy: Option<BoxPaint>,
}

#[derive(Clone, Copy, Debug)]
struct Override {
    origin: f64,
    start: Option<f64>,
    hold: Option<f64>,
    rate: f64,
    cancelled: bool,
}

impl Override {
    fn local(&self, now: f64) -> f64 {
        match self.hold {
            Some(hold) => hold,
            None => (now - self.start.unwrap_or(now)) * self.rate,
        }
    }
}

type Overrides = BTreeMap<i64, BTreeMap<Vec<u8>, Override>>;

fn override_in<'a>(overrides: &'a Overrides, id: i64, key: &[u8]) -> Option<&'a Override> {
    overrides.get(&id)?.get(key)
}

fn key(kind: &[u8], name: &[u8]) -> Vec<u8> {
    concat(&[kind, name])
}

fn values(caches: &mut Caches, frames: &[Keyframe], base: &Declared, progress: f64, spec: &AnimationSpec) -> StyleValues {
    struct Stop<'a> {
        offset: f64,
        value: &'a [u8],
        timing: &'a Option<Timing>,
    }
    const NONE: &Option<Timing> = &None;
    let mut properties: Vec<&[u8]> = Vec::new();
    for f in frames {
        for (name, _) in &f.declarations {
            if !properties.contains(&name.as_slice()) {
                properties.push(name);
            }
        }
    }
    let mut result = StyleValues::new();
    let spec_timing = spec.timing(&mut caches.timings);
    for property in properties {
        let mut stops: Vec<Stop> = Vec::new();
        for f in frames {
            if let Some(v) = last_value(&f.declarations, property) {
                stops.push(Stop { offset: f.offset, value: v, timing: &f.timing });
            }
        }
        let own: Option<&[u8]> = match base.get(property) {
            Some(b) => Some(b),
            None => initial_value(property),
        };
        if let Some(own) = own
            && !stops.is_empty()
        {
            if stops[0].offset > 0.0 {
                stops.insert(0, Stop { offset: 0.0, value: own, timing: &frames[0].timing });
            }
            if stops[stops.len() - 1].offset < 1.0 {
                stops.push(Stop { offset: 1.0, value: own, timing: NONE });
            }
        }
        let (Some(front), Some(back)) = (stops.first(), stops.last()) else { continue };
        if progress <= front.offset {
            result.push((property.to_vec(), front.value.to_vec()));
            continue;
        }
        if progress >= back.offset {
            result.push((property.to_vec(), back.value.to_vec()));
            continue;
        }
        for pair in stops.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            if progress >= a.offset && progress <= b.offset {
                let span = max(b.offset - a.offset, 0.000001);
                let local = (progress - a.offset) / span;
                let timing = a.timing.as_ref().unwrap_or(&spec_timing);
                result.push((property.to_vec(), interpolate(&mut caches.pairs, a.value, b.value, timing.css(local))));
                break;
            }
        }
    }
    result
}

/// `events` None: a peek, which reports nothing.
fn sample_animation(
    caches: &mut Caches,
    overrides: &Overrides,
    bases: &BTreeMap<i64, Declared>,
    id: i64,
    item: &mut Running,
    frames: &[Keyframe],
    now: f64,
    events: Option<&mut Vec<AnimationEvent>>,
) -> Option<StyleValues> {
    let mut local = item.local_time(now) - item.spec.delay;
    if let Some(o) = override_in(overrides, id, &key(b"animation:", &item.spec.name)) {
        if o.cancelled {
            return None;
        }
        local = o.local(now) - item.spec.delay;
    }
    let empty = Declared::new();
    let base = bases.get(&id).unwrap_or(&empty);
    let spec = &item.spec;
    let reversed = |iteration: i32| match spec.direction.as_slice() {
        b"reverse" => true,
        b"alternate" => iteration % 2 == 1,
        b"alternate-reverse" => iteration % 2 == 0,
        _ => false,
    };
    if local < 0.0 {
        if spec.fill == b"backwards" || spec.fill == b"both" {
            let progress = if spec.direction.starts_with(b"reverse") || spec.direction == b"alternate-reverse" { 1.0 } else { 0.0 };
            return Some(values(caches, frames, base, progress, spec));
        }
        return None;
    }
    let duration = max(spec.duration, 0.0001);
    let total = spec.iterations * duration;
    if local >= total {
        let forwards = spec.fill == b"forwards" || spec.fill == b"both";
        let iterations = spec.iterations;
        let last_iteration = 0.max(clamp(iterations, 0.0, 1e9).ceil() as i32 - 1);
        let reversed_last = reversed(last_iteration);
        if !item.ended {
            item.ended = true;
            if let Some(events) = events {
                events.push(AnimationEvent { kind: EventKind::AnimationEnd, target: id, name: item.spec.name.clone(), elapsed: total });
            }
        }
        if forwards {
            let fraction = iterations % 1.0;
            let mut progress = if fraction == 0.0 { 1.0 } else { fraction };
            if reversed_last {
                progress = 1.0 - progress;
            }
            return Some(values(caches, frames, base, progress, &item.spec));
        }
        return None;
    }
    // The C++'s int() of a NaN is INT_MIN on x64, Rust's 0; a finite clock gives no NaN here.
    let iteration = min(local / duration, 1e9) as i32;
    let reversed_now = reversed(iteration);
    if iteration != item.iteration {
        item.iteration = iteration;
        if let Some(events) = events {
            events.push(AnimationEvent { kind: EventKind::AnimationIteration, target: id, name: item.spec.name.clone(), elapsed: local });
        }
    }
    let mut progress = (local - f64::from(iteration) * duration) / duration;
    if reversed_now {
        progress = 1.0 - progress;
    }
    Some(values(caches, frames, base, progress, &item.spec))
}

fn transition_value(
    pairs: &mut PairCache,
    transitions: &BTreeMap<i64, BTreeMap<Vec<u8>, Transition>>,
    id: i64,
    property: &[u8],
    now: f64,
) -> Option<Vec<u8>> {
    let t = transitions.get(&id)?.get(property)?;
    let elapsed = now - t.start - t.spec.delay;
    if elapsed <= 0.0 {
        return Some(t.from.clone());
    }
    let progress = min(1.0, elapsed / t.spec.duration);
    Some(interpolate(pairs, &t.from, &t.to, t.spec.timing.css(progress)))
}

const WATCHED: [&str; 42] = [
    "animation",
    "animation-name",
    "animation-duration",
    "animation-timing-function",
    "animation-delay",
    "animation-iteration-count",
    "animation-direction",
    "animation-fill-mode",
    "animation-play-state",
    "transition",
    "transition-property",
    "transition-duration",
    "transition-timing-function",
    "transition-delay",
    "transform",
    "opacity",
    "color",
    "background-color",
    "background",
    "width",
    "height",
    "top",
    "left",
    "right",
    "bottom",
    "box-shadow",
    "border-color",
    "max-height",
    "stroke-dashoffset",
    "fill",
    "stroke",
    "translate",
    "scale",
    "rotate",
    "filter",
    "letter-spacing",
    "font-size",
    "margin-top",
    "margin-left",
    "flex-grow",
    "transform-origin",
    "backdrop-filter",
];

const ENTRY_PROPERTIES: [&str; 7] =
    ["animation", "animation-name", "transition", "transition-property", "backdrop-filter", "filter", "box-shadow"];

/// The animations, transitions and Web Animations of one renderer.
#[derive(Debug, Default)]
pub struct Animator {
    caches: SharedCaches,
    keyframes: KeyframeRules,
    /// Rule 5 for each @keyframes name, read once per set of style sheets.
    keyframes_paint: HashMap<Vec<u8>, Option<BoxPaint>>,
    running: BTreeMap<i64, Vec<Running>>,
    transitions: BTreeMap<i64, BTreeMap<Vec<u8>, Transition>>,
    bases: BTreeMap<i64, Declared>,
    overrides: Overrides,
    /// By aid; sampled in aid order, as twidget does.
    web: BTreeMap<i64, WebAnimation>,
    events: Vec<AnimationEvent>,
    /// The value an element has without animations, for Web Animations that add to it or leave a
    /// keyframe open. The renderer sets it before `sample`.
    pub underlying: HashMap<i64, Declared>,
}

impl Animator {
    /// An animator with caches of its own.
    pub fn new() -> Animator {
        Animator::default()
    }

    /// An animator on caches it shares, as the C++'s animators on one thread share theirs.
    pub fn with_caches(caches: SharedCaches) -> Animator {
        Animator { caches, ..Animator::default() }
    }

    pub fn caches(&self) -> &SharedCaches {
        &self.caches
    }

    /// Declared properties the renderer reads per element; transitions are watched on these.
    pub fn watched() -> &'static [&'static str] {
        &WATCHED
    }

    /// An element is read only when it declares one of these (or is a Web Animations target).
    pub fn entry_properties() -> &'static [&'static str] {
        &ENTRY_PROPERTIES
    }

    pub fn set_style_sheets<T: AsRef<[u8]>>(&mut self, texts: &[T]) {
        let mut caches = lock(&self.caches);
        let mut rules = KeyframeRules::new();
        for text in texts {
            parse_keyframes(&mut caches.timings, text.as_ref(), &mut rules);
        }
        self.keyframes = rules;
        self.keyframes_paint.clear();
    }

    /// `id` declares an animation or a transition, whether or not one runs now.
    pub fn declares_motion(&self, id: i64) -> bool {
        let Some(b) = self.bases.get(&id) else { return false };
        for property in ["animation", "animation-name", "transition", "transition-property"] {
            if let Some(v) = b.get(property.as_bytes())
                && !v.is_empty()
                && v != b"none"
            {
                return true;
            }
        }
        false
    }

    fn underlying_value(&self, id: i64, property: &[u8]) -> Vec<u8> {
        if let Some(v) = self.bases.get(&id).and_then(|b| b.get(property)) {
            return v.clone();
        }
        if let Some(v) = self.underlying.get(&id).and_then(|u| u.get(property)) {
            return v.clone();
        }
        initial_value(property).unwrap_or_default().to_vec()
    }

    /// `property` of `id` without its animations: as declared, or the property's initial value.
    pub fn base_value(&self, id: i64, property: &[u8]) -> Vec<u8> {
        self.underlying_value(id, property)
    }

    /// The declared values of every element after a build; starts, restarts and stops animations
    /// and starts transitions where a watched value changed.
    pub fn update(&mut self, declarations: &BTreeMap<i64, Declared>, now: f64) {
        let mut caches = lock(&self.caches);
        let caches = &mut *caches;
        let mut next_running: BTreeMap<i64, Vec<Running>> = BTreeMap::new();
        for (&id, values) in declarations {
            let specs = AnimationSpec::list(&mut caches.timings, values);
            if !specs.is_empty() {
                let previous: &[Running] = self.running.get(&id).map_or(&[], Vec::as_slice);
                let next = next_running.entry(id).or_default();
                for spec in specs {
                    if let Some(existing) = previous.iter().find(|r| r.spec.name == spec.name) {
                        if existing.spec == spec {
                            next.push(existing.clone());
                            continue;
                        }
                        let mut changed = Running::new(spec, existing.start, existing.iteration);
                        changed.paused_duration = existing.paused_duration;
                        if changed.spec.paused {
                            changed.paused_at = Some(existing.paused_at.unwrap_or(now));
                        } else if let Some(paused_at) = existing.paused_at {
                            changed.paused_duration += now - paused_at;
                        }
                        next.push(changed);
                        continue;
                    }
                    if self.keyframes.contains_key(&spec.name) {
                        self.events.push(AnimationEvent {
                            kind: EventKind::AnimationStart,
                            target: id,
                            name: spec.name.clone(),
                            elapsed: 0.0,
                        });
                    }
                    let mut fresh = Running::new(spec, now, 0);
                    if fresh.spec.paused {
                        fresh.paused_at = Some(now);
                    }
                    next.push(fresh);
                }
            }
            let transition_specs = TransitionSpec::list(&mut caches.timings, values);
            let known = self.bases.contains_key(&id);
            let mut base: Declared = self.bases.get(&id).cloned().unwrap_or_default();
            for property in WATCHED {
                if property.starts_with("animation") || property.starts_with("transition") {
                    continue;
                }
                let property = property.as_bytes();
                let new_value = values.get(property);
                let mut old_value: Option<Vec<u8>> = base.get(property).cloned();
                // A transition from unset starts at the property's initial value.
                if old_value.is_none()
                    && known
                    && new_value.is_some()
                    && let Some(initial) = initial_value(property)
                {
                    old_value = Some(initial.to_vec());
                }
                if let (Some(old_value), Some(new_value)) = (&old_value, new_value)
                    && old_value != new_value
                {
                    match transition_specs.iter().rev().find(|s| s.property == property || s.property == b"all") {
                        None => {
                            if let Some(t) = self.transitions.get_mut(&id) {
                                t.remove(property);
                            }
                        }
                        Some(spec) => {
                            let mut current = transition_value(&mut caches.pairs, &self.transitions, id, property, now)
                                .unwrap_or_else(|| old_value.clone());
                            if current.len() > MAX_RETARGET_VALUE {
                                current = old_value.clone();
                            }
                            // Its keyframes are its two ends.
                            let mut heavy = AnimationPaint::default();
                            heavy.keyframe(Some(0.0));
                            heavy.property(property, Some(&current));
                            heavy.keyframe(Some(1.0));
                            heavy.property(property, Some(new_value));
                            let transition =
                                Transition { from: current, to: new_value.clone(), start: now, spec: spec.clone(), heavy: heavy.result() };
                            self.transitions.entry(id).or_default().insert(property.to_vec(), transition);
                        }
                    }
                } else if new_value.is_none()
                    && let Some(t) = self.transitions.get_mut(&id)
                {
                    t.remove(property);
                }
                match new_value {
                    Some(v) => {
                        base.insert(property.to_vec(), v.clone());
                    }
                    None => {
                        base.remove(property);
                    }
                }
            }
            self.bases.insert(id, base);
            if self.transitions.get(&id).is_some_and(BTreeMap::is_empty) {
                self.transitions.remove(&id);
            }
        }
        let transitions = &mut self.transitions;
        self.bases.retain(|id, _| {
            let keep = declarations.contains_key(id);
            if !keep {
                transitions.remove(id);
            }
            keep
        });
        self.running = next_running;
        prune_overrides(&mut self.overrides, &self.running, &self.transitions);
    }

    /// Style overrides at `now`, and the animation events crossed since the last sample.
    pub fn sample(&mut self, now: f64) -> StyleOverrides {
        let mut caches = lock(&self.caches);
        let caches = &mut *caches;
        let mut out = StyleOverrides::new();
        for (&id, list) in self.running.iter_mut() {
            for item in list.iter_mut() {
                let Some(frames) = self.keyframes.get(&item.spec.name) else { continue };
                if frames.is_empty() {
                    continue;
                }
                if let Some(values) = sample_animation(caches, &self.overrides, &self.bases, id, item, frames, now, Some(&mut self.events))
                {
                    for (property, value) in values {
                        merge(out.entry(id).or_default(), &property, value);
                    }
                }
            }
        }
        let overrides = &self.overrides;
        let events = &mut self.events;
        self.transitions.retain(|&id, list| {
            list.retain(|property, transition| {
                let mut elapsed = now - transition.start - transition.spec.delay;
                if let Some(o) = override_in(overrides, id, &key(b"transition:", property)) {
                    if o.cancelled {
                        return false;
                    }
                    elapsed = o.local(now) - transition.spec.delay;
                }
                if elapsed >= transition.spec.duration {
                    events.push(AnimationEvent {
                        kind: EventKind::TransitionEnd,
                        target: id,
                        name: property.clone(),
                        elapsed: transition.spec.duration,
                    });
                    return false;
                }
                let progress = max(0.0, elapsed) / transition.spec.duration;
                let value = if elapsed <= 0.0 {
                    transition.from.clone()
                } else {
                    interpolate(&mut caches.pairs, &transition.from, &transition.to, transition.spec.timing.css(progress))
                };
                merge(out.entry(id).or_default(), property, value);
                true
            });
            !list.is_empty()
        });
        if !self.web.is_empty() {
            self.sample_web(caches, &mut out, now, 0);
        }
        out.retain(|_, values| !values.is_empty());
        out
    }

    /// `only` non-zero: that target alone.
    fn sample_web(&self, caches: &mut Caches, out: &mut StyleOverrides, now: f64, only: i64) {
        // By target, then in aid order: later animations composite onto earlier ones.
        let mut by_target: BTreeMap<i64, Vec<&WebAnimation>> = BTreeMap::new();
        for a in self.web.values() {
            if only == 0 || a.target == only {
                by_target.entry(a.target).or_default().push(a);
            }
        }
        for (id, list) in by_target {
            for a in list {
                let Some(state) = compute(a, local_time(a, now), a.rate >= 0.0) else { continue };
                if state.active_time.is_none() {
                    continue;
                }
                let mut values = StyleValues::new();
                {
                    let current: &StyleOverrides = out;
                    let underlying_of = |property: &[u8]| -> Vec<u8> {
                        if let Some(v) = current.get(&id).and_then(|o| last_value(o, property)) {
                            return v.clone();
                        }
                        self.underlying_value(id, property)
                    };
                    for property in &a.properties {
                        if let Some(v) = web_value(&mut caches.pairs, a, property, state.progress, state.iteration, &underlying_of) {
                            values.push((property.clone(), v));
                        }
                    }
                }
                for (property, value) in values {
                    merge(out.entry(id).or_default(), &property, value);
                }
            }
        }
    }

    /// Something will change on the next frame.
    pub fn active(&self, now: f64) -> bool {
        self.active_except(now, &HashSet::new())
    }

    /// The same, leaving out the elements in `skip`.
    pub fn active_except(&self, now: f64, skip: &HashSet<i64>) -> bool {
        for &id in self.transitions.keys() {
            if !skip.contains(&id) {
                return true;
            }
        }
        for a in self.web.values() {
            if !a.running() || skip.contains(&a.target) {
                continue;
            }
            let Some(state) = compute(a, local_time(a, now), a.rate >= 0.0) else { continue };
            let pending = if a.rate > 0.0 { state.before } else { state.after };
            if (!state.before && !state.after) || pending {
                return true;
            }
        }
        for (id, list) in &self.running {
            if skip.contains(id) {
                continue;
            }
            for r in list {
                if !r.ended && !r.spec.paused && self.keyframes.contains_key(&r.spec.name) {
                    return true;
                }
            }
        }
        false
    }

    pub fn take_events(&mut self) -> Vec<AnimationEvent> {
        std::mem::take(&mut self.events)
    }

    // What a compositor needs to play an element's animations without the core.

    /// Elements whose animations, transitions or Web Animations still change something at `now`: not
    /// ended, paused, held or finished.
    pub fn animated_ids(&self, now: f64) -> Vec<i64> {
        let mut out = Vec::new();
        for (&id, list) in &self.running {
            for r in list {
                if r.ended || !self.keyframes.contains_key(&r.spec.name) {
                    continue;
                }
                let skip = match override_in(&self.overrides, id, &key(b"animation:", &r.spec.name)) {
                    Some(o) => o.cancelled || o.hold.is_some(),
                    None => r.spec.paused,
                };
                if skip {
                    continue;
                }
                out.push(id);
                break;
            }
        }
        out.extend(self.transitions.keys().copied());
        for a in self.web.values() {
            if !a.running() {
                continue;
            }
            if let Some(state) = compute(a, local_time(a, now), a.rate >= 0.0)
                && !(if a.rate > 0.0 { state.after } else { state.before })
            {
                out.push(a.target);
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Every property the animations of `id` write.
    pub fn animated_properties(&self, id: i64) -> Vec<Vec<u8>> {
        let mut out: Vec<Vec<u8>> = Vec::new();
        let mut add = |p: &[u8]| {
            if !out.iter().any(|x| x == p) {
                out.push(p.to_vec());
            }
        };
        if let Some(list) = self.running.get(&id) {
            for item in list {
                if let Some(frames) = self.keyframes.get(&item.spec.name) {
                    for f in frames {
                        for (property, _) in &f.declarations {
                            add(property);
                        }
                    }
                }
            }
        }
        if let Some(list) = self.transitions.get(&id) {
            for property in list.keys() {
                add(property);
            }
        }
        for a in self.web.values() {
            if a.target == id {
                for p in &a.properties {
                    add(p);
                }
            }
        }
        out
    }

    /// What `sample` would write for `id` at `t`, without events and without changing any state.
    pub fn peek(&self, id: i64, t: f64) -> StyleValues {
        let mut caches = lock(&self.caches);
        let caches = &mut *caches;
        let mut out = StyleOverrides::new();
        if let Some(list) = self.running.get(&id) {
            for item in list {
                let mut item = item.clone();
                let Some(frames) = self.keyframes.get(&item.spec.name) else { continue };
                if frames.is_empty() {
                    continue;
                }
                if let Some(values) = sample_animation(caches, &self.overrides, &self.bases, id, &mut item, frames, t, None) {
                    for (property, value) in values {
                        merge(out.entry(id).or_default(), &property, value);
                    }
                }
            }
        }
        if let Some(list) = self.transitions.get(&id) {
            for (property, transition) in list {
                let mut elapsed = t - transition.start - transition.spec.delay;
                if let Some(o) = override_in(&self.overrides, id, &key(b"transition:", property)) {
                    if o.cancelled {
                        continue;
                    }
                    elapsed = o.local(t) - transition.spec.delay;
                }
                // An ended transition leaves the declared value, which the element already has.
                if elapsed >= transition.spec.duration {
                    continue;
                }
                let progress = max(0.0, elapsed) / transition.spec.duration;
                let value = if elapsed <= 0.0 {
                    transition.from.clone()
                } else {
                    interpolate(&mut caches.pairs, &transition.from, &transition.to, transition.spec.timing.css(progress))
                };
                merge(out.entry(id).or_default(), property, value);
            }
        }
        if !self.web.is_empty() {
            self.sample_web(caches, &mut out, t, id);
        }
        out.remove(&id).unwrap_or_default()
    }

    /// None when there is nothing to hand over, or when the timing cannot be baked: a reversed
    /// playback rate, two endless loops of different lengths, accumulating iterations.
    pub fn plan(&self, id: i64, now: f64, iterations: bool) -> Option<Plan> {
        struct Span {
            now: f64,
            end: f64,
            looping: bool,
            loop_period: f64,
            loop_origin: f64,
            wait: f64,
            next_iteration: f64,
        }
        impl Span {
            // An animation's local time runs `rate` times as fast as the clock from `origin`; it begins
            // after `delay` local seconds and repeats every `period` local seconds.
            fn add(&mut self, origin: f64, rate: f64, delay: f64, duration: f64, count: f64, alternate: bool) -> bool {
                if rate <= 0.0 || duration <= 0.0 {
                    return false;
                }
                let begin = origin + delay / rate;
                if self.now < begin {
                    self.wait = min(self.wait, begin);
                    return true;
                }
                let iteration = duration / rate;
                if count.is_infinite() {
                    let period = iteration * if alternate { 2.0 } else { 1.0 };
                    if self.looping && (period - self.loop_period).abs() > 1e-6 {
                        return false;
                    }
                    if !self.looping {
                        self.loop_origin = begin;
                    }
                    self.looping = true;
                    self.loop_period = period;
                } else {
                    self.end = min(self.end, begin + count * iteration);
                }
                let done = ((self.now - begin) / iteration).floor() + 1.0;
                if count.is_infinite() || done < count {
                    self.next_iteration = min(self.next_iteration, begin + done * iteration);
                }
                true
            }
        }
        let mut plan = Plan::default();
        let mut s = Span { now, end: INF, looping: false, loop_period: 0.0, loop_origin: 0.0, wait: INF, next_iteration: INF };

        if let Some(list) = self.running.get(&id) {
            for item in list {
                if item.ended || !self.keyframes.contains_key(&item.spec.name) {
                    continue;
                }
                let spec = &item.spec;
                let mut origin = item.timeline_start();
                let mut rate = 1.0;
                if let Some(o) = override_in(&self.overrides, id, &key(b"animation:", &spec.name)) {
                    if o.cancelled || o.hold.is_some() {
                        continue;
                    }
                    // An override that is neither cancelled nor held has a start (apply_web).
                    let Some(start) = o.start else { continue };
                    origin = start;
                    rate = o.rate;
                } else if spec.paused {
                    continue;
                }
                let alternate = spec.direction == b"alternate" || spec.direction == b"alternate-reverse";
                if !s.add(origin, rate, spec.delay, max(spec.duration, 0.0001), spec.iterations, alternate) {
                    return None;
                }
            }
        }
        if let Some(list) = self.transitions.get(&id) {
            for (property, transition) in list {
                let mut origin = transition.start;
                let mut rate = 1.0;
                if let Some(o) = override_in(&self.overrides, id, &key(b"transition:", property)) {
                    if o.cancelled || o.hold.is_some() {
                        continue;
                    }
                    let Some(start) = o.start else { continue };
                    origin = start;
                    rate = o.rate;
                }
                if !s.add(origin, rate, transition.spec.delay, transition.spec.duration, 1.0, false) {
                    return None;
                }
            }
        }
        for a in self.web.values() {
            if a.target != id || !a.running() {
                continue;
            }
            if a.iteration_composite != b"replace" {
                return None;
            }
            let alternate = a.direction == b"alternate" || a.direction == b"alternate-reverse";
            let start_ms = a.start_ms.unwrap_or(0.0);
            // Local milliseconds: (now·1000 − start) · rate, so the origin is start / 1000 seconds.
            if !s.add(start_ms / 1000.0, a.rate, a.delay / 1000.0, a.duration / 1000.0, a.iterations, alternate) {
                return None;
            }
        }

        plan.wake = min(s.wait, s.end);
        if iterations {
            plan.wake = min(plan.wake, s.next_iteration);
        }
        if s.wait < INF && s.end == INF && !s.looping {
            plan.ready = false;
            return Some(plan);
        }
        // Something still waiting changes the picture at `wait`: bake only up to there.
        let end = min(s.end, s.wait);
        plan.start = now;
        if end < INF {
            plan.duration = end - now;
        } else if s.looping {
            // From the loop's last start, which the platform runs from part way in: a loop baked from now
            // would carry its wrap from end to start inside the track.
            plan.start = s.loop_origin + ((now - s.loop_origin) / s.loop_period).floor() * s.loop_period;
            plan.duration = s.loop_period;
            plan.repeat = true;
        } else {
            return None;
        }
        if plan.duration <= 0.0 {
            return None;
        }
        Some(plan)
    }

    /// Changes whenever anything that decides how `id` animates changes; equal values bake equal tracks.
    pub fn signature(&self, id: i64) -> u64 {
        let mut s: Vec<u8> = Vec::new();
        fn num(s: &mut Vec<u8>, v: f64) {
            s.extend_from_slice(&number_text(v, 6));
            s.push(b'|');
        }
        fn text(s: &mut Vec<u8>, v: &[u8]) {
            s.extend_from_slice(v);
            s.push(b'|');
        }
        let flag = |b: bool| if b { 1.0 } else { 0.0 };
        let control = |s: &mut Vec<u8>, key: &[u8]| {
            if let Some(o) = override_in(&self.overrides, id, key) {
                text(s, key);
                num(s, o.origin);
                num(s, o.start.unwrap_or(-1.0));
                num(s, o.hold.unwrap_or(-1.0));
                num(s, o.rate);
                num(s, flag(o.cancelled));
            }
        };
        if let Some(list) = self.running.get(&id) {
            for item in list {
                let spec = &item.spec;
                text(&mut s, &spec.name);
                num(&mut s, item.timeline_start());
                num(&mut s, spec.duration);
                text(&mut s, &spec.timing_text);
                num(&mut s, spec.delay);
                num(&mut s, spec.iterations);
                text(&mut s, &spec.direction);
                text(&mut s, &spec.fill);
                num(&mut s, flag(spec.paused));
                num(&mut s, flag(item.ended));
                control(&mut s, &key(b"animation:", &spec.name));
            }
        }
        if let Some(list) = self.transitions.get(&id) {
            for (property, transition) in list {
                text(&mut s, property);
                text(&mut s, &transition.from);
                text(&mut s, &transition.to);
                num(&mut s, transition.start);
                num(&mut s, transition.spec.duration);
                num(&mut s, transition.spec.delay);
                text(&mut s, &transition.spec.timing.text);
                control(&mut s, &key(b"transition:", property));
            }
        }
        for (&aid, a) in &self.web {
            if a.target != id {
                continue;
            }
            num(&mut s, aid as f64);
            num(&mut s, a.start_ms.unwrap_or(-1.0));
            num(&mut s, a.hold_ms.unwrap_or(-1.0));
            num(&mut s, a.rate);
            num(&mut s, a.duration);
            num(&mut s, a.delay);
            num(&mut s, a.iterations);
            text(&mut s, &a.direction);
            text(&mut s, &a.fill);
            text(&mut s, &a.easing.text);
            text(&mut s, &a.composite);
            for frame in &a.frames {
                num(&mut s, frame.offset);
                text(&mut s, &frame.easing.text);
                for (property, value) in &frame.values {
                    text(&mut s, property);
                    text(&mut s, value);
                }
            }
        }
        for property in ["transform", "opacity", "translate", "rotate", "scale", "transform-origin"] {
            text(&mut s, &self.base_value(id, property.as_bytes()));
        }
        fnv1a(&s)
    }

    /// `{type:'anim', now, anims}`: Web Animations records, or script control of CSS animations and
    /// transitions (`css`). `clock_offset_ms` maps the sandbox clock onto the renderer's.
    pub fn apply_web(&mut self, message: &Json, clock_offset_ms: f64) {
        const MAX_ANIMATIONS: usize = 1024;
        let mut caches = lock(&self.caches);
        let mut count = 0usize;
        for item in message.get("anims").arr() {
            count += 1;
            if count > 4096 {
                break;
            }
            let Some(aid) = item.find("aid") else { continue };
            if !aid.is_number() {
                continue;
            }
            if let Some(css_record) = item.find("css")
                && css_record.is_object()
            {
                let target = item.find("target");
                let kind = css_record.find("kind");
                let name = css_record.find("name");
                let origin = css_record.find("origin");
                let (Some(target), Some(kind), Some(name), Some(origin)) = (target, kind, name, origin) else { continue };
                if !target.is_number() || !name.is_string() || !origin.is_number() {
                    continue;
                }
                if kind.as_str() != b"animation" && kind.as_str() != b"transition" {
                    continue;
                }
                let id = i64::from(target.as_int(0));
                let key = concat(&[kind.as_str(), b":", name.as_str()]);
                if item.get("remove").as_bool(false) {
                    if let Some(o) = self.overrides.get_mut(&id) {
                        o.remove(&key);
                        if o.is_empty() {
                            self.overrides.remove(&id);
                        }
                    }
                    continue;
                }
                let rate = if item.get("playbackRate").is_number() { item.get("playbackRate").as_number(0.0) } else { 1.0 };
                if !rate.is_finite() || rate.abs() > 1000.0 {
                    continue;
                }
                let start = item.get("startTime");
                let hold = item.get("holdTime");
                let o = Override {
                    origin: origin.as_number(0.0) / 1000.0,
                    rate,
                    cancelled: item.get("cancelled").as_bool(false),
                    start: start.is_number().then(|| (start.as_number(0.0) + clock_offset_ms) / 1000.0),
                    hold: hold.is_number().then(|| hold.as_number(0.0) / 1000.0),
                };
                if !o.cancelled && o.start.is_none() && o.hold.is_none() {
                    continue;
                }
                if o.start.is_some_and(|v| !v.is_finite()) || o.hold.is_some_and(|v| !v.is_finite()) {
                    continue;
                }
                if !self.overrides.contains_key(&id) && self.overrides.len() >= MAX_ANIMATIONS {
                    continue;
                }
                self.overrides.entry(id).or_default().insert(key, o);
                continue;
            }
            let key = i64::from(aid.as_int(0));
            if item.get("remove").as_bool(false) {
                self.web.remove(&key);
                continue;
            }
            let Some(mut animation) = WebAnimation::parse(&mut caches.timings, item, clock_offset_ms) else {
                self.web.remove(&key);
                continue;
            };
            if !self.web.contains_key(&key) && self.web.len() >= MAX_ANIMATIONS {
                continue;
            }
            let mut heavy = AnimationPaint::default();
            for frame in &animation.frames {
                heavy.keyframe(Some(frame.offset));
                for (property, value) in &frame.values {
                    heavy.property(property, Some(value));
                }
            }
            animation.heavy = heavy.result();
            self.web.insert(key, animation);
        }
    }

    /// The motions at `now` into `out` when it is given, and a hash of the set either way: equal sets
    /// hash equal. `&mut self` for the C++'s `mutable` cache of each @keyframes rule's paint.
    pub fn heavy_motion(&mut self, now: f64, mut out: Option<&mut Vec<Motion>>) -> u64 {
        if let Some(out) = out.as_deref_mut() {
            out.clear();
        }
        let mut hash = 0u64;
        let mut add = |out: &mut Option<&mut Vec<Motion>>, id: i64, kind: u8, name: &[u8], aid: i64, p: BoxPaint| {
            // A sum of mixed members: the same set in any order hashes the same.
            let mut h = fnv1a(name) ^ (id as u64).wrapping_mul(0x9E3779B97F4A7C15) ^ (aid as u64).wrapping_mul(0xC2B2AE3D27D4EB4F);
            h ^= (u64::from(kind) << 56) ^ (u64::from(p.backdrop) << 48);
            h = h.wrapping_mul(0xFF51AFD7ED558CCD);
            hash = hash.wrapping_add(h ^ (h >> 33));
            if let Some(out) = out.as_deref_mut() {
                out.push(Motion { id, kind, name: name.to_vec(), aid, paint: p });
            }
        };
        for (&id, list) in &self.running {
            for r in list {
                if r.ended {
                    continue;
                }
                let cached = *self.keyframes_paint.entry(r.spec.name.clone()).or_insert_with(|| {
                    self.keyframes.get(&r.spec.name).and_then(|frames| {
                        let mut scan = AnimationPaint::default();
                        for frame in frames {
                            scan.keyframe(Some(frame.offset));
                            for (property, value) in &frame.declarations {
                                scan.property(property, Some(value));
                            }
                        }
                        scan.result()
                    })
                });
                let Some(paint) = cached else { continue };
                let mut local = r.local_time(now);
                if !self.overrides.is_empty()
                    && let Some(o) = override_in(&self.overrides, id, &key(b"animation:", &r.spec.name))
                {
                    if o.cancelled {
                        continue;
                    }
                    local = o.local(now);
                }
                local -= r.spec.delay;
                if local < 0.0 || local >= r.spec.iterations * max(r.spec.duration, 0.0001) {
                    continue;
                }
                add(&mut out, id, b'a', &r.spec.name, 0, paint);
            }
        }
        for (&id, list) in &self.transitions {
            for (property, t) in list {
                let Some(paint) = t.heavy else { continue };
                let mut elapsed = now - t.start;
                if !self.overrides.is_empty()
                    && let Some(o) = override_in(&self.overrides, id, &key(b"transition:", property))
                {
                    if o.cancelled {
                        continue;
                    }
                    elapsed = o.local(now);
                }
                elapsed -= t.spec.delay;
                if elapsed < 0.0 || elapsed >= t.spec.duration {
                    continue;
                }
                add(&mut out, id, b't', property, 0, paint);
            }
        }
        for (&aid, a) in &self.web {
            let Some(paint) = a.heavy else { continue };
            if !a.running() {
                continue;
            }
            let Some(state) = compute(a, local_time(a, now), a.rate >= 0.0) else { continue };
            let finished = if a.rate > 0.0 { state.after } else { state.before };
            if finished {
                continue;
            }
            add(&mut out, a.target, b'w', b"", aid, paint);
        }
        hash
    }

    /// Targets of Web Animations, whose underlying values the renderer must supply.
    pub fn web_targets(&self) -> Vec<i64> {
        let mut out: Vec<i64> = Vec::new();
        for a in self.web.values() {
            if !out.contains(&a.target) {
                out.push(a.target);
            }
        }
        out
    }

    /// Properties the Web Animations on `id` animate.
    pub fn web_properties(&self, id: i64) -> Vec<Vec<u8>> {
        let mut out: Vec<Vec<u8>> = Vec::new();
        for a in self.web.values() {
            if a.target == id {
                for p in &a.properties {
                    if !out.contains(p) {
                        out.push(p.clone());
                    }
                }
            }
        }
        out
    }

    /// The `anims` query: running CSS animations and transitions of `ids`.
    pub fn css_animations(&self, ids: &[i64], now: f64) -> Json {
        let mut list = Json::array();
        for &id in ids {
            if let Some(transitions) = self.transitions.get(&id) {
                for (property, transition) in transitions {
                    let o = override_in(&self.overrides, id, &key(b"transition:", property));
                    if o.is_some_and(|o| o.cancelled) {
                        continue;
                    }
                    let mut e = Json::object();
                    e.set("kind", "transition").set("id", id as f64).set("name", property.clone()).set("index", 0);
                    e.set("origin", transition.start * 1000.0)
                        .set("currentTime", o.map_or(now - transition.start, |o| o.local(now)) * 1000.0);
                    e.set("playbackRate", o.map_or(1.0, |o| o.rate)).set("paused", o.is_some_and(|o| o.hold.is_some()));
                    e.set("duration", transition.spec.duration * 1000.0).set("delay", transition.spec.delay * 1000.0).set("iterations", 1);
                    e.set("direction", "normal").set("fill", "backwards").set("easing", transition.spec.timing.text.clone());
                    list.push(e);
                }
            }
            if let Some(running) = self.running.get(&id) {
                for (i, item) in running.iter().enumerate() {
                    if !self.keyframes.contains_key(&item.spec.name) {
                        continue;
                    }
                    let o = override_in(&self.overrides, id, &key(b"animation:", &item.spec.name));
                    if o.is_some_and(|o| o.cancelled) {
                        continue;
                    }
                    let mut e = Json::object();
                    e.set("kind", "animation").set("id", id as f64).set("name", item.spec.name.clone()).set("index", i as i32);
                    e.set("origin", item.start * 1000.0)
                        .set("currentTime", o.map_or_else(|| item.local_time(now), |o| o.local(now)) * 1000.0);
                    e.set("playbackRate", o.map_or(1.0, |o| o.rate)).set("paused", o.map_or(item.spec.paused, |o| o.hold.is_some()));
                    e.set("duration", item.spec.duration * 1000.0).set("delay", item.spec.delay * 1000.0);
                    e.set("direction", item.spec.direction.clone())
                        .set("fill", item.spec.fill.clone())
                        .set("easing", item.spec.timing_text.clone());
                    e.set("iterations", if item.spec.iterations.is_finite() { Json::Number(item.spec.iterations) } else { Json::Null });
                    list.push(e);
                }
            }
        }
        list
    }

    /// Drops Web Animations whose target left the document.
    pub fn prune(&mut self, alive: impl Fn(i64) -> bool) {
        self.web.retain(|_, a| alive(a.target));
    }
}

fn prune_overrides(
    overrides: &mut Overrides,
    running: &BTreeMap<i64, Vec<Running>>,
    transitions: &BTreeMap<i64, BTreeMap<Vec<u8>, Transition>>,
) {
    overrides.retain(|&id, list| {
        list.retain(|key, o| {
            if let Some(name) = key.strip_prefix(b"animation:") {
                running.get(&id).is_some_and(|r| r.iter().any(|x| x.spec.name == name && (x.start - o.origin).abs() < 0.002))
            } else {
                let property = key.get(11..).unwrap_or_default();
                let mut alive = o.cancelled;
                if let Some(p) = transitions.get(&id).and_then(|t| t.get(property)) {
                    alive = (p.start - o.origin).abs() < 0.002;
                }
                alive
            }
        });
        !list.is_empty()
    });
}
