//! renderer.cpp's `class Container`, the `document_container` half: what the fork asks of the renderer while it parses,
//! lays out and paints. Fonts are the backend's, one face per description shared by every document and font handle,
//! with the widths measured in each (the width cache and its bounds); the text breaker cuts between user-perceived
//! characters; images are `data:` sources the backend decodes, the validator's placeholder and the renderer's own
//! `tw-canvas:` sources, within their table's bounds; media features come from the renderer's width and scale; and
//! every paint call becomes the display list's commands, with the clips, groups, filters, backdrops and SVG paints the
//! container keeps or draws again.
//!
//! The C++ container reads the renderer (`r_.width_`, `r_.dpr_`, `r_.canvases_`) and pushes its violations: here those
//! are fields the engine sets ([`Container::view`]) and a list it takes. What paint reads of an element (a filter's
//! owner's colour, a group's owner that is a disabled button) is gathered before each paint
//! ([`Container::prepare_paint`]), since the document is drawing while the container is asked.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use twidget_ends::Backend;
use twidget_litehtml::Px;
use twidget_litehtml::background::{BackgroundLayer, ConicGradient, GradientBase, LinearGradient, RadialGradient};
use twidget_litehtml::borders::{BorderRadiuses, Borders};
use twidget_litehtml::css_effects::{FilterKind, FilterPaint, PaintGroup, ShadowPaint, TransformMatrix, VectorPath};
use twidget_litehtml::css_tokenizer::CssToken;
use twidget_litehtml::css_values::*;
use twidget_litehtml::document_container::{DocumentContainer, ListMarker};
use twidget_litehtml::element::ElementId;
use twidget_litehtml::font_description::FontDescription;
use twidget_litehtml::types::{FontMetrics as LhMetrics, MediaFeatures, Position, Size};
use twidget_litehtml::web_color::{ResolveColor, WebColor};
use twidget_wire::{Cmd, Color, Command, DisplayList, Filter, Filters, FontMetrics, Path, PathOp, Radii, Rect, Stop};

use crate::cxx::{cmax, cmin};

/// renderer.cpp's `MIN_HEIGHT`: the container's viewport height before its first build.
pub const MIN_HEIGHT: i32 = 50;
/// Backend fonts kept for documents to come, beyond those a document holds.
pub const MAX_FACES: usize = 256;
/// The SVG paints the container keeps for draw_cached, and the commands they hold together.
pub const MAX_CACHED_PAINTS: usize = 128;
pub const MAX_CACHED_COMMANDS: usize = 50000;
/// A repeated background is recorded tile by tile while the display list is shorter than this; then once per layer.
pub const MAX_TILED_COMMANDS: usize = 65536;
/// Measured widths, keyed by their text: longer text is measured each time, and every face's keys together stay
/// within the byte bound.
pub const MAX_WIDTH_ENTRIES: usize = 4096;
pub const MAX_WIDTH_KEY: usize = 256;
pub const MAX_WIDTH_BYTES: usize = 1 << 20;
/// The profile's image types (widget-sandbox's PROFILE_IMAGES).
pub const IMAGE_TYPES: [&[u8]; 5] = [b"image/png", b"image/jpeg", b"image/jpg", b"image/gif", b"image/webp"];
/// The image sources a widget names over its life, and their bytes together: past either a new one is refused.
pub const MAX_IMAGES: usize = 4096;
pub const MAX_IMAGE_BYTES: usize = 32 << 20;

pub(crate) fn color_of(c: &WebColor) -> Color {
    Color { r: c.red, g: c.green, b: c.blue, a: c.alpha }
}

pub(crate) fn rect_of(p: &Position) -> Rect {
    Rect::new(p.x.value(), p.y.value(), p.width.value(), p.height.value())
}

pub(crate) fn radii_of(r: &BorderRadiuses) -> Radii {
    Radii {
        tlx: r.top_left_x.value(),
        tly: r.top_left_y.value(),
        trx: r.top_right_x.value(),
        try_: r.top_right_y.value(),
        brx: r.bottom_right_x.value(),
        bry: r.bottom_right_y.value(),
        blx: r.bottom_left_x.value(),
        bly: r.bottom_left_y.value(),
    }
}

/// `text` up to its first NUL, as a `const char*` holds it.
pub(crate) fn c_str(text: &[u8]) -> &[u8] {
    text.iter().position(|&c| c == 0).map_or(text, |n| &text[..n])
}

/// Where letter-spacing goes: after each character, with combining marks, variation selectors, emoji modifiers and
/// ZWJ sequences kept on the character before them (`spacing_units`).
pub(crate) fn spacing_units(text: &[u8]) -> Vec<&[u8]> {
    let mut units = Vec::new();
    let mut start = 0;
    let mut joined = false;
    let mut i = 0;
    while i < text.len() {
        let c = text[i];
        let n = (if c < 0x80 {
            1
        } else if (c >> 5) == 6 {
            2
        } else if (c >> 4) == 14 {
            3
        } else {
            4
        })
        .min(text.len() - i);
        let mut cp: u32 = if c < 0x80 {
            u32::from(c)
        } else if n == 2 {
            u32::from(c & 0x1F)
        } else if n == 3 {
            u32::from(c & 0x0F)
        } else {
            u32::from(c & 0x07)
        };
        for k in 1..n {
            cp = (cp << 6) | u32::from(text[i + k] & 0x3F);
        }
        let extends = (0x300..=0x36F).contains(&cp)
            || (0x1AB0..=0x1AFF).contains(&cp)
            || (0x1DC0..=0x1DFF).contains(&cp)
            || (0x20D0..=0x20FF).contains(&cp)
            || (0xFE00..=0xFE0F).contains(&cp)
            || (0xFE20..=0xFE2F).contains(&cp)
            || (0x1F3FB..=0x1F3FF).contains(&cp)
            || (0xE0020..=0xE007F).contains(&cp)
            || cp == 0x200D;
        if i > 0 && !extends && !joined {
            units.push(&text[start..i]);
            start = i;
        }
        joined = cp == 0x200D;
        i += n;
    }
    if start < text.len() {
        units.push(&text[start..]);
    }
    units
}

/// The spaces word-spacing widens: U+0020 and U+00A0 (`spacing_spaces`).
pub(crate) fn spacing_spaces(text: &[u8]) -> usize {
    let mut n = 0;
    for i in 0..text.len() {
        if text[i] == b' ' || (text[i] == 0xC2 && i + 1 < text.len() && text[i + 1] == 0xA0) {
            n += 1;
        }
    }
    n
}

/// `text` cut around the spaces word-spacing widens, each space on its own (`spacing_words`).
pub(crate) fn spacing_words(text: &[u8]) -> Vec<&[u8]> {
    let mut words = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < text.len() {
        let n = if text[i] == b' ' {
            1
        } else if text[i] == 0xC2 && i + 1 < text.len() && text[i + 1] == 0xA0 {
            2
        } else {
            0
        };
        if n == 0 {
            i += 1;
            continue;
        }
        if i > start {
            words.push(&text[start..i]);
        }
        words.push(&text[i..i + n]);
        i += n;
        start = i;
    }
    if start < text.len() {
        words.push(&text[start..]);
    }
    words
}

fn lower(s: &[u8]) -> Vec<u8> {
    s.iter().map(u8::to_ascii_lowercase).collect()
}

/// A data: URL's media type as widget-sandbox's checkUrl reads it (`data_mime`): the scheme in any case, around tabs,
/// newlines and the C0 controls and spaces at its ends; trimmed and lower-cased. Empty when not a data: URL.
pub fn data_mime(raw: &[u8]) -> Vec<u8> {
    let url: Vec<u8> = raw.iter().copied().filter(|&c| c != b'\t' && c != b'\n' && c != b'\r').collect();
    let (mut from, mut to) = (0, url.len());
    while from < to && url[from] <= 0x20 {
        from += 1;
    }
    while to > from && url[to - 1] <= 0x20 {
        to -= 1;
    }
    let u = &url[from..to];
    if u.len() < 5 || lower(&u[..5]) != b"data:" {
        return Vec::new();
    }
    let Some(end) = u[5..].iter().position(|&c| c == b',' || c == b';').map(|e| e + 5) else { return Vec::new() };
    if !u[5..].contains(&b',') {
        return Vec::new();
    }
    twidget_css::trim(&lower(&u[5..end]))
}

/// `sscanf(s, "%d/%dx%d", ...)`: three ints and the separators between them, else None.
fn scan_canvas_source(s: &[u8]) -> Option<(i32, i32, i32)> {
    let mut at = 0;
    let int = |at: &mut usize| -> Option<i32> {
        while *at < s.len() && matches!(s[*at], b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r') {
            *at += 1;
        }
        let mut negative = false;
        if *at < s.len() && (s[*at] == b'+' || s[*at] == b'-') {
            negative = s[*at] == b'-';
            *at += 1;
        }
        let start = *at;
        let mut v: i64 = 0;
        while *at < s.len() && s[*at].is_ascii_digit() {
            v = v.wrapping_mul(10).wrapping_add(i64::from(s[*at] - b'0'));
            *at += 1;
        }
        if *at == start {
            return None;
        }
        Some((if negative { v.wrapping_neg() } else { v }) as i32)
    };
    let id = int(&mut at)?;
    if s.get(at) != Some(&b'/') {
        return None;
    }
    at += 1;
    let w = int(&mut at)?;
    if s.get(at) != Some(&b'x') {
        return None;
    }
    at += 1;
    let h = int(&mut at)?;
    Some((id, w, h))
}

/// `Face`: a backend font for one description, shared by every document and font handle asking for it, with the
/// widths measured in it.
#[derive(Debug, Default)]
struct Face {
    handle: i32,
    metrics: FontMetrics,
    /// litehtml font handles on it.
    uses: i32,
    used: u64,
    widths: HashMap<Vec<u8>, f32>,
    /// This face's share of the key bytes every face holds (`width_bytes_`).
    bytes: usize,
}

/// `FaceKey`: family, size, weight, italic, features, ordered as `std::tuple` orders them.
#[derive(Clone, Debug)]
struct FaceKey {
    family: Vec<u8>,
    size: f32,
    weight: i32,
    italic: bool,
    features: Vec<u8>,
}

impl PartialEq for FaceKey {
    fn eq(&self, other: &FaceKey) -> bool {
        self.cmp(other) == std::cmp::Ordering::Equal
    }
}
impl Eq for FaceKey {}
impl PartialOrd for FaceKey {
    fn partial_cmp(&self, other: &FaceKey) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for FaceKey {
    fn cmp(&self, other: &FaceKey) -> std::cmp::Ordering {
        use std::cmp::Ordering::*;
        // std::tuple's operator<: each member by its own `<`, a float by IEEE `<` (NaN equivalent to anything).
        match self.family.cmp(&other.family) {
            Equal => {}
            o => return o,
        }
        if self.size < other.size {
            return Less;
        }
        if other.size < self.size {
            return Greater;
        }
        match self.weight.cmp(&other.weight) {
            Equal => {}
            o => return o,
        }
        match self.italic.cmp(&other.italic) {
            Equal => {}
            o => return o,
        }
        self.features.cmp(&other.features)
    }
}

/// `Font`: a litehtml font handle: its face, the face's metrics, and what the description draws with.
#[derive(Clone, Debug)]
struct Font {
    face: u64,
    handle: i32,
    metrics: FontMetrics,
    size: f32,
    decoration: i32,
    decoration_color: WebColor,
    letter_spacing: f32,
    word_spacing: f32,
}

/// `Image`.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Image {
    pub id: i32,
    pub canvas: i32,
    pub placeholder: bool,
    pub w: f32,
    pub h: f32,
}

/// `CachedPaint`: an SVG paint recorded under its key, where its box was when it was recorded.
#[derive(Clone, Debug, Default)]
struct CachedPaint {
    x: f32,
    y: f32,
    used: u64,
    commands: Vec<Command>,
}

/// The fonts: faces by description, font handles, the width cache's pool.
#[derive(Debug, Default)]
struct Fonts {
    width_bytes: usize,
    faces: BTreeMap<FaceKey, u64>,
    face_data: HashMap<u64, Face>,
    next_face: u64,
    face_clock: u64,
    fonts: HashMap<u64, Font>,
    next_font: u64,
}

impl Fonts {
    /// `Face::forget`.
    fn forget(&mut self, face: u64) {
        if let Some(f) = self.face_data.get_mut(&face) {
            f.widths.clear();
            self.width_bytes = self.width_bytes.wrapping_sub(f.bytes);
            f.bytes = 0;
        }
    }

    /// `Face::width`.
    fn face_width(&mut self, backend: &mut dyn Backend, face: u64, text: &[u8]) -> f32 {
        let Some(f) = self.face_data.get(&face) else { return 0.0 };
        if let Some(&w) = f.widths.get(text) {
            return w;
        }
        let w = backend.text_width(f.handle, text);
        // Words repeat across documents, but a widget that types a counter would grow this forever.
        if text.len() > MAX_WIDTH_KEY {
            return w;
        }
        if f.widths.len() >= MAX_WIDTH_ENTRIES || self.width_bytes + text.len() > MAX_WIDTH_BYTES {
            self.forget(face);
        }
        if self.width_bytes + text.len() > MAX_WIDTH_BYTES {
            return w; // other faces hold the budget
        }
        if let Some(f) = self.face_data.get_mut(&face) {
            f.widths.insert(text.to_vec(), w);
            f.bytes += text.len();
        }
        self.width_bytes += text.len();
        w
    }

    /// `Font::width`.
    fn font_width(&mut self, backend: &mut dyn Backend, font: &Font, text: &[u8]) -> f32 {
        let mut w = self.face_width(backend, font.face, text);
        if font.letter_spacing != 0.0 {
            w += font.letter_spacing * spacing_units(text).len() as f32;
        }
        if font.word_spacing != 0.0 {
            w += font.word_spacing * spacing_spaces(text) as f32;
        }
        w
    }

    /// Drops face `id` (`faces_.erase`): its widths' bytes go back to the pool.
    fn erase_face(&mut self, key: &FaceKey) {
        if let Some(id) = self.faces.remove(key) {
            self.forget(id);
            self.face_data.remove(&id);
        }
    }
}

/// The images: by source, the next id, the table's bounds.
#[derive(Debug, Default)]
pub(crate) struct Images {
    pub map: HashMap<Vec<u8>, Image>,
    pub next_image: i32,
    pub sources: usize,
    pub bytes: usize,
    pub full: bool,
}

/// What a paint is drawing into: the list, the clips pushed and suspended, the groups' accumulated matrices, and the
/// SVG paints kept.
#[derive(Debug, Default)]
struct Paint {
    list: DisplayList,
    clips: Vec<Command>,
    suspended: Vec<Vec<Command>>,
    groups: Vec<TransformMatrix>,
    cached: HashMap<u64, CachedPaint>,
    cached_commands: usize,
    cached_clock: u64,
}

/// `TextBreaker`: where overflow-wrap may break a word that does not fit, between user-perceived characters, measured
/// through the container.
#[derive(Clone, Debug)]
struct Breaker {
    text: Vec<u8>,
    font: u64,
    boundaries: Vec<usize>,
    minimum: f32,
}

impl Breaker {
    fn new(text: &[u8], font: u64) -> Breaker {
        let mut boundaries = vec![0usize];
        let ascii = text.iter().all(|&c| (0x20..0x7f).contains(&c));
        if ascii {
            for i in 1..=text.len() {
                boundaries.push(i);
            }
            return Breaker { text: text.to_vec(), font, boundaries, minimum: -1.0 };
        }
        let decode = |at: usize| -> (usize, u32) {
            let c = text[at];
            let mut n = if c < 0x80 {
                1
            } else if (c >> 5) == 6 {
                2
            } else if (c >> 4) == 14 {
                3
            } else if (c >> 3) == 30 {
                4
            } else {
                1
            };
            if at + n > text.len() {
                n = 1;
            }
            let mut cp: u32 = if n == 1 {
                u32::from(c)
            } else if n == 2 {
                u32::from(c & 0x1f)
            } else if n == 3 {
                u32::from(c & 0x0f)
            } else {
                u32::from(c & 0x07)
            };
            for k in 1..n {
                cp = (cp << 6) | u32::from(text[at + k] & 0x3f);
            }
            (n, cp)
        };
        let extends = |cp: u32| {
            (0x300..=0x36f).contains(&cp)
                || (0x483..=0x489).contains(&cp)
                || (0x591..=0x5bd).contains(&cp)
                || (0x610..=0x61a).contains(&cp)
                || (0x64b..=0x65f).contains(&cp)
                || (0x900..=0x903).contains(&cp)
                || (0x93a..=0x94f).contains(&cp)
                || (0x1ab0..=0x1aff).contains(&cp)
                || (0x1dc0..=0x1dff).contains(&cp)
                || (0x200c..=0x200d).contains(&cp)
                || (0x20d0..=0x20ff).contains(&cp)
                || (0xfe00..=0xfe0f).contains(&cp)
                || (0xfe20..=0xfe2f).contains(&cp)
                || (0x1f3fb..=0x1f3ff).contains(&cp)
                || (0xe0020..=0xe007f).contains(&cp)
                || (0xe0100..=0xe01ef).contains(&cp)
        };
        let regional = |cp: u32| (0x1f1e6..=0x1f1ff).contains(&cp);
        let mut previous = 0u32;
        let mut regionals = 0i32;
        let mut at = 0;
        while at < text.len() {
            let (n, cp) = decode(at);
            let joined = at > 0 && (extends(cp) || previous == 0x200d || (regional(cp) && regionals % 2 == 1));
            regionals = if regional(cp) { regionals.wrapping_add(1) } else { 0 };
            if at > 0 && !joined {
                boundaries.push(at);
            }
            previous = cp;
            at += n;
        }
        boundaries.push(text.len());
        Breaker { text: text.to_vec(), font, boundaries, minimum: -1.0 }
    }
}

/// renderer.cpp's `Container` as litehtml's `document_container`. `K` is the backend it makes fonts, measures text and
/// decodes images with: the renderer's own, shared (the C++ holds a reference to the renderer's).
#[derive(Debug)]
pub struct Container<K: Backend> {
    backend: RefCell<K>,
    /// `r_.width_` and `r_.dpr_`, and the container's `vp_height_`.
    width: Cell<f64>,
    dpr: Cell<f64>,
    vp_height: Cell<i32>,
    /// `r_.canvases_`: each held canvas's id, width and height.
    canvases: RefCell<Vec<(i32, i32, i32)>>,
    /// What the container pushed to `r_.pending_violations_`, until the engine takes it.
    violations: RefCell<Vec<Vec<u8>>>,
    trusted_styles: Cell<bool>,
    cursor: RefCell<Vec<u8>>,
    fonts: RefCell<Fonts>,
    pub(crate) images: RefCell<Images>,
    paint: RefCell<Paint>,
    breakers: RefCell<HashMap<u64, Breaker>>,
    next_breaker: Cell<u64>,
    no_effects: Cell<bool>,
    /// `litehtml::g_paint_trace` is set: the paint callbacks return at once.
    tracing: Cell<bool>,
    /// What paint reads of elements, gathered before it: a filter's owner's `color`, the `<button disabled>`s.
    owner_colors: RefCell<HashMap<ElementId, WebColor>>,
    disabled_buttons: RefCell<BTreeSet<ElementId>>,
}

impl<K: Backend> Container<K> {
    pub fn new(backend: K) -> Container<K> {
        Container {
            backend: RefCell::new(backend),
            width: Cell::new(0.0),
            dpr: Cell::new(1.0),
            vp_height: Cell::new(MIN_HEIGHT),
            canvases: RefCell::new(Vec::new()),
            violations: RefCell::new(Vec::new()),
            trusted_styles: Cell::new(false),
            cursor: RefCell::new(Vec::new()),
            fonts: RefCell::new(Fonts::default()),
            images: RefCell::new(Images::default()),
            paint: RefCell::new(Paint::default()),
            breakers: RefCell::new(HashMap::new()),
            next_breaker: Cell::new(0),
            no_effects: Cell::new(false),
            tracing: Cell::new(false),
            owner_colors: RefCell::new(HashMap::new()),
            disabled_buttons: RefCell::new(BTreeSet::new()),
        }
    }

    /// The backend, for the engine's own calls and the tests.
    pub fn backend(&self) -> std::cell::RefMut<'_, K> {
        self.backend.borrow_mut()
    }

    pub fn set_width(&self, width: f64, dpr: f64) {
        self.width.set(width);
        self.dpr.set(dpr);
    }
    pub fn set_vp_height(&self, vp_height: i32) {
        self.vp_height.set(vp_height);
    }
    pub fn width(&self) -> f64 {
        self.width.get()
    }
    pub fn vp_height(&self) -> i32 {
        self.vp_height.get()
    }
    pub fn set_canvases(&self, held: &[(i32, i32, i32)]) {
        let mut c = self.canvases.borrow_mut();
        c.clear();
        c.extend_from_slice(held);
    }
    pub fn take_violations(&self) -> Vec<Vec<u8>> {
        std::mem::take(&mut *self.violations.borrow_mut())
    }
    fn push_violation(&self, v: Vec<u8>) {
        self.violations.borrow_mut().push(v);
    }
    pub fn cursor(&self) -> Vec<u8> {
        self.cursor.borrow().clone()
    }
    pub fn clear_cursor(&self) {
        self.cursor.borrow_mut().clear();
    }
    pub fn set_tracing(&self, on: bool) {
        self.tracing.set(on);
    }

    /// `fonts_.size()`.
    pub fn font_count(&self) -> usize {
        self.fonts.borrow().fonts.len()
    }

    /// `font_info`: what a litehtml font handle is for the backend: its font, its descent and its spacings.
    pub fn font_info(&self, h: u64) -> Option<(i32, f32, f32, f32)> {
        let fonts = self.fonts.borrow();
        let f = fonts.fonts.get(&h)?;
        Some((f.handle, f.metrics.descent, f.letter_spacing, f.word_spacing))
    }

    /// The backend font of litehtml font `h`, 0 for none (`editor_style`'s `spec.font`).
    pub fn font_handle(&self, h: u64) -> i32 {
        self.fonts.borrow().fonts.get(&h).map_or(0, |f| f.handle)
    }

    /// The paint's list so far, taken (`std::move(list_)`).
    pub fn take_list(&self) -> DisplayList {
        std::mem::take(&mut self.paint.borrow_mut().list)
    }

    /// `list_.clear(); clips_.clear(); suspended_.clear(); groups_.clear();`.
    pub fn begin_list(&self) {
        let mut p = self.paint.borrow_mut();
        p.list.clear();
        p.clips.clear();
        p.suspended.clear();
        p.groups.clear();
    }

    /// The SVG paints kept, dropped (`cached_.clear(); cached_commands_ = 0;`).
    pub fn drop_cached(&self) {
        let mut p = self.paint.borrow_mut();
        p.cached.clear();
        p.cached_commands = 0;
    }

    /// `set_no_effects`'s container half: true when it changed.
    pub fn set_no_effects(&self) -> bool {
        if self.no_effects.get() {
            return false;
        }
        self.no_effects.set(true);
        self.drop_cached();
        true
    }

    /// What paint will read of the document's elements: each filtering element's colour (a drop-shadow's
    /// currentColor), and with the paint limit's effects off, which elements are disabled buttons.
    pub fn prepare_paint(&self, doc: &twidget_litehtml::Document) {
        let mut colors = self.owner_colors.borrow_mut();
        colors.clear();
        let mut buttons = self.disabled_buttons.borrow_mut();
        buttons.clear();
        let no_effects = self.no_effects.get();
        for (i, el) in doc.elements.iter().enumerate() {
            let id = ElementId(i as u32);
            if !el.css.m_filter.is_empty() || !el.css.m_backdrop_filter.is_empty() {
                colors.insert(id, el.css.m_color);
            }
            if no_effects
                && el.kind.is_html_tag()
                && el.tag == twidget_litehtml::string_id::_button_
                && el.attrs.contains_key(&b"disabled"[..])
            {
                buttons.insert(id);
            }
        }
    }

    /// `trim`'s container half: fonts no document holds, measured widths, the SVG paints kept.
    pub fn trim(&self) {
        let mut fonts = self.fonts.borrow_mut();
        let keys: Vec<(FaceKey, u64)> = fonts.faces.iter().map(|(k, &v)| (k.clone(), v)).collect();
        for (key, id) in keys {
            let (uses, handle) = fonts.face_data.get(&id).map_or((0, 0), |f| (f.uses, f.handle));
            if uses == 0 {
                self.backend.borrow_mut().delete_font(handle);
                fonts.erase_face(&key);
            } else {
                fonts.forget(id);
            }
        }
        drop(fonts);
        self.drop_cached();
    }

    /// `~Container`'s last step: every face back to the backend.
    pub fn delete_faces(&self) {
        let mut fonts = self.fonts.borrow_mut();
        let ids: Vec<u64> = fonts.faces.values().copied().collect();
        for id in ids {
            if let Some(f) = fonts.face_data.get(&id) {
                self.backend.borrow_mut().delete_font(f.handle);
            }
        }
        fonts.faces.clear();
        fonts.face_data.clear();
    }

    /// `delete_font(h)`: the font handle goes; its face stays for the next document, which asks for the same fonts.
    pub fn delete_font(&self, h: u64) {
        let mut fonts = self.fonts.borrow_mut();
        let Some(f) = fonts.fonts.remove(&h) else { return };
        if let Some(face) = fonts.face_data.get_mut(&f.face) {
            face.uses -= 1;
        }
    }

    /// `face_for`.
    fn face_for(&self, fonts: &mut Fonts, family: &[u8], size: f32, weight: i32, italic: bool, features: &[u8]) -> Option<u64> {
        let key = FaceKey { family: family.to_vec(), size, weight, italic, features: features.to_vec() };
        let id = match fonts.faces.get(&key) {
            Some(&id) => id,
            None => {
                // Past the bound, faces no document holds go, least recently used first.
                if fonts.faces.len() >= MAX_FACES {
                    let mut idle: Vec<(u64, FaceKey)> = Vec::new();
                    for (k, id) in &fonts.faces {
                        if let Some(f) = fonts.face_data.get(id)
                            && f.uses == 0
                        {
                            idle.push((f.used, k.clone()));
                        }
                    }
                    idle.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
                    let mut i = 0;
                    while i < idle.len() && fonts.faces.len() >= MAX_FACES / 2 {
                        if let Some(&id) = fonts.faces.get(&idle[i].1) {
                            let handle = fonts.face_data.get(&id).map_or(0, |f| f.handle);
                            self.backend.borrow_mut().delete_font(handle);
                        }
                        fonts.erase_face(&idle[i].1);
                        i += 1;
                    }
                    // A cached paint may draw text in a face gone now.
                    self.drop_cached();
                }
                let mut face = Face::default();
                face.handle = self.backend.borrow_mut().create_font(family, size, weight, italic, features, &mut face.metrics);
                if face.handle == 0 {
                    return None;
                }
                fonts.next_face += 1;
                let id = fonts.next_face;
                fonts.face_data.insert(id, face);
                fonts.faces.insert(key, id);
                id
            }
        };
        fonts.face_clock += 1;
        let clock = fonts.face_clock;
        if let Some(f) = fonts.face_data.get_mut(&id) {
            f.used = clock;
        }
        Some(id)
    }

    /// `text_width` of a litehtml font, through the width cache.
    pub fn measure(&self, text: &[u8], h: u64) -> f32 {
        let mut fonts = self.fonts.borrow_mut();
        let Some(font) = fonts.fonts.get(&h).cloned() else { return 0.0 };
        let mut backend = self.backend.borrow_mut();
        fonts.font_width(&mut *backend, &font, text)
    }

    // ── images ──

    /// `load_image(src)`: a source the document or a canvas will draw, decoded once.
    pub fn load(&self, url: &[u8]) {
        let mut images = self.images.borrow_mut();
        if images.map.contains_key(url) {
            return;
        }
        let own = url.starts_with(b"tw-canvas:");
        if !own && (images.sources >= MAX_IMAGES || url.len() > MAX_IMAGE_BYTES.wrapping_sub(images.bytes)) {
            if !images.full {
                self.push_violation(format!("image limit: {} sources, {} bytes", images.sources, images.bytes).into_bytes());
            }
            images.full = true;
            return;
        }
        let mut img = Image::default();
        let refused = |url: &[u8]| [b"image refused: ".as_slice(), &url[..url.len().min(80)]].concat();
        if own {
            // The renderer's own source for a <canvas>: tw-canvas:<node id>/<w>x<h>, drawn from the backend's bitmap.
            let canvases = self.canvases.borrow();
            let held = |id: i32, w: i32, h: i32| canvases.iter().any(|&(cid, cw, ch)| cid == id && cw == w && ch == h);
            let scanned = scan_canvas_source(c_str(&url[10..]));
            let Some((id, w, h)) = scanned.filter(|&(id, w, h)| held(id, w, h)) else {
                self.push_violation(refused(url));
                return;
            };
            // A canvas is named by the size it has now: the sources of its sizes before, and of canvases gone, are not kept.
            images.map.retain(|_, entry| entry.canvas == 0 || held(entry.canvas, entry.w as i32, entry.h as i32));
            img.canvas = id;
            img.w = w as f32;
            img.h = h as f32;
        } else {
            let mime = data_mime(url);
            if mime == b"image/svg+xml" {
                // Profile 1 has no SVG images: the validator puts its 48 px placeholder, an SVG, in place of every
                // image it refuses. No SVG reaches a backend, whatever the spelling.
                img.placeholder = true;
                img.w = 48.0;
                img.h = 48.0;
            } else if IMAGE_TYPES.contains(&&mime[..]) {
                // The backend may answer the size later (`image_ready`): the id stays, the size is 0 until then.
                images.next_image = images.next_image.wrapping_add(1);
                img.id = images.next_image;
                if !self.backend.borrow_mut().load_image(img.id, url, &mut img.w, &mut img.h) {
                    img.id = 0;
                }
            } else {
                self.push_violation(refused(url));
            }
        }
        if !own {
            images.sources += 1;
            images.bytes += url.len();
        }
        images.map.insert(url.to_vec(), img);
    }

    /// `image_of`: a decoded image for the canvas's drawImage, loaded as an <img> would.
    pub fn image_of(&self, url: &[u8]) -> Option<(i32, f32, f32)> {
        self.load(c_str(url));
        let images = self.images.borrow();
        let img = images.map.get(c_str(url))?;
        if img.id == 0 && !img.placeholder {
            return None;
        }
        Some((img.id, img.w, img.h))
    }

    /// `image_ready`: an image decoded later got its size; true when a known image changed.
    pub fn image_ready(&self, id: i32, w: f32, h: f32) -> bool {
        let mut images = self.images.borrow_mut();
        for img in images.map.values_mut() {
            if img.id == id {
                if img.w == w && img.h == h {
                    return false;
                }
                img.w = w;
                img.h = h;
                return true;
            }
        }
        false
    }

    fn push(&self, c: Command) {
        self.paint.borrow_mut().list.push(c);
    }

    fn list_len(&self) -> usize {
        self.paint.borrow().list.len()
    }

    /// widget-sandbox's placeholder (url-policy.ts, PLACEHOLDER_IMAGE), its 48 × 48 drawing fitted into the image,
    /// centred.
    fn draw_placeholder(&self, layer: &BackgroundLayer) {
        let mut to = rect_of(&layer.origin_box);
        let mut clip = rect_of(&layer.clip_box);
        clip_intersect(&mut clip, &rect_of(&layer.border_box));
        let mut sx = to.w / 48.0;
        let mut sy = to.h / 48.0;
        // The placeholder is an SVG with a view box, so it keeps its aspect ratio, centred (preserveAspectRatio
        // xMidYMid meet), as browsers and LHContainer draw it.
        let s = cmin(sx, sy);
        sx = s;
        sy = s;
        to = Rect::new(to.x + (to.w - 48.0 * sx) / 2.0, to.y + (to.h - 48.0 * sy) / 2.0, 48.0 * sx, 48.0 * sy);
        let faint = Color { r: 136, g: 136, b: 136, a: 34 };
        let mark = Color { r: 136, g: 136, b: 136, a: 102 };
        self.push(Command { kind: Cmd::ClipPush, rect: clip, ..Command::default() });
        self.push(Command { kind: Cmd::FillRect, rect: to, color: faint, ..Command::default() });
        let mut hills = Path::default();
        let points: [f32; 10] = [8.0, 36.0, 18.0, 24.0, 26.0, 33.0, 32.0, 26.0, 40.0, 36.0];
        for i in (0..points.len()).step_by(2) {
            hills.ops.push(if i != 0 { PathOp::Line } else { PathOp::Move });
            hills.points.push(to.x + points[i] * sx);
            hills.points.push(to.y + points[i + 1] * sy);
        }
        hills.ops.push(PathOp::Close);
        hills.fill = true;
        hills.fill_color = mark;
        self.push(Command { kind: Cmd::Path, path: Some(Arc::new(hills)), ..Command::default() });
        self.push(Command {
            kind: Cmd::FillEllipse,
            rect: Rect::new(to.x + 29.0 * sx, to.y + 11.0 * sy, 8.0 * sx, 8.0 * sy),
            color: mark,
            ..Command::default()
        });
        self.push(Command { kind: Cmd::ClipPop, ..Command::default() });
    }

    fn gradient_common(c: &mut Command, layer: &BackgroundLayer, g: &GradientBase) {
        c.rect = rect_of(&layer.border_box);
        c.radii = radii_of(&layer.border_radius);
        c.clip = rect_of(&layer.clip_box);
        c.has_clip = true;
        for p in &g.color_points {
            c.stops.push(Stop::new(p.offset, color_of(&p.color)));
        }
    }

    /// The fork's filter functions for the display list; drop-shadow's currentColor is the element's colour.
    fn filters_of(&self, f: &FilterPaint) -> Option<Arc<Filters>> {
        if f.steps.is_empty() {
            return None;
        }
        let mut out = Filters::new();
        let owner_color = f.owner.and_then(|o| self.owner_colors.borrow().get(&o).copied());
        for step in f.steps {
            let kind = match step.kind {
                FilterKind::blur => twidget_wire::FilterKind::Blur,
                FilterKind::brightness => twidget_wire::FilterKind::Brightness,
                FilterKind::contrast => twidget_wire::FilterKind::Contrast,
                FilterKind::grayscale => twidget_wire::FilterKind::Grayscale,
                FilterKind::hue_rotate => twidget_wire::FilterKind::HueRotate,
                FilterKind::invert => twidget_wire::FilterKind::Invert,
                FilterKind::opacity => twidget_wire::FilterKind::Opacity,
                FilterKind::saturate => twidget_wire::FilterKind::Saturate,
                FilterKind::sepia => twidget_wire::FilterKind::Sepia,
                FilterKind::drop_shadow => twidget_wire::FilterKind::DropShadow,
            };
            let mut filter = Filter { kind, ..Filter::default() };
            filter.amount =
                if step.kind == FilterKind::blur || step.kind == FilterKind::drop_shadow { step.radius.val() } else { step.amount };
            if step.kind == FilterKind::drop_shadow {
                filter.x = step.x.val();
                filter.y = step.y.val();
                filter.color = color_of(&match owner_color {
                    Some(c) if step.color.is_current_color => c,
                    _ => step.color,
                });
            }
            out.push(filter);
        }
        Some(Arc::new(out))
    }

    // ── borders ──

    /// `draw_borders`: one stroke when uniform, otherwise each side on its own; dashed, dotted and double by their style.
    fn borders(&self, b: &Borders, draw_pos: &Position) {
        let visible = |s: &twidget_litehtml::borders::Border| {
            s.width.value() > 0.0
                && s.style != BorderStyle::border_style_none.index()
                && s.style != BorderStyle::border_style_hidden.index()
                && s.color.alpha != 0
        };
        let (l, t, rr, bt) = (visible(&b.left), visible(&b.top), visible(&b.right), visible(&b.bottom));
        if !l && !t && !rr && !bt {
            return;
        }
        let same = |a: &twidget_litehtml::borders::Border, c: &twidget_litehtml::borders::Border| {
            a.width == c.width && a.color == c.color && a.style == c.style
        };
        let broken = |s: &twidget_litehtml::borders::Border| {
            s.style == BorderStyle::border_style_dashed.index() || s.style == BorderStyle::border_style_dotted.index()
        };
        let corners = radii_of(&b.radius);
        let square = corners.tlx == 0.0
            && corners.tly == 0.0
            && corners.trx == 0.0
            && corners.try_ == 0.0
            && corners.brx == 0.0
            && corners.bry == 0.0
            && corners.blx == 0.0
            && corners.bly == 0.0;
        // One style all round, rounded dashed or dotted, or double: as LHContainer draws it.
        let all = l && t && rr && bt && same(&b.left, &b.top) && same(&b.left, &b.right) && same(&b.left, &b.bottom);
        if all
            && ((!square && broken(&b.left)) || (b.left.style == BorderStyle::border_style_double.index() && b.left.width.value() >= 3.0))
        {
            self.draw_uniform_styled(&b.left, &rect_of(draw_pos), &corners);
            return;
        }
        if square
            && ((l && broken(&b.left))
                || (t && broken(&b.top))
                || (rr && broken(&b.right))
                || (bt && broken(&b.bottom))
                || !(same(&b.left, &b.top) && same(&b.left, &b.right) && same(&b.left, &b.bottom)))
        {
            self.draw_sides(b, &rect_of(draw_pos), [l, t, rr, bt]);
            return;
        }
        let uniform = l && t && rr && bt && same(&b.left, &b.top) && same(&b.left, &b.right) && same(&b.left, &b.bottom);
        if !square && !uniform {
            self.draw_rounded_sides(b, &rect_of(draw_pos), &corners, [l, t, rr, bt]);
            return;
        }
        if uniform {
            let w = b.left.width.value();
            let mut r = radii_of(&b.radius);
            let shrink = |v: &mut f32| *v = cmax(0.0, *v - w / 2.0);
            shrink(&mut r.tlx);
            shrink(&mut r.tly);
            shrink(&mut r.trx);
            shrink(&mut r.try_);
            shrink(&mut r.brx);
            shrink(&mut r.bry);
            shrink(&mut r.blx);
            shrink(&mut r.bly);
            self.push(Command {
                kind: Cmd::StrokeRect,
                width: w,
                color: color_of(&b.left.color),
                rect: Rect::new(
                    draw_pos.x.value() + w / 2.0,
                    draw_pos.y.value() + w / 2.0,
                    draw_pos.width.value() - w,
                    draw_pos.height.value() - w,
                ),
                radii: r,
                ..Command::default()
            });
            return;
        }
        let p = rect_of(draw_pos);
        let strip = |s: &twidget_litehtml::borders::Border, rect: Rect| {
            self.push(Command { kind: Cmd::FillRect, color: color_of(&s.color), rect, ..Command::default() });
        };
        if t {
            strip(&b.top, Rect::new(p.x, p.y, p.w, b.top.width.value()));
        }
        if bt {
            strip(&b.bottom, Rect::new(p.x, p.y + p.h - b.bottom.width.value(), p.w, b.bottom.width.value()));
        }
        if l {
            strip(&b.left, Rect::new(p.x, p.y, b.left.width.value(), p.h));
        }
        if rr {
            strip(&b.right, Rect::new(p.x + p.w - b.right.width.value(), p.y, b.right.width.value(), p.h));
        }
    }

    /// A dashed or dotted rounded border (`dashed_ring`): its centre line `mid` (corners `mrad`) stroked with the dashes
    /// fitted round it, under a clip to the ring between the border's edges.
    #[allow(clippy::too_many_arguments)]
    fn dashed_ring(
        &self,
        outer_rect: &Rect,
        outer: &Radii,
        inner_rect: &Rect,
        inner: &Radii,
        mid: &Rect,
        mrad: &Radii,
        width: f32,
        reach: f32,
        dotted: bool,
        color: Color,
    ) {
        let d = border_dash(rounded_length(mid, mrad).floor(), width, dotted, true);
        let mut ring = Path::default();
        rounded_rect(&mut ring, outer_rect, outer);
        rounded_rect(&mut ring, inner_rect, inner);
        ring.fill = true;
        ring.evenodd = true;
        let clip = Command { kind: Cmd::ClipPush, rect: *outer_rect, path: Some(Arc::new(ring)), ..Command::default() };
        {
            let mut p = self.paint.borrow_mut();
            p.clips.push(clip.clone());
            p.list.push(clip);
        }
        let mut line = Path::default();
        rounded_rect(&mut line, mid, mrad);
        line.stroke = true;
        line.stroke_color = color;
        line.stroke_width = if d.round { width } else { reach * 2.2 };
        apply_dash(&mut line, &d);
        let mut p = self.paint.borrow_mut();
        p.list.push(Command { kind: Cmd::Path, path: Some(Arc::new(line)), ..Command::default() });
        p.clips.pop();
        p.list.push(Command { kind: Cmd::ClipPop, ..Command::default() });
    }

    /// Rounded borders whose sides differ (`draw_rounded_sides`): each side fills the ring between the outer and the
    /// inner rounded box, clipped to the part of the box it owns.
    fn draw_rounded_sides(&self, b: &Borders, p: &Rect, outer: &Radii, visible: [bool; 4]) {
        let sides = [&b.left, &b.top, &b.right, &b.bottom];
        let (wl, wt, wr, wb) = (b.left.width.value(), b.top.width.value(), b.right.width.value(), b.bottom.width.value());
        let inn = Rect::new(p.x + wl, p.y + wt, cmax(0.0, p.w - wl - wr), cmax(0.0, p.h - wt - wb));
        let inner = Radii {
            tlx: cmax(0.0, outer.tlx - wl),
            tly: cmax(0.0, outer.tly - wt),
            trx: cmax(0.0, outer.trx - wr),
            try_: cmax(0.0, outer.try_ - wt),
            brx: cmax(0.0, outer.brx - wr),
            bry: cmax(0.0, outer.bry - wb),
            blx: cmax(0.0, outer.blx - wl),
            bly: cmax(0.0, outer.bly - wb),
        };
        let (x0, y0, x1, y1, cx, cy) = (p.x, p.y, p.x + p.w, p.y + p.h, p.x + p.w / 2.0, p.y + p.h / 2.0);
        // Corners clockwise from the top left: the point, then the way to its inner corner.
        let corner: [[f32; 4]; 4] = [[x0, y0, wl, wt], [x1, y0, -wr, wt], [x1, y1, -wr, -wb], [x0, y1, wl, -wb]];
        for i in 0..4 {
            if !visible[i] {
                continue;
            }
            // Left runs from the bottom left corner to the top left one, top from there to the top right, and so on.
            let a = &corner[(i + 3) % 4];
            let c = &corner[i];
            let vertical = i == 0 || i == 2;
            let reach = |k: &[f32; 4]| if vertical { (cx - k[0]) / k[2] } else { (cy - k[1]) / k[3] };
            let ta = reach(a);
            let tc = reach(c);
            let mut points: Vec<f32> = vec![a[0], a[1], c[0], c[1]];
            // Where the two lines meet before the midline, as on a short side of a long box.
            let rx = c[0] - a[0];
            let ry = c[1] - a[1];
            let det = c[2] * a[3] - a[2] * c[3];
            let s = if det != 0.0 { (c[2] * ry - c[3] * rx) / det } else { -1.0 };
            let u = if det != 0.0 { (a[2] * ry - a[3] * rx) / det } else { -1.0 };
            if s > 0.0 && s < ta && u > 0.0 && u < tc {
                points.extend_from_slice(&[a[0] + s * a[2], a[1] + s * a[3]]);
            } else {
                points.extend_from_slice(&[c[0] + tc * c[2], c[1] + tc * c[3], a[0] + ta * a[2], a[1] + ta * a[3]]);
            }
            let mut share = Path::default();
            let (mut minx, mut miny, mut maxx, mut maxy) = (points[0], points[1], points[0], points[1]);
            for k in (0..points.len()).step_by(2) {
                share.ops.push(if k != 0 { PathOp::Line } else { PathOp::Move });
                share.points.extend_from_slice(&[points[k], points[k + 1]]);
                minx = cmin(minx, points[k]);
                maxx = cmax(maxx, points[k]);
                miny = cmin(miny, points[k + 1]);
                maxy = cmax(maxy, points[k + 1]);
            }
            share.ops.push(PathOp::Close);
            let clip = Command {
                kind: Cmd::ClipPush,
                rect: Rect::new(minx, miny, maxx - minx, maxy - miny),
                path: Some(Arc::new(share)),
                ..Command::default()
            };
            {
                let mut pt = self.paint.borrow_mut();
                pt.clips.push(clip.clone());
                pt.list.push(clip);
            }
            let style = sides[i].style;
            if style == BorderStyle::border_style_dashed.index() || style == BorderStyle::border_style_dotted.index() {
                // The whole centre line, dashed round as one, of which the share keeps this side's part.
                let mid = Rect::new(x0 + wl / 2.0, y0 + wt / 2.0, p.w - (wl + wr) / 2.0, p.h - (wt + wb) / 2.0);
                let mrad = Radii {
                    tlx: cmax(0.0, outer.tlx - wl / 2.0),
                    tly: cmax(0.0, outer.tly - wt / 2.0),
                    trx: cmax(0.0, outer.trx - wr / 2.0),
                    try_: cmax(0.0, outer.try_ - wt / 2.0),
                    brx: cmax(0.0, outer.brx - wr / 2.0),
                    bry: cmax(0.0, outer.bry - wb / 2.0),
                    blx: cmax(0.0, outer.blx - wl / 2.0),
                    bly: cmax(0.0, outer.bly - wb / 2.0),
                };
                let w = sides[i].width.value();
                let reach = if vertical { cmax(cmax(w, wt), wb) } else { cmax(cmax(w, wl), wr) };
                self.dashed_ring(
                    p,
                    outer,
                    &inn,
                    &inner,
                    &mid,
                    &mrad,
                    w,
                    reach,
                    style == BorderStyle::border_style_dotted.index(),
                    color_of(&sides[i].color),
                );
            } else {
                let mut ring = Path::default();
                rounded_rect(&mut ring, p, outer);
                rounded_rect(&mut ring, &inn, &inner);
                ring.fill = true;
                ring.evenodd = true;
                ring.fill_color = color_of(&sides[i].color);
                self.push(Command { kind: Cmd::Path, path: Some(Arc::new(ring)), ..Command::default() });
            }
            let mut pt = self.paint.borrow_mut();
            pt.clips.pop();
            pt.list.push(Command { kind: Cmd::ClipPop, ..Command::default() });
        }
    }

    /// A uniform border's style that the solid paths cannot draw, as LHContainer draws it (`draw_uniform_styled`).
    fn draw_uniform_styled(&self, s: &twidget_litehtml::borders::Border, p: &Rect, outer: &Radii) {
        let w = s.width.value();
        let inset = |d: f32, r: &mut Rect, k: &mut Radii| {
            *r = Rect::new(p.x + d, p.y + d, cmax(0.0, p.w - 2.0 * d), cmax(0.0, p.h - 2.0 * d));
            *k = Radii {
                tlx: cmax(0.0, outer.tlx - d),
                tly: cmax(0.0, outer.tly - d),
                trx: cmax(0.0, outer.trx - d),
                try_: cmax(0.0, outer.try_ - d),
                brx: cmax(0.0, outer.brx - d),
                bry: cmax(0.0, outer.bry - d),
                blx: cmax(0.0, outer.blx - d),
                bly: cmax(0.0, outer.bly - d),
            };
            r.w > 0.0 && r.h > 0.0
        };
        let mut r = Rect::default();
        let mut k = Radii::default();
        if s.style == BorderStyle::border_style_double.index() {
            let mut rings = Path::default();
            rounded_rect(&mut rings, p, outer);
            for d in [w / 3.0, 2.0 * w / 3.0, w] {
                if inset(d, &mut r, &mut k) {
                    rounded_rect(&mut rings, &r, &k);
                }
            }
            rings.fill = true;
            rings.evenodd = true;
            rings.fill_color = color_of(&s.color);
            self.push(Command { kind: Cmd::Path, path: Some(Arc::new(rings)), ..Command::default() });
            return;
        }
        let mut mid = Rect::default();
        let mut mrad = Radii::default();
        inset(w, &mut r, &mut k);
        inset(w / 2.0, &mut mid, &mut mrad);
        self.dashed_ring(p, outer, &r, &k, &mid, &mrad, w, w, s.style == BorderStyle::border_style_dotted.index(), color_of(&s.color));
    }

    /// Square borders side by side (`draw_sides`): each side is the trapezoid its corners mitre, filled when solid, and
    /// stroked along its middle under a clip to it when dashed or dotted.
    fn draw_sides(&self, b: &Borders, p: &Rect, visible: [bool; 4]) {
        let sides = [&b.left, &b.top, &b.right, &b.bottom];
        let (wl, wt, wr, wb) = (b.left.width.value(), b.top.width.value(), b.right.width.value(), b.bottom.width.value());
        let (x0, y0, x1, y1) = (p.x, p.y, p.x + p.w, p.y + p.h);
        // Outer corners, then inner ones, clockwise from the top left.
        let quads: [[f32; 8]; 4] = [
            [x0, y1, x0, y0, x0 + wl, y0 + wt, x0 + wl, y1 - wb],
            [x0, y0, x1, y0, x1 - wr, y0 + wt, x0 + wl, y0 + wt],
            [x1, y0, x1, y1, x1 - wr, y1 - wb, x1 - wr, y0 + wt],
            [x1, y1, x0, y1, x0 + wl, y1 - wb, x1 - wr, y1 - wb],
        ];
        for i in 0..4 {
            if !visible[i] {
                continue;
            }
            let s = sides[i];
            let mut quad = Path::default();
            for k in (0..8).step_by(2) {
                quad.ops.push(if k != 0 { PathOp::Line } else { PathOp::Move });
                quad.points.push(quads[i][k]);
                quad.points.push(quads[i][k + 1]);
            }
            quad.ops.push(PathOp::Close);
            let dashed = s.style == BorderStyle::border_style_dashed.index();
            let dotted = s.style == BorderStyle::border_style_dotted.index();
            if !dashed && !dotted {
                quad.fill = true;
                quad.fill_color = color_of(&s.color);
                self.push(Command { kind: Cmd::Path, path: Some(Arc::new(quad)), ..Command::default() });
                continue;
            }
            let w = s.width.value();
            let horizontal = i == 1 || i == 3;
            // The middle of the side, corner to corner, where the dashes are fitted whole.
            let mut ax = if horizontal {
                x0
            } else if i == 0 {
                x0 + w / 2.0
            } else {
                x1 - w / 2.0
            };
            let mut ay = if horizontal { if i == 1 { y0 + w / 2.0 } else { y1 - w / 2.0 } } else { y0 };
            let mut bx = if horizontal { x1 } else { ax };
            let mut by = if horizontal { ay } else { y1 };
            let length = if horizontal { p.w } else { p.h };
            let mut line = Path { stroke: true, stroke_color: color_of(&s.color), stroke_width: w, ..Path::default() };
            let d = border_dash(length.round(), w, dotted, false);
            apply_dash(&mut line, &d);
            if d.round {
                // A round dot sits inside each end, its cap reaching the corner.
                ax += if horizontal { w / 2.0 } else { 0.0 };
                ay += if horizontal { 0.0 } else { w / 2.0 };
                bx -= if horizontal { w / 2.0 } else { 0.0 };
                by -= if horizontal { 0.0 } else { w / 2.0 };
            }
            line.ops = vec![PathOp::Move, PathOp::Line];
            line.points = vec![ax, ay, bx, by];
            let q = &quads[i];
            let min4 = |a: f32, b: f32, c: f32, d: f32| cmin(cmin(cmin(a, b), c), d);
            let max4 = |a: f32, b: f32, c: f32, d: f32| cmax(cmax(cmax(a, b), c), d);
            let mut rect = Rect::new(min4(q[0], q[2], q[4], q[6]), min4(q[1], q[3], q[5], q[7]), 0.0, 0.0);
            rect.w = max4(q[0], q[2], q[4], q[6]) - rect.x;
            rect.h = max4(q[1], q[3], q[5], q[7]) - rect.y;
            let clip = Command { kind: Cmd::ClipPush, rect, path: Some(Arc::new(quad)), ..Command::default() };
            let mut pt = self.paint.borrow_mut();
            pt.clips.push(clip.clone());
            pt.list.push(clip);
            pt.list.push(Command { kind: Cmd::Path, path: Some(Arc::new(line)), ..Command::default() });
            pt.clips.pop();
            pt.list.push(Command { kind: Cmd::ClipPop, ..Command::default() });
        }
    }

    /// Paints `f` into the list (the trace's and the draw callbacks' common start): None while tracing.
    fn painting(&self) -> bool {
        !self.tracing.get()
    }
}

pub(crate) fn clip_intersect(a: &mut Rect, b: &Rect) {
    let x0 = cmax(a.x, b.x);
    let y0 = cmax(a.y, b.y);
    let x1 = cmin(a.x + a.w, b.x + b.w);
    let y1 = cmin(a.y + a.h, b.y + b.h);
    *a = Rect::new(x0, y0, cmax(0.0, x1 - x0), cmax(0.0, y1 - y0));
}

/// Appends `r` with corners `k` to `path` as one closed figure (`rounded_rect`).
fn rounded_rect(path: &mut Path, r: &Rect, k: &Radii) {
    const K: f32 = 0.5523; // a quarter circle as one cubic
    let (x0, y0, x1, y1) = (r.x, r.y, r.x + r.w, r.y + r.h);
    let line = |path: &mut Path, x: f32, y: f32| {
        path.ops.push(PathOp::Line);
        path.points.extend_from_slice(&[x, y]);
    };
    let curve = |path: &mut Path, ax: f32, ay: f32, bx: f32, by: f32, x: f32, y: f32| {
        path.ops.push(PathOp::Cubic);
        path.points.extend_from_slice(&[ax, ay, bx, by, x, y]);
    };
    path.ops.push(PathOp::Move);
    path.points.extend_from_slice(&[x0 + k.tlx, y0]);
    line(path, x1 - k.trx, y0);
    curve(path, x1 - k.trx + k.trx * K, y0, x1, y0 + k.try_ - k.try_ * K, x1, y0 + k.try_);
    line(path, x1, y1 - k.bry);
    curve(path, x1, y1 - k.bry + k.bry * K, x1 - k.brx + k.brx * K, y1, x1 - k.brx, y1);
    line(path, x0 + k.blx, y1);
    curve(path, x0 + k.blx - k.blx * K, y1, x0, y1 - k.bly + k.bly * K, x0, y1 - k.bly);
    line(path, x0, y0 + k.tly);
    curve(path, x0, y0 + k.tly - k.tly * K, x0 + k.tlx - k.tlx * K, y0, x0 + k.tlx, y0);
    path.ops.push(PathOp::Close);
}

/// How Chromium breaks a dashed or dotted border (`BorderDash`).
#[derive(Clone, Copy, Debug)]
struct BorderDash {
    solid: bool,
    round: bool,
    dash: f32,
    gap: f32,
}

/// `border_dash`.
fn border_dash(length: f32, width: f32, dotted: bool, closed: bool) -> BorderDash {
    let mut out = BorderDash { solid: true, round: false, dash: 0.0, gap: 0.0 };
    if width <= 0.0 || length <= 0.0 {
        return out;
    }
    if !dotted || width <= 3.0 {
        let mut dash = width;
        let mut gap = width;
        if !dotted {
            dash *= if width >= 3.0 { 2.0 } else { 3.0 };
            gap *= if width >= 3.0 { 1.0 } else { 2.0 };
        }
        if length <= dash * 2.0 {
            return out;
        }
        let pair = if closed { dash + gap } else { 2.0 * dash + gap };
        out.solid = false;
        if length <= pair {
            out.dash = dash * length / pair;
            out.gap = gap * length / pair;
        } else {
            out.dash = dash;
            out.gap = if dotted { gap } else { best_dash_gap(length, dash, gap, closed) };
        }
        return out;
    }
    out.solid = false;
    out.round = true;
    if length < 2.0 * width {
        out.gap = 2.0 * width;
        return out;
    }
    // Round caps draw each zero-length dash as a dot a width across.
    out.gap = best_dash_gap(length, width, width, closed) + width - 0.01;
    if !out.gap.is_finite() {
        out.gap = length + 2.0 * width;
    }
    out
}

fn apply_dash(path: &mut Path, d: &BorderDash) {
    path.line_cap = if d.round { 1 } else { 0 };
    if !d.solid {
        path.dashes = vec![d.dash, d.gap];
    }
}

/// The length of a rounded box's outline, its corners as quarter ellipses (Ramanujan): `rounded_length`.
fn rounded_length(r: &Rect, k: &Radii) -> f32 {
    if r.w <= 0.0 || r.h <= 0.0 {
        return 0.0;
    }
    let cx = |v: f32| cmin(cmax(v, 0.0), r.w / 2.0);
    let cy = |v: f32| cmin(cmax(v, 0.0), r.h / 2.0);
    let corner = |a: f32, b: f32| {
        if a <= 0.0 || b <= 0.0 {
            return a.hypot(b);
        }
        std::f32::consts::FRAC_PI_4 * (3.0 * (a + b) - ((3.0 * a + b) * (a + 3.0 * b)).sqrt())
    };
    let xs = [cx(k.tlx), cx(k.trx), cx(k.brx), cx(k.blx)];
    let ys = [cy(k.tly), cy(k.try_), cy(k.bry), cy(k.bly)];
    let mut length = 2.0 * (r.w + r.h);
    for i in 0..4 {
        length += corner(xs[i], ys[i]) - xs[i] - ys[i];
    }
    length
}

/// The gap that fits whole dashes into `length` nearest to `gap` (LHContainer's BestDashGap): `best_dash_gap`.
fn best_dash_gap(length: f32, dash: f32, gap: f32, closed: bool) -> f32 {
    let fewer = ((if closed { length } else { length + gap }) / (dash + gap)).floor();
    let more = fewer + 1.0;
    let fewer_gap = (length - fewer * dash) / (if closed { fewer } else { fewer - 1.0 });
    let more_gap = (length - more * dash) / (if closed { more } else { more - 1.0 });
    if more_gap <= 0.0 || (fewer_gap - gap).abs() < (more_gap - gap).abs() { fewer_gap } else { more_gap }
}

/// `marker_text`: a list marker's text; out of a style's range the number in decimal.
fn marker_text(t: i32, index: i32) -> Vec<u8> {
    let alpha = |n: i32, base: u8| -> Vec<u8> {
        if n < 1 {
            return n.to_string().into_bytes();
        }
        let mut s = Vec::new();
        let mut n = n;
        while n > 0 {
            n -= 1;
            s.insert(0, base + (n % 26) as u8);
            n /= 26;
        }
        s
    };
    let roman = |n: i32, upper: bool| -> Vec<u8> {
        if !(1..=3999).contains(&n) {
            return n.to_string().into_bytes();
        }
        const U: [&str; 13] = ["M", "CM", "D", "CD", "C", "XC", "L", "XL", "X", "IX", "V", "IV", "I"];
        const L: [&str; 13] = ["m", "cm", "d", "cd", "c", "xc", "l", "xl", "x", "ix", "v", "iv", "i"];
        const V: [i32; 13] = [1000, 900, 500, 400, 100, 90, 50, 40, 10, 9, 5, 4, 1];
        let mut s = Vec::new();
        let mut n = n;
        for i in 0..13 {
            while n >= V[i] {
                s.extend_from_slice(if upper { U[i] } else { L[i] }.as_bytes());
                n -= V[i];
            }
        }
        s
    };
    let mut out = if t == ListStyleType::list_style_type_decimal_leading_zero.index() {
        let mut s = if index < 10 { b"0".to_vec() } else { Vec::new() };
        s.extend_from_slice(index.to_string().as_bytes());
        s
    } else if t == ListStyleType::list_style_type_lower_alpha.index() || t == ListStyleType::list_style_type_lower_latin.index() {
        alpha(index, b'a')
    } else if t == ListStyleType::list_style_type_upper_alpha.index() || t == ListStyleType::list_style_type_upper_latin.index() {
        alpha(index, b'A')
    } else if t == ListStyleType::list_style_type_lower_roman.index() {
        roman(index, false)
    } else if t == ListStyleType::list_style_type_upper_roman.index() {
        roman(index, true)
    } else {
        index.to_string().into_bytes()
    };
    out.push(b'.');
    out
}

fn path_of(path: &VectorPath) -> Path {
    let mut p = Path::default();
    p.ops.reserve(path.ops.len());
    for op in &path.ops {
        p.ops.push(match op {
            twidget_litehtml::css_effects::PathOp::r#move => PathOp::Move,
            twidget_litehtml::css_effects::PathOp::line => PathOp::Line,
            twidget_litehtml::css_effects::PathOp::quad => PathOp::Quad,
            twidget_litehtml::css_effects::PathOp::cubic => PathOp::Cubic,
            twidget_litehtml::css_effects::PathOp::close => PathOp::Close,
        });
    }
    p.points = path.points.clone();
    p
}

impl<K: Backend> ResolveColor for Container<K> {
    fn resolve_color(&self, _name: &[u8]) -> Vec<u8> {
        Vec::new()
    }
}

impl<K: Backend> DocumentContainer for Container<K> {
    fn pt_to_px(&self, pt: f32) -> Px {
        Px(pt * 96.0 / 72.0)
    }

    fn get_default_font_size(&self) -> Px {
        Px(16.0)
    }

    fn import_css(&self, _url: &[u8], _baseurl: &mut Vec<u8>) -> Vec<u8> {
        // @import is dropped by the validator anyway.
        Vec::new()
    }

    fn get_media_features(&self) -> MediaFeatures {
        let width = Px(self.width.get() as f32);
        let height = Px(self.vp_height.get() as f32);
        MediaFeatures {
            ty: MediaType::media_type_screen.index(),
            width,
            height,
            device_width: width,
            device_height: height,
            color: 8,
            color_index: 256,
            monochrome: 0,
            resolution: Px((96.0 * self.dpr.get()) as f32),
        }
    }

    fn sanitize_declaration(&self, name: &[u8], value: &[CssToken]) -> bool {
        if self.trusted_styles.get() || value.len() != 1 || (name != b"position" && name != b"display") {
            return true;
        }
        let keyword = value[0].ident();
        if name == b"position" { keyword != b"sticky" } else { keyword != b"list-item" && keyword != b"inline-text" }
    }

    fn create_font(&self, descr: &FontDescription) -> (u64, LhMetrics) {
        let mut fonts = self.fonts.borrow_mut();
        let Some(face_id) = self.face_for(
            &mut fonts,
            &descr.family,
            descr.size.value(),
            descr.weight,
            descr.style == FontStyle::font_style_italic.index(),
            &descr.features,
        ) else {
            return (0, LhMetrics::default());
        };
        let face = fonts.face_data.get_mut(&face_id).map(|f| {
            f.uses += 1;
            f.metrics
        });
        let m = face.unwrap_or_default();
        let f = Font {
            face: face_id,
            handle: fonts.face_data.get(&face_id).map_or(0, |f| f.handle),
            metrics: m,
            size: descr.size.value(),
            decoration: descr.decoration_line,
            decoration_color: descr.decoration_color,
            letter_spacing: descr.letter_spacing,
            word_spacing: descr.word_spacing,
        };
        let fm = LhMetrics {
            font_size: descr.size,
            ascent: Px(m.ascent),
            descent: Px(m.descent),
            height: Px(cmax(m.height, m.ascent + m.descent)),
            x_height: Px(if m.x_height > 0.0 { m.x_height } else { m.ascent * 0.5 }),
            ch_width: Px(m.zero_width),
            draw_spaces: descr.decoration_line != TextDecorationLine::text_decoration_line_none.index(),
            sub_shift: Px(f.size / 5.0),
            super_shift: Px(f.size / 3.0),
        };
        fonts.next_font += 1;
        let id = fonts.next_font;
        fonts.fonts.insert(id, f);
        (id, fm)
    }

    fn text_width(&self, text: &[u8], font: u64) -> Px {
        Px(self.measure(c_str(text), font))
    }

    fn get_default_font_name(&self) -> Vec<u8> {
        b"-apple-system, system-ui, sans-serif".to_vec()
    }

    fn load_image(&self, src: &[u8], _baseurl: Option<&[u8]>, _redraw_on_ready: bool) {
        self.load(c_str(src));
    }

    fn transform_text(&self, text: &mut Vec<u8>, tt: i32) {
        // ASCII only; the platforms' Unicode case mapping would need a backend call.
        if tt == TextTransform::text_transform_uppercase.index() {
            text.iter_mut().for_each(|c| *c = c.to_ascii_uppercase());
        } else if tt == TextTransform::text_transform_lowercase.index() {
            text.iter_mut().for_each(|c| *c = c.to_ascii_lowercase());
        } else if tt == TextTransform::text_transform_capitalize.index() {
            let mut start = true;
            for c in text.iter_mut() {
                if start {
                    *c = c.to_ascii_uppercase();
                }
                start = *c == b' ';
            }
        }
    }

    fn get_language(&self) -> (Vec<u8>, Vec<u8>) {
        (b"en".to_vec(), Vec::new())
    }

    fn trusted_styles(&self, trusted: bool) {
        self.trusted_styles.set(trusted);
    }

    fn get_viewport(&self) -> Position {
        Position { x: Px(0.0), y: Px(0.0), width: Px(self.width.get() as f32), height: Px(self.vp_height.get() as f32) }
    }

    fn get_image_size(&self, src: &[u8], _baseurl: Option<&[u8]>) -> Size {
        let images = self.images.borrow();
        match images.map.get(c_str(src)) {
            Some(img) => Size { width: Px(img.w), height: Px(img.h) },
            None => Size { width: Px(0.0), height: Px(0.0) },
        }
    }

    fn create_text_breaker(&self, text: &[u8], font: u64) -> u64 {
        if !self.fonts.borrow().fonts.contains_key(&font) {
            return 0;
        }
        let id = self.next_breaker.get() + 1;
        self.next_breaker.set(id);
        self.breakers.borrow_mut().insert(id, Breaker::new(text, font));
        id
    }

    fn text_breaker_fit(&self, breaker: u64, start: usize, width: Px) -> usize {
        let Some(b) = self.breakers.borrow().get(&breaker).cloned() else { return start };
        let measure = |from: usize, to: usize| self.measure(c_str(&b.text[from..to]), b.font);
        let bounds = &b.boundaries;
        let first = bounds.partition_point(|&v| v < start);
        if first == bounds.len() || first + 1 == bounds.len() {
            return start;
        }
        // At least one character goes on the line, however narrow it is.
        let mut lo = first + 1;
        let mut hi = bounds.len() - 1;
        let width = width.value();
        if width <= 0.0 || measure(start, bounds[lo]) > width {
            return bounds[lo];
        }
        // The end of the line is near its start: halving the whole rest of the text measures it for every line.
        let mut step: usize = 1;
        while lo < hi {
            let probe = hi.min(lo + step);
            if measure(start, bounds[probe]) > width {
                hi = probe - 1;
                break;
            }
            lo = probe;
            step = step.wrapping_mul(2);
        }
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            if measure(start, bounds[mid]) <= width {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        bounds[lo]
    }

    fn text_breaker_minimum_width(&self, breaker: u64) -> Px {
        let Some(b) = self.breakers.borrow().get(&breaker).cloned() else { return Px(0.0) };
        if b.minimum < 0.0 {
            let mut minimum = 0.0f32;
            for i in 1..b.boundaries.len() {
                minimum = cmax(minimum, self.measure(c_str(&b.text[b.boundaries[i - 1]..b.boundaries[i]]), b.font));
            }
            if let Some(kept) = self.breakers.borrow_mut().get_mut(&breaker) {
                kept.minimum = minimum;
            }
            return Px(minimum);
        }
        Px(b.minimum)
    }

    fn drop_text_breaker(&self, breaker: u64) {
        self.breakers.borrow_mut().remove(&breaker);
    }

    fn draw_text(&self, _hdc: u64, text: &[u8], font: u64, color: WebColor, pos: &Position) {
        if !self.painting() {
            return;
        }
        let text = c_str(text);
        let Some(f) = self.fonts.borrow().fonts.get(&font).cloned() else { return };
        if text.is_empty() {
            return;
        }
        let width_of = |t: &[u8]| {
            let mut fonts = self.fonts.borrow_mut();
            let mut backend = self.backend.borrow_mut();
            fonts.font_width(&mut *backend, &f, t)
        };
        let x = pos.x.value();
        // litehtml hands the line box; the baseline sits `descent` above its bottom (as the cairo container does).
        let y = pos.y.value() + pos.height.value() - f.metrics.descent;
        // The line box, for damage: where the run can paint.
        let rw = if pos.width.value() > 0.0 { pos.width.value() } else { width_of(text) };
        let c = Command {
            kind: Cmd::Text,
            font: f.handle,
            text: text.to_vec(),
            color: color_of(&color),
            x,
            y,
            rect: Rect::new(pos.x.value(), pos.y.value(), rw, pos.height.value()),
            ..Command::default()
        };
        if f.letter_spacing != 0.0 || (f.word_spacing != 0.0 && spacing_spaces(text) != 0) {
            // A painter draws a run as the font spaces it: each character goes on its own, or each word when only
            // the spaces widen, so that words keep their shaping.
            let mut x = c.x;
            let units = if f.letter_spacing != 0.0 { spacing_units(text) } else { spacing_words(text) };
            for unit in units {
                let mut u = c.clone();
                u.text = unit.to_vec();
                u.x = x;
                u.rect.x = x;
                u.rect.w = width_of(unit);
                x += u.rect.w;
                if !u.text.is_empty() && u.text != b" " && u.text != b"\xC2\xA0" {
                    self.push(u);
                }
            }
        } else {
            self.push(c);
        }
        if f.decoration != TextDecorationLine::text_decoration_line_none.index() {
            let mut w = pos.width.value();
            if w <= 0.0 {
                w = self.backend.borrow_mut().text_width(f.handle, text);
            }
            let thickness = cmax(1.0, f.size / 14.0);
            let dc = if f.decoration_color.is_current_color { color_of(&color) } else { color_of(&f.decoration_color) };
            let baseline = pos.y.value() + pos.height.value() - f.metrics.descent;
            let line = |y: f32| {
                self.push(Command { kind: Cmd::FillRect, rect: Rect::new(pos.x.value(), y, w, thickness), color: dc, ..Command::default() })
            };
            if f.decoration & TextDecorationLine::text_decoration_line_underline.index() != 0 {
                line(baseline + cmax(1.0, f.metrics.descent * 0.35));
            }
            if f.decoration & TextDecorationLine::text_decoration_line_line_through.index() != 0 {
                line(baseline - f.metrics.ascent * 0.3 - thickness / 2.0);
            }
            if f.decoration & TextDecorationLine::text_decoration_line_overline.index() != 0 {
                line(pos.y.value());
            }
        }
    }

    fn draw_list_marker(&self, _hdc: u64, marker: &ListMarker) {
        if !self.painting() {
            return;
        }
        let mut c = Command { rect: rect_of(&marker.pos), color: color_of(&marker.color), ..Command::default() };
        let t = marker.marker_type;
        if t == ListStyleType::list_style_type_none.index() {
            return;
        } else if t == ListStyleType::list_style_type_disc.index() {
            c.kind = Cmd::FillEllipse;
        } else if t == ListStyleType::list_style_type_circle.index() {
            c.kind = Cmd::StrokeEllipse;
            c.width = 1.0;
        } else if t == ListStyleType::list_style_type_square.index() {
            c.kind = Cmd::FillRect;
        } else if t == ListStyleType::list_style_type_disclosure_open.index()
            || t == ListStyleType::list_style_type_disclosure_closed.index()
        {
            // The marker box is a third of the font size; the triangle takes half of it, in the 1em an inside marker
            // is given.
            let em = c.rect.w * 3.0;
            let cx = c.rect.x + em * 0.3;
            let cy = c.rect.y + c.rect.h / 2.0;
            let open = t == ListStyleType::list_style_type_disclosure_open.index();
            let closed_points: [f32; 6] = [-0.2, -0.25, 0.25, 0.0, -0.2, 0.25];
            let open_points: [f32; 6] = [-0.25, -0.2, 0.25, -0.2, 0.0, 0.25];
            let points = if open { &open_points } else { &closed_points };
            let mut triangle = Path::default();
            for i in (0..6).step_by(2) {
                triangle.ops.push(if i != 0 { PathOp::Line } else { PathOp::Move });
                triangle.points.push(cx + points[i] * em);
                triangle.points.push(cy + points[i + 1] * em);
            }
            triangle.ops.push(PathOp::Close);
            triangle.fill = true;
            triangle.fill_color = c.color;
            c.kind = Cmd::Path;
            c.path = Some(Arc::new(triangle));
        } else {
            let Some(f) = self.fonts.borrow().fonts.get(&marker.font).cloned() else { return };
            c.kind = Cmd::Text;
            c.font = f.handle;
            c.text = marker_text(t, marker.index);
            c.x = marker.pos.x.value();
            c.y = marker.pos.y.value() + marker.pos.height.value() - f.metrics.descent;
        }
        self.push(c);
    }

    fn draw_image(&self, _hdc: u64, layer: &BackgroundLayer, url: &[u8], _base_url: &[u8]) {
        if !self.painting() {
            return;
        }
        let Some(img) = self.images.borrow().map.get(url).copied() else { return };
        if img.id == 0 && img.canvas == 0 && !img.placeholder {
            return;
        }
        if img.placeholder {
            self.draw_placeholder(layer);
            return;
        }
        let mut c = Command {
            kind: Cmd::Image,
            image: img.id,
            canvas: img.canvas,
            rect: rect_of(&layer.origin_box),
            radii: radii_of(&layer.border_radius),
            clip: rect_of(&layer.clip_box),
            has_clip: true,
            ..Command::default()
        };
        // the border box bounds the drawing too
        let b = rect_of(&layer.border_box);
        clip_intersect(&mut c.clip, &b);
        // background-repeat: the tile is placed once by litehtml and repeated here over the clip, as one command per
        // tile, up to a bound past which a painter would only be slowed down.
        let across = layer.repeat == BackgroundRepeat::background_repeat_repeat.index()
            || layer.repeat == BackgroundRepeat::background_repeat_repeat_x.index();
        let down = layer.repeat == BackgroundRepeat::background_repeat_repeat.index()
            || layer.repeat == BackgroundRepeat::background_repeat_repeat_y.index();
        let tile = c.rect;
        if (across || down) && tile.w >= 1.0 && tile.h >= 1.0 && !c.clip.empty() && self.list_len() < MAX_TILED_COMMANDS {
            let x0 = if across { tile.x - ((tile.x - c.clip.x) / tile.w).ceil() * tile.w } else { tile.x };
            let y0 = if down { tile.y - ((tile.y - c.clip.y) / tile.h).ceil() * tile.h } else { tile.y };
            let x1 = if across { c.clip.x + c.clip.w } else { tile.x + tile.w };
            let y1 = if down { c.clip.y + c.clip.h } else { tile.y + tile.h };
            // Tiles are counted, not stepped to: far from the origin a float no longer moves by a tile's size.
            let columns = ((x1 - x0) / tile.w).ceil();
            let rows = ((y1 - y0) / tile.h).ceil();
            if columns < 1.0 || rows < 1.0 {
                return;
            }
            if columns * rows <= 4096.0 {
                for row in 0..(rows as i32) {
                    for column in 0..(columns as i32) {
                        let mut t = c.clone();
                        t.rect = Rect::new(x0 + column as f32 * tile.w, y0 + row as f32 * tile.h, tile.w, tile.h);
                        self.push(t);
                    }
                }
                return;
            }
        }
        self.push(c);
    }

    fn draw_solid_fill(&self, _hdc: u64, layer: &BackgroundLayer, color: &WebColor) {
        if !self.painting() {
            return;
        }
        if color.alpha == 0 {
            return;
        }
        let mut c = Command { kind: Cmd::FillRect, color: color_of(color), ..Command::default() };
        if layer.is_root {
            // the root background covers the whole canvas, like a browser's
            c.rect = Rect::new(0.0, 0.0, self.width.get() as f32, self.vp_height.get() as f32);
        } else {
            c.rect = rect_of(&layer.border_box);
            c.radii = radii_of(&layer.border_radius);
            c.clip = rect_of(&layer.clip_box);
            c.has_clip = true;
        }
        self.push(c);
    }

    fn draw_linear_gradient(&self, _hdc: u64, layer: &BackgroundLayer, g: &LinearGradient) {
        if !self.painting() {
            return;
        }
        let mut c = Command { kind: Cmd::Linear, ..Command::default() };
        Container::<K>::gradient_common(&mut c, layer, &g.base);
        c.ax = g.start.x;
        c.ay = g.start.y;
        c.bx = g.end.x;
        c.by = g.end.y;
        self.push(c);
    }

    fn draw_radial_gradient(&self, _hdc: u64, layer: &BackgroundLayer, g: &RadialGradient) {
        if !self.painting() {
            return;
        }
        let mut c = Command { kind: Cmd::Radial, ..Command::default() };
        Container::<K>::gradient_common(&mut c, layer, &g.base);
        c.ax = g.position.x;
        c.ay = g.position.y;
        c.bx = g.radius.x;
        c.by = g.radius.y;
        self.push(c);
    }

    fn draw_conic_gradient(&self, _hdc: u64, layer: &BackgroundLayer, g: &ConicGradient) {
        if !self.painting() {
            return;
        }
        let mut c = Command { kind: Cmd::Conic, ..Command::default() };
        Container::<K>::gradient_common(&mut c, layer, &g.base);
        c.ax = g.position.x;
        c.ay = g.position.y;
        c.bx = g.angle;
        c.by = g.radius;
        self.push(c);
    }

    fn draw_borders(&self, _hdc: u64, borders: &Borders, draw_pos: &Position, _root: bool) {
        if !self.painting() {
            return;
        }
        self.borders(borders, draw_pos);
    }

    fn set_clip(&self, pos: &Position, bdr_radius: &BorderRadiuses) {
        if !self.painting() {
            return;
        }
        let c = Command { kind: Cmd::ClipPush, rect: rect_of(pos), radii: radii_of(bdr_radius), ..Command::default() };
        let mut p = self.paint.borrow_mut();
        p.clips.push(c.clone());
        p.list.push(c);
    }

    fn del_clip(&self) {
        if !self.painting() {
            return;
        }
        let mut p = self.paint.borrow_mut();
        p.clips.pop();
        p.list.push(Command { kind: Cmd::ClipPop, ..Command::default() });
    }

    // A fixed element outside every group escapes its ancestors' overflow clips, as in WebKit: the clips are popped
    // around it and pushed again after.
    fn suspend_clips(&self) {
        if !self.painting() {
            return;
        }
        let mut p = self.paint.borrow_mut();
        for _ in 0..p.clips.len() {
            p.list.push(Command { kind: Cmd::ClipPop, ..Command::default() });
        }
        let clips = std::mem::take(&mut p.clips);
        p.suspended.push(clips);
    }

    fn resume_clips(&self) {
        if !self.painting() {
            return;
        }
        let mut p = self.paint.borrow_mut();
        let Some(back) = p.suspended.pop() else { return };
        for _ in 0..p.clips.len() {
            p.list.push(Command { kind: Cmd::ClipPop, ..Command::default() });
        }
        p.clips = back;
        let again: Vec<Command> = p.clips.clone();
        p.list.extend(again);
    }

    fn draw_box_shadow(&self, _hdc: u64, bx: &Position, radius: &BorderRadiuses, shadow: &ShadowPaint) {
        if !self.painting() {
            return;
        }
        if shadow.color.alpha == 0 || (self.no_effects.get() && shadow.blur.value() > 0.0) {
            return;
        }
        self.push(Command {
            kind: Cmd::Shadow,
            rect: rect_of(bx),
            radii: radii_of(radius),
            color: color_of(&shadow.color),
            ax: shadow.x.value(),
            ay: shadow.y.value(),
            bx: shadow.blur.value(),
            by: shadow.spread.value(),
            inset: shadow.inset,
            ..Command::default()
        });
    }

    fn set_clip_path(&self, path: &VectorPath) {
        if !self.painting() {
            return;
        }
        let mut p = path_of(path);
        p.evenodd = path.evenodd;
        let (mut x0, mut y0, mut x1, mut y1) = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        let mut i = 0;
        while i + 1 < p.points.len() {
            x0 = cmin(x0, p.points[i]);
            x1 = cmax(x1, p.points[i]);
            y0 = cmin(y0, p.points[i + 1]);
            y1 = cmax(y1, p.points[i + 1]);
            i += 2;
        }
        let rect = if x0 <= x1 && y0 <= y1 { Rect::new(x0, y0, x1 - x0, y1 - y0) } else { Rect::default() };
        let c = Command { kind: Cmd::ClipPush, rect, path: Some(Arc::new(p)), ..Command::default() };
        let mut pt = self.paint.borrow_mut();
        pt.clips.push(c.clone());
        pt.list.push(c);
    }

    fn point_in_clip_path(&self, path: &VectorPath, x: Px, y: Px) -> bool {
        let p = path_of(path);
        let g = twidget_wire::Geometry { ops: p.ops, points: p.points };
        twidget_canvas::geometry_contains(&g, f64::from(x.value()), f64::from(y.value()), path.evenodd)
    }

    fn begin_group(&self, _hdc: u64, group: &PaintGroup) {
        if !self.painting() {
            return;
        }
        let mut p = self.paint.borrow_mut();
        // Backface visibility needs the accumulated matrix: a flip undone by an outer flip is a front face.
        let total = match p.groups.last() {
            None => group.matrix,
            Some(back) => group.matrix.then(back),
        };
        p.groups.push(total);
        let m = &group.matrix;
        let mut c = Command {
            kind: Cmd::GroupPush,
            matrix: [m.a, m.b, m.c, m.d, m.tx, m.ty],
            opacity: group.opacity,
            hidden: group.backface_hidden && total.a * total.d - total.b * total.c < 0.0,
            ..Command::default()
        };
        if self.no_effects.get() && c.opacity < 1.0 {
            // A box that was invisible stays so, without a group to blend; a disabled button keeps its dimming.
            if c.opacity <= 0.0 {
                c.hidden = true;
            } else if !group.owner.is_some_and(|o| self.disabled_buttons.borrow().contains(&o)) {
                c.opacity = 1.0;
            }
        }
        c.rect = rect_of(&group.bounds);
        p.list.push(c);
    }

    fn draw_filtered(&self, hdc: u64, filter: &FilterPaint, paint: &mut dyn FnMut(u64)) {
        let filters = if !self.painting() || self.no_effects.get() { None } else { self.filters_of(filter) };
        let Some(filters) = filters else {
            paint(hdc);
            return;
        };
        self.push(Command { kind: Cmd::FilterPush, rect: rect_of(&filter.bounds), filter: Some(filters), ..Command::default() });
        paint(hdc);
        self.push(Command { kind: Cmd::FilterPop, ..Command::default() });
    }

    fn draw_backdrop(&self, _hdc: u64, filter: &FilterPaint) {
        if !self.painting() || self.no_effects.get() {
            return;
        }
        let Some(filters) = self.filters_of(filter) else { return };
        self.push(Command {
            kind: Cmd::Backdrop,
            rect: rect_of(&filter.r#box),
            radii: radii_of(&filter.radius),
            filter: Some(filters),
            ..Command::default()
        });
    }

    fn draw_path(&self, _hdc: u64, path: &VectorPath) {
        if !self.painting() {
            return;
        }
        if !path.fill && !path.stroke {
            return;
        }
        let mut p = path_of(path);
        p.fill = path.fill;
        p.evenodd = path.evenodd;
        p.fill_color = color_of(&path.fill_color);
        let g = &path.fill_gradient;
        p.gradient = g.ty;
        p.x1 = g.x1;
        p.y1 = g.y1;
        p.x2 = g.x2;
        p.y2 = g.y2;
        p.r = g.r;
        p.fx = g.fx;
        p.fy = g.fy;
        for (offset, color) in &g.stops {
            p.stops.push(Stop::new(*offset, color_of(color)));
        }
        p.stroke = path.stroke;
        p.stroke_color = color_of(&path.stroke_color);
        p.stroke_width = path.stroke_width;
        p.line_cap = path.line_cap;
        p.line_join = path.line_join;
        p.miter_limit = path.miter_limit;
        p.dashes = path.dashes.clone();
        p.dash_offset = path.dash_offset;
        let sg = &path.stroke_gradient;
        p.stroke_gradient = sg.ty;
        p.sg = [sg.x1, sg.y1, sg.x2, sg.y2, sg.r, sg.fx, sg.fy];
        for (offset, color) in &sg.stops {
            p.stroke_stops.push(Stop::new(*offset, color_of(color)));
        }
        self.push(Command { kind: Cmd::Path, path: Some(Arc::new(p)), ..Command::default() });
    }

    fn end_group(&self, _hdc: u64) {
        if !self.painting() {
            return;
        }
        let mut p = self.paint.borrow_mut();
        p.groups.pop();
        p.list.push(Command { kind: Cmd::GroupPop, ..Command::default() });
    }

    /// An SVG paint under a key that stands for all it draws: recorded once, then copied in, shifted by how far the
    /// box moved, inside a keyed group a painter may raster once (Cmd::GroupPush).
    fn draw_cached(&self, hdc: u64, bx: &Position, key: u64, paint: &mut dyn FnMut(u64)) -> bool {
        // The trace walks into the paint for its elements.
        if !self.painting() {
            return false;
        }
        let r = rect_of(bx);
        if r.empty() {
            return false;
        }
        let mut push = Command { kind: Cmd::GroupPush, key, rect: r, ..Command::default() };
        let pop = Command { kind: Cmd::GroupPop, ..Command::default() };
        {
            let mut p = self.paint.borrow_mut();
            p.cached_clock += 1;
            let clock = p.cached_clock;
            if let Some(hit) = p.cached.get_mut(&key) {
                // The key holds the fractional offset: a move is whole pixels.
                push.matrix[4] = r.x - hit.x;
                push.matrix[5] = r.y - hit.y;
                push.rect = Rect::new(hit.x, hit.y, r.w, r.h);
                hit.used = clock;
                let commands = hit.commands.clone();
                p.list.push(push);
                p.list.extend(commands);
                p.list.push(pop);
                return true;
            }
            p.cached_clock -= 1;
            p.list.push(push);
        }
        let start = self.list_len();
        paint(hdc);
        let mut p = self.paint.borrow_mut();
        let count = p.list.len() - start;
        if count <= MAX_CACHED_COMMANDS / 4 {
            while !p.cached.is_empty() && (p.cached.len() >= MAX_CACHED_PAINTS || p.cached_commands + count > MAX_CACHED_COMMANDS) {
                let Some((&oldest, _)) = p.cached.iter().min_by_key(|(_, c)| c.used) else { break };
                if let Some(gone) = p.cached.remove(&oldest) {
                    p.cached_commands -= gone.commands.len();
                }
            }
            p.cached_clock += 1;
            let clock = p.cached_clock;
            let commands = p.list[start..].to_vec();
            let entry = p.cached.entry(key).or_default();
            entry.x = r.x;
            entry.y = r.y;
            entry.used = clock;
            entry.commands = commands;
            p.cached_commands += count;
        }
        p.list.push(pop);
        true
    }

    fn set_cursor(&self, cursor: &[u8]) {
        *self.cursor.borrow_mut() = c_str(cursor).to_vec();
    }
}
