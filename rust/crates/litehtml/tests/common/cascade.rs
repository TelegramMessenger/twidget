//! The Rust side of lhdump's `l` record (stage L5, the cascade): a document as `createUpdatableFromString`
//! finalizes it with a master and a user sheet under a viewport, then steps, and every element's computed style.
//!
//! - `l` f32 width, height, dpr, master, user, html, u32 steps (u8 `d`; `m` f32 width, height; `h`, `a`, `x` u32 k;
//!   `r`): u8 mode, then for each step a dump (`d`) or u8 (`m`: whether `media_changed` restyled; `h`, `a`, `x`:
//!   bit 0 the pseudo-classes changed, bit 1 `find_styles_changes` restyled; `r`: 1), and the names the case added to
//!   the process's table (u32 count, each), all as one string; then the
//!   container's calls by phase (the finalization, then each step): u32 phases, each u32 count and its calls (u8
//!   kind `F` font, `W` width, `I` image, `T` transform, `L` a limit met, its texts and a u64). The finalization and
//!   a media change that restyles build the render tree (stage L6) too, whose anonymous boxes compute their style,
//!   whose list items get `list_index` and whose table cells their column's width.
//! - `o` `r`, then as `l`: the same, but each dump step dumps the render tree (see `render`).
//!
//! A dump: u32 elements in tree order from the root, each u8 class, i32 parent, tag, the computed style (`css_out`,
//! the share id as the first element in the walk that has it, since the C++'s render tree takes share ids too), for
//! a tag its matched selectors (u64 place, u8 used), style, u8 has base, base style, i32 counter changes, u8
//! anonymous; the counters (name, i32); a text's shown text, u8 transformed, u8 draw spaces, f32 width and height,
//! u64 measured font, i32 measured transform, u8 white space, u8 break; a generated box's generations, boxes and u64
//! bytes; a control's look (u8 native, dark, disabled), a select's u8 arrow, an image's u8 loaded. Then the fonts
//! (hash, u64 handle, metrics), u32 shared styles, the media lists (u8 used), i32 ids selectable, u8 backdrop, i32
//! generated boxes, u64 bytes, u8 limited.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use super::*;
use twidget_litehtml::css_effects::{BoxShadow, FilterFunction, GridTrackList};
use twidget_litehtml::css_properties::{CssBorder, CssMargins, CssProperties};
use twidget_litehtml::cxx;
use twidget_litehtml::document::{Document, StyleCx, Switches};
use twidget_litehtml::element::{ElementId, ElementKind, ElementState, FormLook};
use twidget_litehtml::font_description::FontDescription;
use twidget_litehtml::types::{Position, RenderType, Size};
use twidget_litehtml::web_color::WebColor;

use super::paint::PaintLog;
use twidget_litehtml::background::{BackgroundLayer, ConicGradient, LinearGradient, RadialGradient};
use twidget_litehtml::borders::{BorderRadiuses, Borders};
use twidget_litehtml::css_effects::{FilterPaint, PaintGroup, ShadowPaint, VectorPath};
use twidget_litehtml::document_container::ListMarker;

/// A mark: byte offset, field, element (-1 for a field).
pub type Mark = (usize, &'static str, i32);

thread_local! {
    static MARKS: RefCell<Option<Vec<Mark>>> = const { RefCell::new(None) };
}

/// Marks of where each element and field of the answers that follow starts, to tell a difference by name.
pub fn start_marks() {
    MARKS.with(|m| *m.borrow_mut() = Some(Vec::new()));
}

/// The marks since `start_marks`: byte offset, field, element (-1 for a field).
pub fn take_marks() -> Vec<Mark> {
    MARKS.with(|m| m.borrow_mut().take().unwrap_or_default())
}

pub fn mark(o: &Out, what: &'static str, element: i32) {
    MARKS.with(|m| {
        if let Some(marks) = m.borrow_mut().as_mut() {
            marks.push((o.s.len(), what, element));
        }
    });
}

/// lhdump's `CascadeContainer`: the renderer's answers where the fork asks, every font, width, image and transform
/// call logged; for layout the viewport, image sizes from the source's bytes and text breakers that break between
/// any two characters, logged too; for painting an image's aspect, what follows a `#` in its source, logged too.
pub struct CascadeContainer {
    pub width: Cell<f32>,
    pub height: Cell<f32>,
    pub dpr: Cell<f32>,
    pub trusted: Cell<bool>,
    /// Stage L19's sanitizer of elements (lhdump's `sanitizing`): no `object` or `embed`, `blink` as `span`, no `on*`
    /// attributes or `javascript:` sources; each call logged (`E`).
    pub sanitizing: Cell<bool>,
    pub calls: RefCell<Vec<Call>>,
    sizes: RefCell<BTreeMap<u64, f32>>,
    next: Cell<u64>,
    /// The breakers made, by handle - 1: their text and font size.
    breakers: RefCell<Vec<(Vec<u8>, f32)>>,
    /// What the paint calls answer (stage L13).
    pub paint: PaintLog,
    /// Stage L16: the pointer's calls log their elements in this numbering (-1 none, -2 out of it), and the elements
    /// that take a click (a button or a label, as lhdump's `on_element_click` asks the tag).
    pub hit_elements: RefCell<BTreeMap<ElementId, i32>>,
    pub hit_takers: RefCell<std::collections::BTreeSet<ElementId>>,
}

/// The code points of a text as byte ranges (a lead byte and its continuation bytes).
fn code_point_ends(text: &[u8], from: usize) -> Vec<usize> {
    let mut ends = Vec::new();
    let mut i = from;
    while i < text.len() {
        let mut next = i + 1;
        while next < text.len() && (text[next] & 0xC0) == 0x80 {
            next += 1;
        }
        ends.push(next);
        i = next;
    }
    ends
}

impl CascadeContainer {
    pub fn new(width: f32, height: f32, dpr: f32) -> CascadeContainer {
        CascadeContainer {
            width: Cell::new(width),
            height: Cell::new(height),
            dpr: Cell::new(dpr),
            trusted: Cell::new(false),
            sanitizing: Cell::new(false),
            calls: RefCell::new(Vec::new()),
            sizes: RefCell::new(BTreeMap::new()),
            next: Cell::new(0),
            breakers: RefCell::new(Vec::new()),
            paint: PaintLog::default(),
            hit_elements: RefCell::new(BTreeMap::new()),
            hit_takers: RefCell::new(std::collections::BTreeSet::new()),
        }
    }

    fn hit_element(&self, el: Option<ElementId>) -> i32 {
        match el {
            None => -1,
            Some(el) => self.hit_elements.borrow().get(&el).copied().unwrap_or(-2),
        }
    }

    fn log(&self, kind: u8, a: &[u8], b: Vec<u8>, hdc: u64) {
        self.calls.borrow_mut().push((kind, a.to_vec(), b, hdc));
    }
}

impl twidget_litehtml::web_color::ResolveColor for CascadeContainer {
    fn resolve_color(&self, _name: &[u8]) -> Vec<u8> {
        Vec::new()
    }
}

impl DocumentContainer for CascadeContainer {
    fn pt_to_px(&self, pt: f32) -> Px {
        Px(pt * 96.0 / 72.0)
    }
    fn get_default_font_size(&self) -> Px {
        Px(16.0)
    }
    fn import_css(&self, url: &[u8], baseurl: &mut Vec<u8>) -> Vec<u8> {
        TestContainer.import_css(url, baseurl)
    }
    fn get_media_features(&self) -> MediaFeatures {
        MediaFeatures {
            ty: MediaType::media_type_screen.index(),
            width: Px(self.width.get()),
            height: Px(self.height.get()),
            device_width: Px(self.width.get()),
            device_height: Px(self.height.get()),
            color: 8,
            color_index: 256,
            monochrome: 0,
            resolution: Px(96.0 * self.dpr.get()),
        }
    }
    fn sanitize_declaration(&self, name: &[u8], value: &[CssToken]) -> bool {
        renderer_sanitize_declaration(self.trusted.get(), name, value)
    }
    fn trusted_styles(&self, trusted: bool) {
        self.trusted.set(trusted);
    }
    fn sanitize_element(&self, tag: &mut Vec<u8>, svg: bool, attrs: &mut twidget_litehtml::element::StringMap) -> bool {
        if !self.sanitizing.get() {
            return true;
        }
        self.calls.borrow_mut().push((b'E', tag.clone(), Vec::new(), u64::from(svg) | (attrs.len() as u64) << 1));
        if tag == b"object" || tag == b"embed" {
            return false;
        }
        if tag == b"blink" {
            *tag = b"span".to_vec();
        }
        attrs.retain(|k, v| !(k.starts_with(b"on") || (k == b"src" && v.starts_with(b"javascript:"))));
        true
    }
    fn sanitizer_limit(&self, what: &[u8]) {
        self.calls.borrow_mut().push((b'L', what.to_vec(), Vec::new(), 0));
    }
    fn create_font(&self, d: &FontDescription) -> (u64, FontMetrics) {
        self.calls.borrow_mut().push((b'F', d.hash(), Vec::new(), 0));
        let id = self.next.get() + 1;
        self.next.set(id);
        self.sizes.borrow_mut().insert(id, d.size.value());
        let size = d.size.value();
        let ascent = size * if d.family.len().is_multiple_of(3) { 0.75 } else { 0.8 };
        let descent = size * 0.25;
        let height = size * 1.15 + if d.weight >= 600 { 1.0 } else { 0.0 };
        let x_height = if d.style == FontStyle::font_style_italic.index() { 0.0 } else { ascent * 0.6 };
        let zero_width = size * 0.5 + d.weight as f32 / 1000.0;
        let fm = FontMetrics {
            font_size: d.size,
            ascent: Px(ascent),
            descent: Px(descent),
            height: Px(cxx::max_f32(height, ascent + descent)),
            x_height: Px(if x_height > 0.0 { x_height } else { ascent * 0.5 }),
            ch_width: Px(zero_width),
            draw_spaces: d.decoration_line != 0,
            sub_shift: Px(size / 5.0),
            super_shift: Px(size / 3.0),
        };
        (id, fm)
    }
    fn text_width(&self, text: &[u8], font: u64) -> Px {
        self.calls.borrow_mut().push((b'W', text.to_vec(), Vec::new(), font));
        match self.sizes.borrow().get(&font) {
            Some(size) => Px(text.len() as f32 * size * 0.5),
            None => Px(0.0),
        }
    }
    fn load_image(&self, src: &[u8], baseurl: Option<&[u8]>, redraw_on_ready: bool) {
        let n = if baseurl.is_some() { 2 } else { 0 } | u64::from(redraw_on_ready);
        self.calls.borrow_mut().push((b'I', src.to_vec(), baseurl.unwrap_or_default().to_vec(), n));
    }
    fn get_viewport(&self) -> Position {
        self.calls.borrow_mut().push((b'V', Vec::new(), Vec::new(), 0));
        Position::new(Px(0.0), Px(0.0), Px(self.width.get()), Px(self.height.get()))
    }
    fn get_image_size(&self, src: &[u8], baseurl: Option<&[u8]>) -> Size {
        self.calls.borrow_mut().push((b'S', src.to_vec(), baseurl.unwrap_or_default().to_vec(), u64::from(baseurl.is_some())));
        if src.is_empty() {
            return Size::default();
        }
        let sum = src.iter().fold(0u32, |sum, &c| sum.wrapping_add(u32::from(c)));
        Size::new(Px((sum % 200) as f32), Px((sum.wrapping_mul(7).wrapping_add(src.len() as u32) % 150) as f32))
    }
    fn create_text_breaker(&self, text: &[u8], font: u64) -> u64 {
        self.calls.borrow_mut().push((b'B', text.to_vec(), Vec::new(), font));
        let size = self.sizes.borrow().get(&font).copied().unwrap_or(0.0);
        let mut breakers = self.breakers.borrow_mut();
        breakers.push((text.to_vec(), size));
        breakers.len() as u64
    }
    fn text_breaker_fit(&self, breaker: u64, start: usize, width: Px) -> usize {
        let mut at = (start as u32).to_le_bytes().to_vec();
        at.extend(width.value().to_le_bytes());
        self.calls.borrow_mut().push((b'b', at, Vec::new(), breaker));
        let breakers = self.breakers.borrow();
        let (text, size) = &breakers[breaker as usize - 1];
        if start >= text.len() {
            return start;
        }
        let mut end = start;
        for next in code_point_ends(text, start) {
            if (next - start) as f32 * size * 0.5 > width.value() {
                if end == start {
                    end = next;
                }
                break;
            }
            end = next;
        }
        end
    }
    fn text_breaker_minimum_width(&self, breaker: u64) -> Px {
        self.calls.borrow_mut().push((b'm', Vec::new(), Vec::new(), breaker));
        let breakers = self.breakers.borrow();
        let (text, size) = &breakers[breaker as usize - 1];
        let mut widest = 0.0f32;
        let mut i = 0;
        for next in code_point_ends(text, 0) {
            widest = cxx::max_f32(widest, (next - i) as f32 * size * 0.5);
            i = next;
        }
        Px(widest)
    }
    fn transform_text(&self, text: &mut Vec<u8>, tt: i32) {
        self.calls.borrow_mut().push((b'T', text.clone(), Vec::new(), tt as u64));
        if tt == TextTransform::text_transform_uppercase.index() {
            text.make_ascii_uppercase();
        } else if tt == TextTransform::text_transform_lowercase.index() {
            text.make_ascii_lowercase();
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
    fn draw_text(&self, hdc: u64, text: &[u8], font: u64, color: WebColor, pos: &Position) {
        self.log(b't', text, PaintLog::text(font, color, pos), hdc);
    }
    fn draw_list_marker(&self, hdc: u64, marker: &ListMarker) {
        self.log(b'k', &marker.image, PaintLog::marker(marker), hdc);
    }
    fn draw_image(&self, hdc: u64, layer: &BackgroundLayer, url: &[u8], base_url: &[u8]) {
        self.log(b'g', url, PaintLog::image(layer, base_url), hdc);
    }
    fn draw_solid_fill(&self, hdc: u64, layer: &BackgroundLayer, color: &WebColor) {
        self.log(b'f', b"", PaintLog::fill(layer, color), hdc);
    }
    fn draw_linear_gradient(&self, hdc: u64, layer: &BackgroundLayer, gradient: &LinearGradient) {
        self.log(b'l', b"", PaintLog::linear(layer, gradient), hdc);
    }
    fn draw_radial_gradient(&self, hdc: u64, layer: &BackgroundLayer, gradient: &RadialGradient) {
        self.log(b'r', b"", PaintLog::radial(layer, gradient), hdc);
    }
    fn draw_conic_gradient(&self, hdc: u64, layer: &BackgroundLayer, gradient: &ConicGradient) {
        self.log(b'c', b"", PaintLog::conic(layer, gradient), hdc);
    }
    fn draw_borders(&self, hdc: u64, borders: &Borders, draw_pos: &Position, root: bool) {
        self.log(b'd', b"", PaintLog::borders(borders, draw_pos, root), hdc);
    }
    fn set_clip(&self, pos: &Position, bdr_radius: &BorderRadiuses) {
        self.log(b'C', b"", PaintLog::clip(pos, bdr_radius), 0);
    }
    fn del_clip(&self) {
        self.log(b'D', b"", Vec::new(), 0);
    }
    fn suspend_clips(&self) {
        self.log(b'U', b"", Vec::new(), 0);
    }
    fn resume_clips(&self) {
        self.log(b'R', b"", Vec::new(), 0);
    }
    fn draw_box_shadow(&self, hdc: u64, bx: &Position, radius: &BorderRadiuses, shadow: &ShadowPaint) {
        self.log(b'h', b"", PaintLog::shadow(bx, radius, shadow), hdc);
    }
    fn set_clip_path(&self, path: &VectorPath) {
        self.log(b'P', b"", PaintLog::path(path), 0);
    }
    fn begin_group(&self, hdc: u64, group: &PaintGroup) {
        self.log(b'G', b"", self.paint.group(group), hdc);
    }
    fn draw_filtered(&self, hdc: u64, filter: &FilterPaint, paint: &mut dyn FnMut(u64)) {
        self.log(b'X', b"", self.paint.filter(filter), hdc);
        paint(hdc * 2 + 1);
        self.log(b'x', b"", Vec::new(), hdc);
    }
    fn draw_backdrop(&self, hdc: u64, filter: &FilterPaint) {
        self.log(b'K', b"", self.paint.filter(filter), hdc);
    }
    fn draw_path(&self, hdc: u64, path: &VectorPath) {
        self.log(b'p', b"", PaintLog::path(path), hdc);
    }
    fn end_group(&self, hdc: u64) {
        self.log(b'E', b"", Vec::new(), hdc);
    }
    fn draw_cached(&self, hdc: u64, bx: &Position, key: u64, paint: &mut dyn FnMut(u64)) -> bool {
        self.log(b'A', b"", PaintLog::boxed(bx, Some(key)), hdc);
        if !self.paint.caching.get() {
            return false;
        }
        let new = self.paint.cached_keys.borrow_mut().insert(key);
        if new {
            paint(hdc * 2 + 1);
            self.log(b'a', b"", Vec::new(), hdc);
        }
        true
    }
    fn draw_isolated(&self, hdc: u64, bx: &Position, paint: &mut dyn FnMut(u64)) {
        self.log(b'O', b"", PaintLog::boxed(bx, None), hdc);
        paint(hdc * 2 + 1);
        self.log(b'o', b"", Vec::new(), hdc);
    }
    fn controls_active(&self) -> bool {
        self.log(b'Q', b"", Vec::new(), 0);
        self.paint.active.get()
    }
    fn control_accent(&self) -> WebColor {
        self.log(b'q', b"", Vec::new(), 0);
        self.paint.accent.get()
    }
    fn get_image_aspect(&self, src: &[u8]) -> Vec<u8> {
        self.calls.borrow_mut().push((b'N', src.to_vec(), Vec::new(), 0));
        src.iter().position(|&c| c == b'#').map_or_else(Vec::new, |at| src[at + 1..].to_vec())
    }
    fn point_in_clip_path(&self, path: &VectorPath, x: Px, y: Px) -> bool {
        let p = &path.points;
        let mut inside = false;
        if p.len() >= 2 {
            let (mut min_x, mut min_y) = (p[0], p[1]);
            let (mut max_x, mut max_y) = (min_x, min_y);
            let mut i = 0;
            while i + 1 < p.len() {
                if p[i] < min_x {
                    min_x = p[i];
                }
                if p[i] > max_x {
                    max_x = p[i];
                }
                if p[i + 1] < min_y {
                    min_y = p[i + 1];
                }
                if p[i + 1] > max_y {
                    max_y = p[i + 1];
                }
                i += 2;
            }
            inside = x.value() >= min_x && x.value() < max_x && y.value() >= min_y && y.value() < max_y;
        }
        let mut b = PaintLog::path(path);
        let mut o = Out { s: std::mem::take(&mut b) };
        super::svg::Answer::a32(&mut o, x.value());
        super::svg::Answer::a32(&mut o, y.value());
        self.log(b'J', b"", o.take(), u64::from(inside));
        inside
    }
    fn on_mouse_event(&self, el: Option<ElementId>, event: twidget_litehtml::document_container::MouseEvent) {
        self.log(b'M', b"", self.hit_element(el).to_le_bytes().to_vec(), event as u64);
    }
    fn set_cursor(&self, cursor: &[u8]) {
        self.log(b'^', cursor, Vec::new(), 0);
    }
    fn on_anchor_click(&self, url: &[u8], el: ElementId) {
        self.log(b'@', url, self.hit_element(Some(el)).to_le_bytes().to_vec(), 0);
    }
    fn on_element_click(&self, el: ElementId) -> bool {
        let taken = self.hit_takers.borrow().contains(&el);
        self.log(b'!', b"", self.hit_element(Some(el)).to_le_bytes().to_vec(), u64::from(taken));
        taken
    }
}

/// lhdump's `write_len`.
pub fn len_out(o: &mut Out, len: &CssLength) {
    o.u8(u8::from(len.is_predefined())).i32(len.predef()).u8(len.units().index() as u8).f32(len.val());
    o.u8(u8::from(len.calc().is_some()));
    if len.calc().is_some() {
        o.f32(len.calc_percent(Px(100.0)).value()).f32(len.calc_percent(Px(-37.5)).value()).str(&len.to_string());
    }
}

fn margins_out(o: &mut Out, m: &CssMargins) {
    for l in [&m.left, &m.right, &m.top, &m.bottom] {
        len_out(o, l);
    }
}

fn border_out(o: &mut Out, b: &CssBorder) {
    len_out(o, &b.width);
    o.i32(b.style).color(&b.color);
}

fn track_list_out(o: &mut Out, list: &GridTrackList) {
    for sizes in [&list.tracks, &list.repeat] {
        o.u32(sizes.len() as u32);
        for t in sizes {
            o.i32(t.min_type as i32).i32(t.max_type as i32);
            len_out(o, &t.min_len);
            len_out(o, &t.max_len);
            o.f32(t.fr);
        }
    }
    o.i32(list.repeat_at).u8(u8::from(list.auto_fit));
}

fn shadows_out(o: &mut Out, shadows: &[BoxShadow]) {
    o.u32(shadows.len() as u32);
    for s in shadows {
        for l in [&s.x, &s.y, &s.blur, &s.spread] {
            len_out(o, l);
        }
        o.color(&s.color).u8(u8::from(s.inset));
    }
}

fn filters_out(o: &mut Out, filters: &[FilterFunction]) {
    o.u32(filters.len() as u32);
    for f in filters {
        o.i32(f.kind as i32).f32(f.amount);
        for l in [&f.x, &f.y, &f.radius] {
            len_out(o, l);
        }
        o.color(&f.color);
    }
}

pub fn metrics_out(o: &mut Out, fm: &FontMetrics) {
    for v in [fm.font_size, fm.height, fm.ascent, fm.descent, fm.x_height, fm.ch_width] {
        o.f32(v.value());
    }
    o.u8(u8::from(fm.draw_spaces)).f32(fm.sub_shift.value()).f32(fm.super_shift.value());
}

/// lhdump's `write_css`.
pub fn css_out(o: &mut Out, names: &Names, c: &CssProperties, shared: i32) {
    mark(o, "css keywords", -1);
    for v in [
        c.m_el_position,
        c.m_text_align,
        c.m_direction,
        c.m_overflow,
        c.m_overflow_x,
        c.m_overflow_y,
        c.m_white_space,
        c.m_overflow_wrap,
        c.m_word_break,
        c.m_user_select,
        c.m_display,
        c.m_visibility,
        c.m_appearance,
        c.m_resize,
        c.m_box_sizing,
    ] {
        o.i32(v);
    }
    mark(o, "css z-index, vertical-align, float, clear", -1);
    len_out(o, &c.m_z_index);
    o.i32(c.m_vertical_align).i32(c.m_float).i32(c.m_clear);
    mark(o, "css margins", -1);
    margins_out(o, &c.m_css_margins);
    mark(o, "css padding", -1);
    margins_out(o, &c.m_css_padding);
    mark(o, "css borders", -1);
    let b = &c.m_css_borders;
    for side in [&b.left, &b.top, &b.right, &b.bottom] {
        border_out(o, side);
    }
    mark(o, "css radii", -1);
    let r = &b.radius;
    for l in [
        &r.top_left_x,
        &r.top_left_y,
        &r.top_right_x,
        &r.top_right_y,
        &r.bottom_right_x,
        &r.bottom_right_y,
        &r.bottom_left_x,
        &r.bottom_left_y,
    ] {
        len_out(o, l);
    }
    mark(o, "css sizes", -1);
    for l in
        [&c.m_css_width, &c.m_cell_width, &c.m_css_height, &c.m_css_min_width, &c.m_css_min_height, &c.m_css_max_width, &c.m_css_max_height]
    {
        len_out(o, l);
    }
    mark(o, "css offsets", -1);
    margins_out(o, &c.m_css_offsets);
    mark(o, "css text-indent, line-height, list-style", -1);
    len_out(o, &c.m_css_text_indent);
    len_out(o, &c.m_css_line_height);
    len_out(o, &c.m_line_height.css_value);
    o.f32(c.m_line_height.computed_value.value());
    o.i32(c.m_list_style_type).i32(c.m_list_style_position).str(&c.m_list_style_image).str(&c.m_list_style_image_baseurl);
    mark(o, "css background", -1);
    let bg = &c.m_bg;
    o.u32(bg.m_image.len() as u32);
    for img in &bg.m_image {
        o.u8(img.ty as u8).str(&img.url);
        gradient_out(o, names, &img.m_gradient);
    }
    o.str(&bg.m_baseurl).color(&bg.m_color);
    o.u32(bg.m_attachment.len() as u32);
    for v in &bg.m_attachment {
        o.i32(*v);
    }
    for list in [&bg.m_position_x, &bg.m_position_y] {
        o.u32(list.len() as u32);
        for l in list {
            len_out(o, l);
        }
    }
    o.u32(bg.m_size.len() as u32);
    for s in &bg.m_size {
        len_out(o, &s.width);
        len_out(o, &s.height);
    }
    for list in [&bg.m_repeat, &bg.m_clip, &bg.m_origin] {
        o.u32(list.len() as u32);
        for v in list {
            o.i32(*v);
        }
    }
    mark(o, "css font", -1);
    o.u64(c.m_font);
    len_out(o, &c.m_font_size);
    o.str(&c.m_font_family);
    len_out(o, &c.m_font_weight);
    o.i32(c.m_font_style).i32(c.m_text_decoration_line).i32(c.m_text_decoration_style);
    len_out(o, &c.m_text_decoration_thickness);
    o.color(&c.m_text_decoration_color).str(&c.m_text_emphasis_style).color(&c.m_text_emphasis_color).i32(c.m_text_emphasis_position);
    mark(o, "css font metrics", -1);
    metrics_out(o, &c.m_font_metrics);
    mark(o, "css share id, text, colour, cursor, content, table", -1);
    o.i32(shared).i32(c.m_text_transform).i32(c.m_font_smoothing).color(&c.m_color).str(&c.m_cursor).str(&c.m_content);
    o.i32(c.m_border_collapse).i32(c.m_table_layout);
    len_out(o, &c.m_css_border_spacing_x);
    len_out(o, &c.m_css_border_spacing_y);
    mark(o, "css flex", -1);
    o.f32(c.m_flex_grow).f32(c.m_flex_shrink);
    len_out(o, &c.m_flex_basis);
    for v in [
        c.m_flex_direction,
        c.m_flex_wrap,
        c.m_flex_justify_content,
        c.m_flex_align_items,
        c.m_flex_align_self,
        c.m_flex_align_content,
        c.m_caption_side,
        c.m_order,
    ] {
        o.i32(v);
    }
    mark(o, "css gaps, grid", -1);
    len_out(o, &c.m_row_gap);
    len_out(o, &c.m_column_gap);
    o.i32(c.m_justify_items).i32(c.m_justify_self);
    track_list_out(o, &c.m_grid_template_columns);
    track_list_out(o, &c.m_grid_template_rows);
    track_list_out(o, &c.m_grid_auto_rows);
    for p in [&c.m_grid_column, &c.m_grid_row] {
        o.i32(p.start).i32(p.end).i32(p.span);
    }
    mark(o, "css opacity, spacing, features", -1);
    o.f32(c.m_opacity).f32(c.m_aspect_ratio).u8(u8::from(c.m_center_content));
    len_out(o, &c.m_letter_spacing);
    len_out(o, &c.m_word_spacing);
    o.str(&c.m_font_variant_numeric).str(&c.m_font_feature_settings);
    mark(o, "css shadows, filters, clip path", -1);
    shadows_out(o, &c.m_text_shadows);
    shadows_out(o, &c.m_box_shadows);
    filters_out(o, &c.m_filter);
    filters_out(o, &c.m_backdrop_filter);
    o.i32(c.m_clip_path.kind as i32).u32(c.m_clip_path.values.len() as u32);
    for v in &c.m_clip_path.values {
        len_out(o, v);
    }
    o.u8(u8::from(c.m_clip_path.evenodd));
    mark(o, "css transform", -1);
    o.u32(c.m_transform.len() as u32);
    for f in &c.m_transform {
        o.i32(f.kind as i32);
        len_out(o, &f.x);
        len_out(o, &f.y);
        for v in f.v {
            o.f32(v);
        }
    }
    mark(o, "css origin, backface, contain, outline, accent, scheme, keyword size", -1);
    len_out(o, &c.m_transform_origin_x);
    len_out(o, &c.m_transform_origin_y);
    o.u8(u8::from(c.m_transform_fill_box)).i32(c.m_backface_hidden).i32(c.m_contain);
    border_out(o, &c.m_outline);
    len_out(o, &c.m_outline_offset);
    o.color(&c.m_accent_color).i32(c.m_dark_scheme).u8(u8::from(c.m_keyword_size)).u8(u8::from(c.m_generic_mono));
}

/// A style's properties in id order: lhdump's `write_style_map`.
pub fn style_map_out(o: &mut Out, names: &Names, st: &Style) {
    o.u32(st.properties().len() as u32);
    for (&id, p) in st.properties() {
        o.u32(id.0).str(&names.s(id));
        property_out(o, names, p);
    }
}

fn look_out(o: &mut Out, look: &FormLook) {
    o.u8(u8::from(look.native)).u8(u8::from(look.dark)).u8(u8::from(look.disabled));
}

/// The tree from the root, each element with its parent's index.
pub fn cascade_walk(doc: &Document, id: ElementId, parent: i32, out: &mut Vec<(ElementId, i32)>) {
    let index = out.len() as i32;
    out.push((id, parent));
    for &child in doc.children(id) {
        cascade_walk(doc, child, index, out);
    }
}

/// lhdump's `write_cascade`.
pub fn cascade_out(o: &mut Out, names: &Names, doc: &Document) {
    let mut nodes = Vec::new();
    if let Some(root) = doc.root {
        cascade_walk(doc, root, -1, &mut nodes);
    }
    let mut shared: BTreeMap<i32, i32> = BTreeMap::new();
    for (k, (id, _)) in nodes.iter().enumerate() {
        shared.entry(doc.el(*id).css.m_shared_id).or_insert(k as i32);
    }
    o.u32(nodes.len() as u32);
    for (index, &(id, parent)) in nodes.iter().enumerate() {
        let el = doc.el(id);
        let kind = el.kind;
        mark(o, "element", index as i32);
        o.u8(kind as u8).i32(parent).str(&names.s(doc.tag(id)));
        css_out(o, names, &el.css, shared[&el.css.m_shared_id]);
        if kind.is_html_tag() {
            mark(o, "matched selectors", -1);
            o.u32(el.used_styles.len() as u32);
            for us in &el.used_styles {
                o.u64(us.selector.place()).u8(u8::from(us.used));
            }
            mark(o, "style", -1);
            style_map_out(o, names, &el.style);
            mark(o, "base style", -1);
            o.u8(u8::from(el.has_style_base));
            style_map_out(o, names, &el.style_base);
            o.i32(el.counter_changes).u8(u8::from(el.anonymous_style));
        }
        mark(o, "counters", -1);
        o.u32(el.counter_values.len() as u32);
        mark(o, "class state", -1);
        for (&name, &value) in &el.counter_values {
            o.str(&names.s(name)).i32(value);
        }
        match kind {
            ElementKind::el_text | ElementKind::el_space => {
                let t = &el.text_state;
                o.str(&t.transformed_text).u8(u8::from(t.use_transformed)).u8(u8::from(t.draw_spaces));
                o.f32(t.size.0.value()).f32(t.size.1.value()).u64(t.measured_font).i32(t.measured_transform);
                o.u8(u8::from(doc.is_white_space(id))).u8(u8::from(doc.is_break(id)));
            }
            ElementKind::el_before | ElementKind::el_after => {
                let (generations, boxes, bytes) = match el.state {
                    ElementState::Generated { generations, boxes, bytes } => (generations, boxes, bytes),
                    _ => (0, 0, 0),
                };
                o.i32(generations).i32(boxes).u64(bytes as u64);
            }
            ElementKind::el_input | ElementKind::el_button | ElementKind::el_textarea | ElementKind::el_meter => look_out(o, &el.look),
            ElementKind::el_select => {
                look_out(o, &el.look);
                o.u8(u8::from(el.arrow));
            }
            ElementKind::el_image => {
                let loaded = matches!(el.state, ElementState::Image { loaded: true, .. });
                o.u8(u8::from(loaded));
            }
            _ => {}
        }
    }
    mark(o, "document fonts, sharing, media, generated", -2);
    let fonts = doc.fonts.borrow();
    o.u32(fonts.len() as u32);
    for (key, item) in fonts.iter() {
        o.str(key).u64(item.font);
        metrics_out(o, &item.metrics);
    }
    o.u32(doc.share_cache.len() as u32);
    o.u32(doc.media_lists.len() as u32);
    for list in &doc.media_lists {
        o.u8(u8::from(list.is_used()));
    }
    o.i32(match doc.ids_selectable.get() {
        None => -1,
        Some(seen) => i32::from(seen),
    });
    o.u8(u8::from(doc.has_backdrop.get())).i32(doc.generated_boxes).u64(doc.generated_bytes as u64).u8(u8::from(doc.generated_limited));
}

/// One step of an `l` record.
#[derive(Clone, Debug)]
pub enum Step {
    Dump,
    Media(f32, f32),
    Hover(u32),
    Active(u32),
    Leave(u32),
    Refresh,
    /// A layout at the width within a budget (`None` the renderer's).
    Layout(f32, Option<u64>),
    /// A layout at the width within the renderer's budget, of the render type (`render_type`: 0 all, 1 all but the
    /// fixed boxes' placement, 2 the fixed boxes only).
    LayoutAs(f32, u8),
    /// A paint (stage L13, `Q` records).
    Paint(Box<super::paint::PaintStep>),
}

/// An `l` record's input.
pub fn cascade_record(width: f32, height: f32, dpr: f32, master: &[u8], user: &[u8], html: &[u8], steps: &[Step]) -> Vec<u8> {
    let mut o = Out::default();
    o.f32(width).f32(height).f32(dpr).str(master).str(user).str(html).u32(steps.len() as u32);
    for step in steps {
        match step {
            Step::Dump => {
                o.u8(b'd');
            }
            Step::Media(w, h) => {
                o.u8(b'm').f32(*w).f32(*h);
            }
            Step::Hover(k) => {
                o.u8(b'h').u32(*k);
            }
            Step::Active(k) => {
                o.u8(b'a').u32(*k);
            }
            Step::Leave(k) => {
                o.u8(b'x').u32(*k);
            }
            Step::Refresh => {
                o.u8(b'r');
            }
            Step::Layout(w, budget) => {
                o.u8(b'w').f32(*w);
                match budget {
                    Some(boxes) => o.u8(1).u64(*boxes),
                    None => o.u8(0),
                };
            }
            Step::LayoutAs(w, rt) => {
                o.u8(b'w').f32(*w).u8(*rt << 1);
            }
            Step::Paint(step) => {
                o.u8(b'p');
                step.write(&mut o);
            }
        }
    }
    o.take()
}

/// What the cascade reached, for the report.
#[derive(Default)]
pub struct CascadeTally {
    pub documents: usize,
    pub elements: usize,
    pub dumps: usize,
    pub properties: BTreeMap<Vec<u8>, usize>,
    pub keywords: BTreeMap<&'static str, usize>,
    pub counts: BTreeMap<&'static str, usize>,
    pub displays: std::collections::BTreeSet<i32>,
    pub positions: std::collections::BTreeSet<i32>,
}

impl CascadeTally {
    fn note(&mut self, names: &Names, doc: &Document) {
        use twidget_litehtml::style::{CssWideKeyword, Value};
        self.dumps += 1;
        let mut nodes = Vec::new();
        if let Some(root) = doc.root {
            cascade_walk(doc, root, -1, &mut nodes);
        }
        self.elements += nodes.len();
        let mut first_with_id = BTreeMap::new();
        for &(id, _) in &nodes {
            if doc.kind(id).is_html_tag() {
                first_with_id.entry(doc.el(id).css.m_shared_id).or_insert(id);
            }
        }
        for &(id, _) in &nodes {
            let el = doc.el(id);
            for (&name, p) in el.style.properties() {
                *self.properties.entry(names.s(name).to_vec()).or_default() += 1;
                let key = match p.value {
                    Value::Inherit => "inherit",
                    Value::CssWideKeyword(CssWideKeyword::initial) => "initial",
                    Value::CssWideKeyword(CssWideKeyword::unset) => "unset",
                    Value::CssWideKeyword(CssWideKeyword::revert) => "revert",
                    _ => "",
                };
                if !key.is_empty() {
                    *self.keywords.entry(key).or_default() += 1;
                }
                if p.m_important {
                    *self.counts.entry("!important declarations").or_default() += 1;
                }
            }
            let c = &el.css;
            let mut count = |what: &'static str, yes: bool| {
                if yes {
                    *self.counts.entry(what).or_default() += 1;
                }
            };
            count("::before and ::after boxes", matches!(el.kind, ElementKind::el_before | ElementKind::el_after));
            count("unused selectors kept", el.used_styles.iter().any(|u| !u.used));
            count("styles with a base", el.has_style_base);
            count("counters", !el.counter_values.is_empty());
            count("transforms", !c.m_transform.is_empty());
            count("filters", !c.m_filter.is_empty() || !c.m_backdrop_filter.is_empty());
            count("box shadows", !c.m_box_shadows.is_empty());
            count("text shadows", !c.m_text_shadows.is_empty());
            count("clip paths", !c.m_clip_path.empty());
            count("grids", !c.m_grid_template_columns.empty() || c.m_grid_column.start != 0);
            count("flex items", c.m_flex_grow != 0.0 || c.m_flex_shrink != 1.0);
            count("calc lengths", c.m_css_width.calc().is_some() || c.m_css_margins.left.calc().is_some());
            count("gradients", c.m_bg.m_image.iter().any(|i| i.ty == twidget_litehtml::gradient::ImageType::type_gradient));
            count("monospace keyword sizes", c.m_generic_mono);
            count("rtl", c.m_direction == Direction::direction_rtl.index());
            count("transformed texts", el.text_state.use_transformed);
            count("controls drawn natively", el.look.native);
            count("collapsible spaces", doc.is_white_space(id));
            count("line breaks", doc.is_break(id));
            count("styles taken from another element", first_with_id.get(&c.m_shared_id).is_some_and(|&first| first != id));
            self.displays.insert(c.m_display);
            self.positions.insert(c.m_el_position);
        }
        self.counts.entry("fonts").or_insert(0);
        *self.counts.get_mut("fonts").expect("fonts") += doc.fonts.borrow().len();
        *self.counts.entry("shared computed styles").or_default() += doc.share_cache.len();
        *self.counts.entry("media lists").or_default() += doc.media_lists.len();
    }

    pub fn report(&self) -> String {
        let keywords: Vec<String> = self.keywords.iter().map(|(k, n)| format!("{k} {n}")).collect();
        let counts: Vec<String> = self.counts.iter().map(|(k, n)| format!("{k} {n}")).collect();
        format!(
            "{} documents, {} dumps, {} elements, {} properties declared, {} of 20 displays and {} of 5 positions computed; keywords: {}; {}",
            self.documents,
            self.dumps,
            self.elements,
            self.properties.len(),
            self.displays.len(),
            self.positions.len(),
            keywords.join(", "),
            counts.join(", ")
        )
    }
}

/// What lhdump answers to an `l` record, with the process's name table and declaration cache.
pub fn cascade_answer(lh: &Lh, switches: Switches, input: &[u8], tally: &mut CascadeTally) -> (u8, Vec<u8>) {
    record_answer(lh, switches, input, &mut |o: &mut Out, cx: &StyleCx, doc: &Document| {
        cascade_out(o, cx.names, doc);
        tally.note(cx.names, doc);
    })
}

/// What lhdump answers to an `l` record's input, each dump step written by `dump`.
pub fn record_answer(lh: &Lh, switches: Switches, input: &[u8], dump: &mut dyn FnMut(&mut Out, &StyleCx, &Document)) -> (u8, Vec<u8>) {
    let names = &lh.names;
    let mut i = In::new(input);
    let (width, height, dpr) = (i.f32(), i.f32(), i.f32());
    let names_before = names.usage().names;
    let container = CascadeContainer::new(width, height, dpr);
    let (master, user, html) = (i.str(), i.str(), i.str());
    let cx = StyleCx { names, container: &container, cache: &lh.cache, switches };
    let (mut doc, root) = Document::build(&html, &cx.env());
    doc.finalize_from_external_root(&cx, root, &master, &user);
    doc.build_template();
    let mut o = Out::default();
    o.u8(doc.mode as u8);
    let mut phases = vec![container.calls.borrow().len()];
    let steps = i.u32();
    let mut s = 0;
    // The grid passes a dump writes are those of the layouts right before it, as lhdump's probe keeps them.
    let mut last_op = 0;
    while s < steps && i.at < i.s.len() {
        if s > 0 {
            phases.push(container.calls.borrow().len());
        }
        s += 1;
        let op = i.u8();
        if last_op != b'w' {
            doc.layout.grid_trace = Some(Vec::new());
        }
        last_op = op;
        match op {
            b'd' => dump(&mut o, &cx, &doc),
            b'm' => {
                container.width.set(i.f32());
                container.height.set(i.f32());
                let changed = doc.root.is_some() && doc.media_changed(&cx);
                o.u8(u8::from(changed));
            }
            b'r' => {
                if let Some(root) = doc.root {
                    doc.refresh_styles(&cx, root);
                    doc.compute_styles(&cx, root, true);
                }
                o.u8(1);
            }
            b'w' => {
                let width = i.f32();
                let how = i.u8();
                let budget = if how & 1 != 0 { i.u64() } else { renderer_layout_budget(&doc) };
                doc.set_layout_budget(budget);
                let rt = match how >> 1 {
                    0 => RenderType::render_all,
                    1 => RenderType::render_no_fixed,
                    _ => RenderType::render_fixed_only,
                };
                let ret = doc.layout_document_as(&cx, Px(width), rt);
                o.f32(ret.value());
            }
            b'p' => super::paint::paint_step(&mut i, &mut o, &cx, &mut doc, &container.paint),
            _ => {
                let k = i.u32();
                let mut nodes = Vec::new();
                if let Some(root) = doc.root {
                    cascade_walk(&doc, root, -1, &mut nodes);
                }
                let tags: Vec<ElementId> = nodes.iter().map(|n| n.0).filter(|&id| doc.kind(id).is_html_tag()).collect();
                if tags.is_empty() {
                    o.u8(0);
                    continue;
                }
                let el = tags[k as usize % tags.len()];
                let changed = match op {
                    b'h' => doc.on_mouse_over(el),
                    b'a' => doc.on_lbutton_down(el),
                    _ => doc.on_mouse_leave(el),
                };
                let root = doc.root.expect("a root");
                let styles = doc.find_styles_changes(&cx, root);
                o.u8(u8::from(changed) | u8::from(styles) << 1);
            }
        }
    }
    if s > 0 {
        phases.push(container.calls.borrow().len());
    }
    let names_after = names.usage().names;
    mark(&o, "names the case interned", -2);
    o.u32((names_after - names_before) as u32);
    for k in names_before..names_after {
        o.str(&names.s(StringId(k as u32)));
    }
    let calls = container.calls.borrow();
    let mut all = Out::default();
    all.str(&o.take());
    all.u32(phases.len() as u32);
    for p in 0..phases.len() {
        let from = if p == 0 { 0 } else { phases[p - 1] };
        all.u32((phases[p] - from) as u32);
        for (kind, a, b, n) in &calls[from..phases[p]] {
            all.u8(*kind).str(a).str(b).u64(*n);
        }
    }
    (1, all.take())
}

/// The renderer's layout budget for a document: 100000 boxes and 50 more per element of the DOM.
pub fn renderer_layout_budget(doc: &Document) -> u64 {
    let mut elements = 0u64;
    let mut pending: Vec<ElementId> = doc.root.into_iter().collect();
    while let Some(el) = pending.pop() {
        elements += 1;
        pending.extend(doc.children(el).iter().copied());
    }
    100000 + 50 * elements
}

/// A container call: its kind, its texts and its number.
pub type Call = (u8, Vec<u8>, Vec<u8>, u64);

/// An answer's body and its calls by phase.
pub fn split_answer(answer: &[u8]) -> (Vec<u8>, Vec<Vec<Call>>) {
    let mut i = In::new(answer);
    let body = i.str();
    let mut phases = Vec::new();
    for _ in 0..i.u32() {
        let n = i.u32();
        phases.push((0..n).map(|_| (i.u8(), i.str(), i.str(), i.u64())).collect());
    }
    (body, phases)
}

/// The container's calls of the two sides alike: `None` when they are, else where they part.
pub fn compare_calls(cpp: &[u8], rust: &[u8]) -> Option<String> {
    let (_, c) = split_answer(cpp);
    let (_, r) = split_answer(rust);
    if c.len() != r.len() {
        return Some(format!("{} phases of calls in the C++, {} in the port", c.len(), r.len()));
    }
    let shown = |calls: &[Call]| -> Vec<String> {
        calls
            .iter()
            .take(4)
            .map(|(k, a, b, n)| format!("{} {:?} {:?} {n}", *k as char, String::from_utf8_lossy(a), String::from_utf8_lossy(b)))
            .collect()
    };
    for (p, (cp, rp)) in c.iter().zip(&r).enumerate() {
        if cp != rp {
            let at = cp.iter().zip(rp).position(|(a, b)| a != b).unwrap_or(cp.len().min(rp.len()));
            return Some(format!(
                "calls of phase {p} part at {at} ({} in the C++, {} in the port): C++ {:?} / Rust {:?}",
                cp.len(),
                rp.len(),
                shown(&cp[at.min(cp.len())..]),
                shown(&rp[at.min(rp.len())..])
            ));
        }
    }
    None
}

// ---- Inputs the cascade's and the render tree's comparisons share ------------------------------------------

/// The renderer's master sheet for a theme: litehtml's, the renderer's UA rules, the profile's layer with the
/// theme's values in place of its `var()`s and without `:where()`; and the theme's variables.
pub fn themed_master(dark: bool) -> (Vec<u8>, twidget_css::Variables) {
    let mut theme = twidget_json::Json::object();
    theme.set("colorScheme", if dark { "dark" } else { "light" });
    let vars = twidget_css::theme_variables(&theme);
    let layer = twidget_css::unwrap_where(&twidget_css::substitute_variables(&ua_stylesheet(), &vars));
    ([master_css(), ua_extra(), layer].concat(), vars)
}

/// A serialised document as the renderer gives it to litehtml in a theme: its sheets' colour schemes and
/// `light-dark()` resolved, the theme's variables on the root.
pub fn themed_html(html: &[u8], dark: bool, vars: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(html.len() + 2048);
    let mut i = 0;
    let mut root_done = false;
    while i < html.len() {
        if !root_done && html[i..].starts_with(b"<html") {
            out.extend_from_slice(b"<html style=\"");
            for (name, value) in vars {
                out.extend_from_slice(name);
                out.push(b':');
                out.extend(value.iter().map(|&c| if c == b'"' { b'\'' } else { c }));
                out.push(b';');
            }
            out.push(b'"');
            i += 5;
            root_done = true;
            continue;
        }
        if html[i..].starts_with(b"<style")
            && let Some(open) = html[i..].iter().position(|c| *c == b'>')
            && let Some(close) = html[i + open..].windows(8).position(|w| w == b"</style>")
        {
            let start = i + open + 1;
            let end = i + open + close;
            out.extend_from_slice(&html[i..start]);
            let css = twidget_css::resolve_light_dark(&twidget_css::rewrite_color_scheme(&html[start..end], dark), dark, 0);
            out.extend_from_slice(&css);
            i = end;
            continue;
        }
        out.push(html[i]);
        i += 1;
    }
    out
}

/// Every tag the fork makes a class of, and the common others, with the attributes they read.
pub fn every_tag_document(quirks: bool) -> Vec<u8> {
    let mut out = String::new();
    if !quirks {
        out += "<!DOCTYPE html>";
    }
    out += "<html><head><title>t</title><base href=\"http://x/\"><style>p{margin:1em}</style></head><body>";
    let tags = [
        "div",
        "p",
        "span",
        "a href=\"#\"",
        "b",
        "strong",
        "i",
        "em",
        "u",
        "s",
        "small",
        "big",
        "sub",
        "sup",
        "mark",
        "code",
        "kbd",
        "samp",
        "pre",
        "blockquote",
        "q",
        "cite",
        "dfn",
        "var",
        "abbr title=\"x\"",
        "address",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "ul",
        "ol",
        "li",
        "dl",
        "dt",
        "dd",
        "menu",
        "table border=\"1\" cellpadding=\"3\" cellspacing=\"2\" bgcolor=\"#eee\" width=\"50%\"",
        "caption",
        "thead",
        "tbody",
        "tfoot",
        "tr",
        "th",
        "td bgcolor=\"red\" width=\"10\" height=\"3\" valign=\"top\" align=\"center\" nowrap",
        "form",
        "fieldset",
        "legend",
        "label",
        "button",
        "select",
        "option",
        "optgroup label=\"g\"",
        "textarea placeholder=\"hint\"",
        "meter value=\"0.3\"",
        "progress value=\"5\" max=\"10\"",
        "details open",
        "summary",
        "dialog",
        "figure",
        "figcaption",
        "article",
        "aside",
        "footer",
        "header",
        "main",
        "nav",
        "section",
        "center",
        "font color=\"red\" size=\"5\" face=\"serif\"",
        "hr",
        "br",
        "img src=\"a.png\" width=\"20\" height=\"10\"",
        "canvas width=\"30\" height=\"40\"",
        "svg width=\"20\" height=\"10\" fill=\"red\"",
        "x-custom",
        "ruby",
        "rt",
        "bdi",
        "bdo dir=\"rtl\"",
        "wbr",
        "noscript",
        "time",
        "output",
        "data value=\"1\"",
        "del",
        "ins",
        "s",
        "strike",
        "tt",
        "nobr",
        "marquee",
        "iframe",
        "object",
        "embed",
        "video",
        "audio",
        "picture",
        "map",
        "area",
        "template",
        "slot",
    ];
    for (k, tag) in tags.iter().enumerate() {
        let name = tag.split(' ').next().unwrap_or(tag);
        out += &format!("<{tag} class=\"c{k}\">t{k} <i>x</i></{name}>");
    }
    for ty in [
        "text", "password", "number", "date", "checkbox", "radio", "range", "color", "button", "submit", "reset", "hidden", "email",
        "search",
    ] {
        out += &format!("<input type=\"{ty}\" value=\"v\" placeholder=\"p\" checked disabled>");
        out += &format!("<input type=\"{ty}\">");
    }
    out += "<select disabled><option>one</option><option selected>two</option></select>";
    out += "<input type=\"range\" style=\"appearance:none\"><button style=\"appearance:none;background:red\">b</button>";
    out += "<div dir=\"auto\">\u{5d0}\u{5d1}</div><div dir=\"auto\">abc</div><p dir=\"rtl\">r</p><div hidden>h</div>";
    out += "</body></html>";
    out.into_bytes()
}
