//! Heavy boxes (`core/src/paint_policy.h`, `paint_policy.cpp`; widget-sandbox's paint-policy.ts and
//! docs/decisions.md "Heavy boxes"): the paint limit every renderer enforces. A box is heavy when its computed
//! style has a blurred box-shadow layer, a filter, a backdrop-filter or opacity below 1 (a disabled or pressed
//! `<button>`'s own dimming aside), or while an animation of one of them runs. At most [`MAX_HEAVY_BOXES`] at
//! once, of which [`MAX_BACKDROP_BOXES`] with backdrop-filter. The rules take CSS text, as the shared fixture
//! writes it: the value counts, not the picture.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::hash::Hash;

use twidget_cstd::{is_space, strtod};

/// widget-sandbox's `kMaxHeavyBoxes` and `kMaxBackdropBoxes` (twidget/renderer.h).
pub const MAX_HEAVY_BOXES: i32 = 16;
pub const MAX_BACKDROP_BOXES: i32 = 2;

/// widget-sandbox's `WidgetPaintLimit`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintLimit {
    /// "paint-limit": more than MAX_HEAVY_BOXES heavy boxes; the count is the heavy boxes'.
    Paint,
    /// "backdrop-limit": more than MAX_BACKDROP_BOXES boxes with backdrop-filter; the count is theirs.
    Backdrop,
}

impl PaintLimit {
    pub fn code(self) -> &'static str {
        match self {
            PaintLimit::Backdrop => "backdrop-limit",
            PaintLimit::Paint => "paint-limit",
        }
    }

    pub fn from_code(code: &[u8]) -> Option<PaintLimit> {
        match code {
            b"paint-limit" => Some(PaintLimit::Paint),
            b"backdrop-limit" => Some(PaintLimit::Backdrop),
            _ => None,
        }
    }

    /// The reason the host stops the widget with, the same on every platform.
    pub fn message(self, count: i32) -> String {
        match self {
            PaintLimit::Backdrop => format!("Paint limit exceeded: {count} boxes with backdrop-filter (max {MAX_BACKDROP_BOXES})"),
            PaintLimit::Paint => {
                format!("Paint limit exceeded: {count} boxes with shadows, filters or transparency (max {MAX_HEAVY_BOXES})")
            }
        }
    }
}

/// What one box adds: to the heavy count, and to the backdrop-filter count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoxPaint {
    pub heavy: bool,
    pub backdrop: bool,
}

/// A `<button>` with the `disabled` attribute, or held down (:active): its opacity is its own dimming.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Button {
    #[default]
    None,
    Disabled,
    Pressed,
}

/// A box's computed values as CSS text; an empty value is the initial one.
#[derive(Clone, Copy, Debug, Default)]
pub struct Style<'a> {
    pub display: &'a [u8],
    pub content: &'a [u8],
    pub box_shadow: &'a [u8],
    pub filter: &'a [u8],
    pub backdrop_filter: &'a [u8],
    pub opacity: &'a [u8],
    /// Ignored for a pseudo-element, a box of its own.
    pub button: Button,
}

fn trim(mut s: &[u8]) -> &[u8] {
    while let [first, rest @ ..] = s
        && is_space(*first)
    {
        s = rest;
    }
    while let [rest @ .., last] = s
        && is_space(*last)
    {
        s = rest;
    }
    s
}

fn equals_lower(s: &[u8], lower: &[u8]) -> bool {
    s.len() == lower.len() && s.iter().zip(lower).all(|(a, b)| a.to_ascii_lowercase() == *b)
}

fn is_none(v: &[u8]) -> bool {
    let v = trim(v);
    v.is_empty() || equals_lower(v, b"none")
}

/// `strtod(text.c_str(), …)`: what a C string of these bytes reads, up to a NUL.
fn c_strtod(text: &[u8]) -> (f64, usize) {
    let end = text.iter().position(|c| *c == 0).unwrap_or(text.len());
    strtod(&text[..end])
}

/// The comma-separated layers of a list value, commas inside parentheses kept; empty layers dropped.
fn layers<'a>(value: &'a [u8], mut visit: impl FnMut(&'a [u8])) {
    let mut depth = 0;
    let mut from = 0;
    for i in 0..=value.len() {
        let c = if i < value.len() { value[i] } else { b',' };
        if c == b'(' {
            depth += 1;
        } else if c == b')' {
            depth = if depth > 0 { depth - 1 } else { 0 };
        } else if c == b',' && (depth == 0 || i == value.len()) {
            let layer = trim(&value[from..i]);
            if !layer.is_empty() {
                visit(layer);
            }
            from = i + 1;
        }
    }
}

/// A CSS length token: a number with an optional exponent, then any unit letters or %.
fn length_token(t: &[u8]) -> Option<f64> {
    let mut i = 0;
    if i < t.len() && (t[i] == b'+' || t[i] == b'-') {
        i += 1;
    }
    let mut digits = 0;
    while i < t.len() && t[i].is_ascii_digit() {
        i += 1;
        digits += 1;
    }
    if i < t.len() && t[i] == b'.' {
        i += 1;
        while i < t.len() && t[i].is_ascii_digit() {
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return None;
    }
    if i < t.len() && (t[i] == b'e' || t[i] == b'E') {
        let mut j = i + 1;
        if j < t.len() && (t[j] == b'+' || t[j] == b'-') {
            j += 1;
        }
        let exp = j;
        while j < t.len() && t[j].is_ascii_digit() {
            j += 1;
        }
        if j > exp {
            i = j;
        }
    }
    if !t[i..].iter().all(|c| c.is_ascii_alphabetic() || *c == b'%') {
        return None;
    }
    Some(c_strtod(t).0)
}

/// Whether a shadow layer's third length, its blur radius, is above 0; a math function is a length of
/// unknown size.
fn layer_blurred(layer: &[u8]) -> bool {
    let mut found = 0;
    let mut i = 0;
    while i < layer.len() {
        if is_space(layer[i]) {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < layer.len() && !is_space(layer[j]) && layer[j] != b'(' {
            j += 1;
        }
        let token = &layer[i..j];
        if j < layer.len() && layer[j] == b'(' {
            // a colour function is skipped, a math one is a length
            let mut depth = 0;
            let mut k = j;
            while k < layer.len() {
                if layer[k] == b'(' {
                    depth += 1;
                } else if layer[k] == b')' {
                    depth -= 1;
                    if depth == 0 {
                        k += 1;
                        break;
                    }
                }
                k += 1;
            }
            if equals_lower(token, b"calc") || equals_lower(token, b"min") || equals_lower(token, b"max") || equals_lower(token, b"clamp") {
                found += 1;
                if found == 3 {
                    return true;
                }
            }
            i = k;
            continue;
        }
        if let Some(length) = length_token(token) {
            found += 1;
            if found == 3 {
                #[allow(clippy::neg_cmp_op_on_partial_ord)]
                return !(length <= 0.0);
            }
        }
        i = j;
    }
    false
}

/// Rule 4: a number or a percentage below 1; what is no number is 1.
fn transparent(v: &[u8]) -> bool {
    let v = trim(v);
    if v.is_empty() {
        return false;
    }
    let (mut n, taken) = c_strtod(v);
    if taken == 0 {
        return false;
    }
    if v.last() == Some(&b'%') {
        n /= 100.0;
    }
    n < 1.0
}

/// Rule 1: a layer of `box-shadow` has a blur radius (its third length) above 0; one of unknown size
/// (calc()) counts.
pub fn box_shadow_blurred(value: &[u8]) -> bool {
    if is_none(value) {
        return false;
    }
    let mut blurred = false;
    layers(value, |layer| blurred = blurred || layer_blurred(layer));
    blurred
}

/// The layers without blur, "none" when nothing is left: what a stopped widget keeps.
pub fn box_shadow_without_blur(value: &[u8]) -> Vec<u8> {
    if is_none(value) {
        return b"none".to_vec();
    }
    let mut out = Vec::new();
    layers(value, |layer| {
        if layer_blurred(layer) {
            return;
        }
        if !out.is_empty() {
            out.extend_from_slice(b", ");
        }
        out.extend_from_slice(layer);
    });
    if out.is_empty() { b"none".to_vec() } else { out }
}

/// `display: none` on an element: nothing under it builds a box either (`contents` hides nothing).
pub fn hides_subtree(display: &[u8]) -> bool {
    equals_lower(trim(display), b"none")
}

/// The box an element (or with `pseudo` its ::before / ::after) builds and what it adds; None when it
/// builds none. Rules 1 to 4; a running animation (rule 5) is [`AnimationPaint`]'s.
pub fn box_paint(style: &Style, pseudo: bool) -> Option<BoxPaint> {
    let display = trim(style.display);
    if equals_lower(display, b"none") || (!pseudo && equals_lower(display, b"contents")) {
        return None;
    }
    if pseudo {
        let content = trim(style.content);
        if content.is_empty() || equals_lower(content, b"none") || equals_lower(content, b"normal") {
            return None;
        }
    }
    let backdrop = !is_none(style.backdrop_filter);
    let dimmed = !pseudo && style.button != Button::None;
    let heavy = backdrop || !is_none(style.filter) || (!dimmed && transparent(style.opacity)) || box_shadow_blurred(style.box_shadow);
    Some(BoxPaint { heavy, backdrop })
}

/// Rule 5: what a running animation adds to its box, from its keyframes fed in order. It counts when it
/// changes filter, backdrop-filter or opacity, or box-shadow through a blurred layer, a value that is no
/// string (unknown), or a missing first or last keyframe. Shadows without blur do not count.
#[derive(Clone, Debug, Default)]
pub struct AnimationPaint {
    at: Option<f64>,
    heavy: bool,
    backdrop: bool,
    shadow: bool,
    shadow_blurred: bool,
    shadow_first: bool,
    shadow_last: bool,
}

impl AnimationPaint {
    /// The next keyframe; its offset (the computed one when implicit), None when it has none.
    pub fn keyframe(&mut self, offset: Option<f64>) {
        self.at = offset;
    }

    /// One member of it, by CSS or camelCase name, vendor prefix allowed; `value` None when it is no
    /// string. `offset`, `computedOffset`, `easing` and `composite` are no properties.
    pub fn property(&mut self, name: &[u8], value: Option<&[u8]>) {
        if equals_lower(name, b"offset")
            || equals_lower(name, b"computedoffset")
            || equals_lower(name, b"easing")
            || equals_lower(name, b"composite")
        {
            return;
        }
        // camelCase (as getKeyframes names them) to the CSS name, the vendor prefix dropped
        let mut css = Vec::with_capacity(name.len() + 4);
        for &c in name {
            if c.is_ascii_uppercase() {
                css.push(b'-');
            }
            css.push(c.to_ascii_lowercase());
        }
        let mut n: &[u8] = &css;
        for prefix in [&b"-webkit-"[..], b"-moz-", b"-ms-", b"-o-"] {
            if n.starts_with(prefix) {
                n = &n[prefix.len()..];
                break;
            }
        }
        match n {
            b"backdrop-filter" => {
                self.heavy = true;
                self.backdrop = true;
            }
            b"filter" | b"opacity" => self.heavy = true,
            b"box-shadow" => {
                self.shadow = true;
                if value.is_none_or(box_shadow_blurred) {
                    self.shadow_blurred = true;
                }
                if self.at == Some(0.0) {
                    self.shadow_first = true;
                }
                if self.at == Some(1.0) {
                    self.shadow_last = true;
                }
            }
            _ => {}
        }
    }

    pub fn result(&self) -> Option<BoxPaint> {
        let heavy = self.heavy || (self.shadow && (self.shadow_blurred || !self.shadow_first || !self.shadow_last));
        heavy.then_some(BoxPaint { heavy: true, backdrop: self.backdrop })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub code: PaintLimit,
    pub count: i32,
}

#[derive(Clone, Debug)]
struct Anim {
    id: Vec<u8>,
    n: i32,
    backdrop: bool,
}

#[derive(Clone, Debug, Default)]
struct Box {
    paint: Option<BoxPaint>,
    anims: Vec<Anim>,
    seen: u32,
}

impl Box {
    fn contribution(&self) -> BoxPaint {
        let Some(mut out) = self.paint else { return BoxPaint::default() };
        for a in &self.anims {
            out.heavy = true;
            out.backdrop = out.backdrop || a.backdrop;
        }
        out
    }
}

/// The ledger of heavy boxes by any key the renderer gives a box (an element and each of its
/// pseudo-elements a key of their own). It keeps both counts current as boxes change and animations
/// start and end, and gives the verdict before a frame is drawn; the first one past a limit sticks.
#[derive(Clone, Debug)]
pub struct PaintCounter<K: Eq + Hash + Clone> {
    boxes: HashMap<K, Box>,
    heavy: i32,
    backdrop: i32,
    peak_heavy: i32,
    peak_backdrop: i32,
    scan: u32,
    stopped: Option<Verdict>,
}

impl<K: Eq + Hash + Clone> Default for PaintCounter<K> {
    fn default() -> Self {
        PaintCounter { boxes: HashMap::new(), heavy: 0, backdrop: 0, peak_heavy: 0, peak_backdrop: 0, scan: 0, stopped: None }
    }
}

impl<K: Eq + Hash + Clone> PaintCounter<K> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn heavy(&self) -> i32 {
        self.heavy
    }

    pub fn backdrop(&self) -> i32 {
        self.backdrop
    }

    /// The most there were at once.
    pub fn peak_heavy(&self) -> i32 {
        self.peak_heavy
    }

    pub fn peak_backdrop(&self) -> i32 {
        self.peak_backdrop
    }

    /// The box's computed values, or None: it builds no box now. Its animations stay.
    pub fn set_box(&mut self, key: &K, paint: Option<BoxPaint>) {
        let scan = self.scan;
        let known = match self.boxes.get_mut(key) {
            Some(b) => {
                b.seen = scan;
                true
            }
            None => false,
        };
        if paint.is_none() && !known {
            return;
        }
        self.update(key, |b| b.paint = paint);
    }

    /// The box is not rendered (display: none on it or an ancestor) or its node left: its animations go
    /// with it.
    pub fn remove_box(&mut self, key: &K) {
        if self.boxes.contains_key(key) {
            self.update(key, |b| {
                b.paint = None;
                b.anims.clear();
            });
        }
    }

    /// An animation of rule 5 started on the box; `id` tells it from the box's others.
    pub fn start_animation(&mut self, key: &K, id: &[u8], paint: BoxPaint) {
        self.update(key, |b| {
            if let Some(a) = b.anims.iter_mut().find(|a| a.id == id) {
                a.n += 1;
                a.backdrop = a.backdrop || paint.backdrop;
                return;
            }
            b.anims.push(Anim { id: id.to_vec(), n: 1, backdrop: paint.backdrop });
        });
    }

    /// That animation ended or was cancelled; one that never started changes nothing.
    pub fn end_animation(&mut self, key: &K, id: &[u8]) {
        if !self.animating(key, id) {
            return;
        }
        self.update(key, |b| {
            if let Some(at) = b.anims.iter().position(|a| a.id == id) {
                b.anims[at].n -= 1;
                if b.anims[at].n <= 0 {
                    b.anims.remove(at);
                }
            }
        });
    }

    pub fn has_box(&self, key: &K) -> bool {
        self.boxes.get(key).is_some_and(|b| b.paint.is_some())
    }

    pub fn animating(&self, key: &K, id: &[u8]) -> bool {
        self.boxes.get(key).is_some_and(|b| b.anims.iter().any(|a| a.id == id))
    }

    /// None while both counts are within their limits; the first time one is past, the heavy count
    /// (checked first) or the backdrop count, and the same verdict from then on.
    pub fn check(&mut self) -> Option<Verdict> {
        if self.stopped.is_some() {
            return self.stopped;
        }
        if self.heavy > MAX_HEAVY_BOXES {
            self.stopped = Some(Verdict { code: PaintLimit::Paint, count: self.heavy });
        } else if self.backdrop > MAX_BACKDROP_BOXES {
            self.stopped = Some(Verdict { code: PaintLimit::Backdrop, count: self.backdrop });
        }
        self.stopped
    }

    /// A count of every box: set_box for each one the renderer builds between the two; a box it did not
    /// set is gone, as remove_box. True when one took running animations with it.
    pub fn begin_scan(&mut self) {
        self.scan = self.scan.wrapping_add(1);
    }

    pub fn end_scan(&mut self) -> bool {
        let scan = self.scan;
        let mut anims = false;
        let mut heavy = 0;
        let mut backdrop = 0;
        self.boxes.retain(|_, b| {
            if b.seen == scan {
                return true;
            }
            let c = b.contribution();
            heavy += i32::from(c.heavy);
            backdrop += i32::from(c.backdrop);
            anims = anims || !b.anims.is_empty();
            false
        });
        self.heavy -= heavy;
        self.backdrop -= backdrop;
        anims
    }

    fn update(&mut self, key: &K, change: impl FnOnce(&mut Box)) {
        let scan = self.scan;
        let b = self.boxes.entry(key.clone()).or_insert_with(|| Box { seen: scan, ..Box::default() });
        let before = b.contribution();
        change(b);
        let after = b.contribution();
        let empty = b.paint.is_none() && b.anims.is_empty();
        self.heavy += i32::from(after.heavy) - i32::from(before.heavy);
        self.backdrop += i32::from(after.backdrop) - i32::from(before.backdrop);
        if empty {
            self.boxes.remove(key);
        }
        self.peak_heavy = self.peak_heavy.max(self.heavy);
        self.peak_backdrop = self.peak_backdrop.max(self.backdrop);
    }
}
