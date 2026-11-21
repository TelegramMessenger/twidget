//! The comparisons of `twidget_document`'s engine with renderer.cpp's container (`oracle/renderer_oracle`): every
//! script through the C++ renderer, whose tape holds every answer its document and container gave, then through the
//! port twice. First on [`Checked`], which asks both the tape and the port's [`DocumentEngine`] every question,
//! compares the answers (floats by their bits, display lists in wire form, documents through a correspondence of
//! their identities) and hands the renderer the C++'s, so that one difference stays one; then end to end on the
//! port's engine alone, transcript for transcript with the C++ renderer's (the documents' identities in the keys
//! are the C++'s addresses, read from the tape, the only thing taken from it).

#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Write as _;
use std::rc::Rc;
use std::sync::Arc;

use crate::common::cases::report;
use crate::common::tape::{self, Entry, Shared, TapeEngine};
use crate::common::{FontStub, Run, Script, Tally, run_on, split};
use twidget_animations::{Declared, StyleValues};
use twidget_document::{DocumentEngine, Process};
use twidget_events::{EditorStyle, TextRun};
use twidget_json::Json;
use twidget_renderer::{BoxStyle, Clip, Engine, HoleChange, ScrollExtent, TextContent, Tile};
use twidget_testkit::{Oracle, hex};
use twidget_wire::{Caps, DisplayList, Radii, Rect, encode_list};

/// Scripts compared in runs of the oracle of at most this many (one C++ process, one [`Process`] per run).
pub const CHUNK: usize = 48;

pub fn bits(r: &Rect) -> [u32; 4] {
    [r.x.to_bits(), r.y.to_bits(), r.w.to_bits(), r.h.to_bits()]
}

fn radii_bits(k: &Radii) -> [u32; 8] {
    [k.tlx, k.tly, k.trx, k.try_, k.brx, k.bry, k.blx, k.bly].map(f32::to_bits)
}

/// A display list in wire form, and whether the wire takes it.
pub fn wire(list: &DisplayList) -> (bool, Vec<u8>) {
    let mut bytes = Vec::new();
    let ok = encode_list(list, &Caps::default(), &mut bytes);
    (ok, bytes)
}

fn list_difference(a: &DisplayList, b: &DisplayList) -> String {
    if a.len() != b.len() {
        let at = a.iter().zip(b).position(|(x, y)| wire(&vec![x.clone()]) != wire(&vec![y.clone()])).unwrap_or(a.len().min(b.len()));
        return format!("{} commands in C++, {} in Rust; first differing {at}: {:?} / {:?}", a.len(), b.len(), a.get(at), b.get(at));
    }
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        if wire(&vec![x.clone()]) != wire(&vec![y.clone()]) {
            return format!("command {i} of {}: C++ {x:?} / Rust {y:?}", a.len());
        }
    }
    "the same commands, shared differently".to_string()
}

fn runs_alike(a: &[TextRun], b: &[TextRun]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            x.owner == y.owner
                && x.offset == y.offset
                && x.text == y.text
                && bits(&x.rect) == bits(&y.rect)
                && x.font == y.font
                && x.block == y.block
                && x.selectable == y.selectable
                && x.line_break == y.line_break
        })
}

fn editor_alike(a: &Option<EditorStyle>, b: &Option<EditorStyle>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => {
            bits(&x.frame) == bits(&y.frame)
                && x.font_family == y.font_family
                && x.font_size.to_bits() == y.font_size.to_bits()
                && x.font_weight == y.font_weight
                && x.italic == y.italic
                && x.color == y.color
                && x.align == y.align
                && x.font == y.font
                && x.has_clip == y.has_clip
                && bits(&x.clip) == bits(&y.clip)
        }
        _ => false,
    }
}

fn opt4(a: &Option<[f32; 4]>) -> Option<[u32; 4]> {
    a.map(|v| v.map(f32::to_bits))
}

/// What the checks found: answers compared per method, those that differ (with the first few), and the
/// correspondence of the documents' identities.
#[derive(Debug, Default)]
pub struct Checks {
    pub tally: Tally,
    pub differences: Vec<String>,
    pub ids: BTreeMap<u64, u64>,
    pub what: String,
}

impl Checks {
    fn same_doc(&mut self, mine: u64, theirs: u64) -> bool {
        if mine == 0 || theirs == 0 {
            return mine == theirs;
        }
        *self.ids.entry(mine).or_insert(theirs) == theirs
    }
}

/// The tape's engine and the port's side by side.
pub struct Checked {
    pub tape: TapeEngine,
    pub port: DocumentEngine<FontStub>,
    pub checks: Rc<RefCell<Checks>>,
}

impl std::fmt::Debug for Checked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Checked").finish_non_exhaustive()
    }
}

impl Checked {
    fn step(&self) -> usize {
        self.tape.tape.borrow().step
    }

    fn check(&self, method: &str, alike: bool, what: impl FnOnce() -> String) {
        let mut c = self.checks.borrow_mut();
        c.tally.add(format!("compared {method}"));
        if alike {
            return;
        }
        c.tally.add(format!("DIFFER {method}"));
        if c.differences.len() < 6 {
            let line = format!("{}: step {}: {method}: {}", c.what, self.step(), what());
            c.differences.push(line);
        }
    }
}

macro_rules! same {
    ($self:ident, $name:literal, $call:ident ( $($arg:expr),* )) => {{
        let a = $self.tape.$call($($arg),*);
        let b = $self.port.$call($($arg),*);
        $self.check($name, a == b, || format!("C++ {a:?} / Rust {b:?}"));
        a
    }};
}

macro_rules! same_rect {
    ($self:ident, $name:literal, $call:ident ( $($arg:expr),* )) => {{
        let a = $self.tape.$call($($arg),*);
        let b = $self.port.$call($($arg),*);
        $self.check($name, bits(&a) == bits(&b), || format!("C++ {a:?} / Rust {b:?}"));
        a
    }};
}

impl Engine for Checked {
    fn viewport(&mut self, width: f64, dpr: f64) {
        self.port.viewport(width, dpr);
    }
    fn viewport_height(&mut self, vp_height: i32) {
        self.port.viewport_height(vp_height);
    }
    fn canvases(&mut self, held: &[(i32, i32, i32)]) {
        self.port.canvases(held);
    }
    fn has_viewport_units(&mut self, html: &[u8]) -> bool {
        same!(self, "has_viewport_units", has_viewport_units(html))
    }
    fn update_from_string(&mut self, html: &[u8]) -> Option<bool> {
        same!(self, "update_from_string", update_from_string(html))
    }
    fn media_changed(&mut self) {
        self.tape.media_changed();
        self.port.media_changed();
    }
    fn last_patch_paint_only(&mut self) -> bool {
        same!(self, "last_patch_paint_only", last_patch_paint_only())
    }
    fn create(&mut self, html: &[u8], master: &[u8]) -> bool {
        same!(self, "create", create(html, master))
    }
    fn drop_document(&mut self) {
        self.tape.drop_document();
        self.port.drop_document();
    }
    fn element_count(&mut self) -> u64 {
        same!(self, "element_count", element_count())
    }
    fn render(&mut self, width: f32, budget: u64) {
        self.tape.render(width, budget);
        self.port.render(width, budget);
    }
    fn layout_exhausted(&mut self) -> bool {
        same!(self, "layout_exhausted", layout_exhausted())
    }
    fn layout_spent(&mut self) -> u64 {
        same!(self, "layout_spent", layout_spent())
    }
    fn index(&mut self) {
        self.tape.index();
        self.port.index();
    }
    fn height(&mut self) -> f64 {
        let a = self.tape.height();
        let b = self.port.height();
        self.check("height", a.to_bits() == b.to_bits(), || format!("C++ {a} / Rust {b}"));
        a
    }
    fn has_root(&mut self) -> bool {
        same!(self, "has_root", has_root())
    }
    fn layout_reads_viewport_height(&mut self) -> bool {
        same!(self, "layout_reads_viewport_height", layout_reads_viewport_height())
    }
    fn font_count(&mut self) -> usize {
        let n = same!(self, "font_count", font_count());
        if n > 4096 {
            self.checks.borrow_mut().tally.add("fonts past 4096");
        }
        n
    }
    fn update_holes(&mut self, changes: &[HoleChange]) -> Option<bool> {
        same!(self, "update_holes", update_holes(changes))
    }
    fn update_styles(&mut self, patches: &[(i64, Vec<u8>)]) -> Option<bool> {
        same!(self, "update_styles", update_styles(patches))
    }
    fn name_usage(&mut self) -> (u64, u64) {
        same!(self, "name_usage", name_usage())
    }
    fn trim(&mut self) {
        self.tape.trim();
        self.port.trim();
    }
    fn set_no_effects(&mut self) {
        self.tape.set_no_effects();
        self.port.set_no_effects();
    }
    fn take_violations(&mut self) -> Vec<Vec<u8>> {
        let a = self.tape.take_violations();
        let b = self.port.take_violations();
        let show = |v: &[Vec<u8>]| v.iter().map(|s| String::from_utf8_lossy(s).into_owned()).collect::<Vec<_>>();
        if !a.is_empty() || !b.is_empty() {
            self.check("take_violations", a == b, || format!("C++ {:?} / Rust {:?}", show(&a), show(&b)));
        }
        a
    }
    fn has_element(&mut self, id: i64) -> bool {
        same!(self, "has_element", has_element(id))
    }
    fn declarations(&mut self, names: &[Vec<u8>], gates: &[Vec<u8>], always: &[i64]) -> (BTreeMap<i64, Declared>, u64) {
        let a = self.tape.declarations(names, gates, always);
        let b = self.port.declarations(names, gates, always);
        // The renderer asks for the scan's generation where it scans the gates; elsewhere the probe writes 0.
        let generation = a.1 == b.1 || (!always.is_empty() && a.1 == 0);
        let show = |m: &BTreeMap<i64, Declared>| {
            m.iter()
                .map(|(id, d)| {
                    let d: Vec<String> =
                        d.iter().map(|(k, v)| format!("{}={}", String::from_utf8_lossy(k), String::from_utf8_lossy(v))).collect();
                    format!("{id}: {}", d.join("; "))
                })
                .collect::<Vec<_>>()
        };
        self.check("declarations", a.0 == b.0 && generation, || format!("C++ {:?} {} / Rust {:?} {}", show(&a.0), a.1, show(&b.0), b.1));
        a
    }
    fn edges(&mut self) -> Vec<(i64, Rect)> {
        let a = self.tape.edges();
        let b = self.port.edges();
        let alike = a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| x.0 == y.0 && bits(&x.1) == bits(&y.1));
        self.check("edges", alike, || format!("C++ {a:?} / Rust {b:?}"));
        a
    }
    fn visual_box(&mut self, id: i64) -> Rect {
        same_rect!(self, "visual_box", visual_box(id))
    }
    fn inline_boxes(&mut self, id: i64) -> Vec<Rect> {
        let a = self.tape.inline_boxes(id);
        let b = self.port.inline_boxes(id);
        let alike = a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| bits(x) == bits(y));
        self.check("inline_boxes", alike, || format!("C++ {a:?} / Rust {b:?}"));
        a
    }
    fn transformed_box(&mut self, id: i64, r: Rect) -> Rect {
        same_rect!(self, "transformed_box", transformed_box(id, r))
    }
    fn border_box(&mut self, id: i64) -> Rect {
        same_rect!(self, "border_box", border_box(id))
    }
    fn borders(&mut self, id: i64) -> Option<[f32; 4]> {
        let a = self.tape.borders(id);
        let b = self.port.borders(id);
        self.check("borders", opt4(&a) == opt4(&b), || format!("C++ {a:?} / Rust {b:?}"));
        a
    }
    fn offsets(&mut self, id: i64) -> (f64, f64, i64) {
        let a = self.tape.offsets(id);
        let b = self.port.offsets(id);
        let alike = a.0.to_bits() == b.0.to_bits() && a.1.to_bits() == b.1.to_bits() && a.2 == b.2;
        self.check("offsets", alike, || format!("C++ {a:?} / Rust {b:?}"));
        a
    }
    fn scroll_extent(&mut self, id: i64) -> ScrollExtent {
        let a = self.tape.scroll_extent(id);
        let b = self.port.scroll_extent(id);
        let f = |e: &ScrollExtent| (e.scroller, [e.pad_right, e.pad_bottom, e.right, e.bottom].map(f32::to_bits));
        self.check("scroll_extent", f(&a) == f(&b), || format!("C++ {a:?} / Rust {b:?}"));
        a
    }
    fn textarea_content(&mut self, id: i64) -> Option<TextContent> {
        let a = self.tape.textarea_content(id);
        let b = self.port.textarea_content(id);
        let f = |t: &Option<TextContent>| t.map(|t| (t.width.to_bits(), t.height.to_bits(), t.pad.map(f32::to_bits)));
        self.check("textarea_content", f(&a) == f(&b), || format!("C++ {a:?} / Rust {b:?}"));
        a
    }
    fn computed_style(&mut self, id: i64, pseudo: &[u8], animated: &StyleValues) -> Json {
        let a = self.tape.computed_style(id, pseudo, animated);
        let b = self.port.computed_style(id, pseudo, animated);
        let (da, db) = (a.dump(), b.dump());
        self.check("computed_style", da == db, || {
            let at = da.iter().zip(&db).position(|(x, y)| x != y).unwrap_or(da.len().min(db.len()));
            let near = |s: &[u8]| String::from_utf8_lossy(&s[at.saturating_sub(40)..(at + 60).min(s.len())]).into_owned();
            format!("C++ …{} / Rust …{}", near(&da), near(&db))
        });
        a
    }
    fn custom_property(&mut self, id: i64, name: &[u8]) -> Vec<u8> {
        same!(self, "custom_property", custom_property(id, name))
    }
    fn svg_text_box(&mut self, id: i64) -> Option<[f32; 4]> {
        let a = self.tape.svg_text_box(id);
        let b = self.port.svg_text_box(id);
        self.check("svg_text_box", opt4(&a) == opt4(&b), || format!("C++ {a:?} / Rust {b:?}"));
        a
    }
    fn svg_geometry(&mut self, id: i64) -> bool {
        same!(self, "svg_geometry", svg_geometry(id))
    }
    fn svg_path_length(&mut self, id: i64, at: f32) -> (f32, f32, f32) {
        let a = self.tape.svg_path_length(id, at);
        let b = self.port.svg_path_length(id, at);
        let f = |v: (f32, f32, f32)| [v.0, v.1, v.2].map(f32::to_bits);
        self.check("svg_path_length", f(a) == f(b), || format!("C++ {a:?} / Rust {b:?}"));
        a
    }
    fn svg_path_bounds(&mut self, id: i64) -> Option<[f32; 4]> {
        let a = self.tape.svg_path_bounds(id);
        let b = self.port.svg_path_bounds(id);
        self.check("svg_path_bounds", opt4(&a) == opt4(&b), || format!("C++ {a:?} / Rust {b:?}"));
        a
    }
    fn hit(&mut self, x: f64, y: f64, mut placement: Option<&mut Rect>) -> i64 {
        let mut mine = placement.as_deref().copied();
        let a = self.tape.hit(x, y, placement.as_deref_mut());
        let b = self.port.hit(x, y, mine.as_mut());
        let theirs = placement.as_deref().copied();
        let alike = a == b && theirs.map(|r| bits(&r)) == mine.map(|r| bits(&r));
        self.check("hit", alike, || format!("C++ {a} {theirs:?} / Rust {b} {mine:?}"));
        a
    }
    fn box_of(&mut self, id: i64) -> Rect {
        same_rect!(self, "box_of", box_of(id))
    }
    fn touch_action(&mut self, id: i64) -> Option<Vec<u8>> {
        same!(self, "touch_action", touch_action(id))
    }
    fn user_select_text(&mut self, id: i64) -> bool {
        same!(self, "user_select_text", user_select_text(id))
    }
    fn mouse(&mut self, action: i32, x: f64, y: f64, hovers: bool) -> bool {
        same!(self, "mouse", mouse(action, x, y, hovers))
    }
    fn leave(&mut self) -> bool {
        same!(self, "leave", leave())
    }
    fn cursor(&mut self) -> Vec<u8> {
        same!(self, "cursor", cursor())
    }
    fn editor_style(&mut self, id: i64, clip: Rect) -> Option<EditorStyle> {
        let a = self.tape.editor_style(id, clip);
        let b = self.port.editor_style(id, clip);
        self.check("editor_style", editor_alike(&a, &b), || format!("C++ {a:?} / Rust {b:?}"));
        a
    }
    fn image_of(&mut self, src: &[u8]) -> Option<(i32, f32, f32)> {
        let a = self.tape.image_of(src);
        let b = self.port.image_of(src);
        let f = |v: Option<(i32, f32, f32)>| v.map(|(i, w, h)| (i, w.to_bits(), h.to_bits()));
        self.check("image_of", f(a) == f(b), || format!("C++ {a:?} / Rust {b:?}"));
        a
    }
    fn image_ready(&mut self, id: i32, w: f32, h: f32) -> bool {
        same!(self, "image_ready", image_ready(id, w, h))
    }
    fn content_size(&mut self, id: i64) -> (f32, f32) {
        let a = self.tape.content_size(id);
        let b = self.port.content_size(id);
        self.check("content_size", (a.0.to_bits(), a.1.to_bits()) == (b.0.to_bits(), b.1.to_bits()), || format!("C++ {a:?} / Rust {b:?}"));
        a
    }
    fn runs_key(&mut self) -> [u64; 4] {
        let a = self.tape.runs_key();
        let b = self.port.runs_key();
        let doc = self.checks.borrow_mut().same_doc(b[0], a[0]);
        self.check("runs_key", doc && a[1..] == b[1..], || format!("C++ {a:?} / Rust {b:?}"));
        a
    }
    fn text_runs(&mut self) -> Vec<TextRun> {
        let a = self.tape.text_runs();
        let b = self.port.text_runs();
        self.check("text_runs", runs_alike(&a, &b), || {
            let at = a
                .iter()
                .zip(&b)
                .position(|(x, y)| !runs_alike(std::slice::from_ref(x), std::slice::from_ref(y)))
                .unwrap_or(a.len().min(b.len()));
            format!("{} runs in C++, {} in Rust; at {at}: {:?} / {:?}", a.len(), b.len(), a.get(at), b.get(at))
        });
        a
    }
    fn text_width(&mut self, text: &[u8], font: u64) -> f32 {
        let a = self.tape.text_width(text, font);
        let b = self.port.text_width(text, font);
        self.check("text_width", a.to_bits() == b.to_bits(), || format!("C++ {a} / Rust {b}"));
        a
    }
    fn font_info(&mut self, font: u64) -> Option<(i32, f32, f32, f32)> {
        let a = self.tape.font_info(font);
        let b = self.port.font_info(font);
        let f = |v: Option<(i32, f32, f32, f32)>| v.map(|(h, d, l, w)| (h, d.to_bits(), l.to_bits(), w.to_bits()));
        self.check("font_info", f(a) == f(b), || format!("C++ {a:?} / Rust {b:?}"));
        a
    }
    fn selectable_key(&mut self) -> [u64; 2] {
        let a = self.tape.selectable_key();
        let b = self.port.selectable_key();
        let doc = self.checks.borrow_mut().same_doc(b[0], a[0]);
        self.check("selectable_key", doc && a[1] == b[1], || format!("C++ {a:?} / Rust {b:?}"));
        a
    }
    fn any_selectable(&mut self) -> bool {
        same!(self, "any_selectable", any_selectable())
    }
    fn boxes(&mut self) -> Vec<(i64, bool, BoxStyle)> {
        let a = self.tape.boxes();
        let b = self.port.boxes();
        let f = |v: &[(i64, bool, BoxStyle)]| {
            v.iter().map(|(k, p, s)| (*k, *p, s.backdrop, s.filter, s.blurred, s.dimmed, s.opacity.to_bits())).collect::<Vec<_>>()
        };
        self.check("boxes", f(&a) == f(&b), || format!("C++ {a:?} / Rust {b:?}"));
        a
    }
    fn box_style(&mut self, id: i64) -> BoxStyle {
        let a = self.tape.box_style(id);
        let b = self.port.box_style(id);
        let f = |s: &BoxStyle| (s.backdrop, s.filter, s.blurred, s.dimmed, s.opacity.to_bits());
        self.check("box_style", f(&a) == f(&b), || format!("C++ {a:?} / Rust {b:?}"));
        a
    }
    fn background_is_local(&mut self, id: i64) -> bool {
        same!(self, "background_is_local", background_is_local(id))
    }
    fn layer_candidate(&mut self, id: i64, movers: Option<&BTreeSet<i64>>) -> Option<Rect> {
        let a = self.tape.layer_candidate(id, movers);
        let b = self.port.layer_candidate(id, movers);
        self.check("layer_candidate", a.map(|r| bits(&r)) == b.map(|r| bits(&r)), || format!("C++ {a:?} / Rust {b:?}"));
        a
    }
    fn clip_of(&mut self, id: i64) -> Option<Clip> {
        let a = self.tape.clip_of(id);
        let b = self.port.clip_of(id);
        // The tape keeps a clip's rect and radii only when it has one.
        let f = |c: Option<Clip>| c.map(|c| (c.has, if c.has { (bits(&c.rect), radii_bits(&c.radii)) } else { ([0; 4], [0; 8]) }));
        self.check("clip_of", f(a) == f(b), || format!("C++ {a:?} / Rust {b:?}"));
        a
    }
    fn background_tile(&mut self, id: i64) -> Option<Tile> {
        let a = self.tape.background_tile(id);
        let b = self.port.background_tile(id);
        let f =
            |t: &Option<Tile>| t.as_ref().map(|t| (bits(&t.tile), bits(&t.area), bits(&t.clip), radii_bits(&t.radii), wire(&t.content)));
        self.check("background_tile", f(&a) == f(&b), || format!("C++ {a:?} / Rust {b:?}"));
        a
    }
    fn declared(&mut self, id: i64, names: &[&str]) -> Declared {
        same!(self, "declared", declared(id, names))
    }
    fn paint_order(&mut self) {
        self.tape.paint_order();
        self.port.paint_order();
    }
    fn paint_index(&mut self, id: i64) -> usize {
        same!(self, "paint_index", paint_index(id))
    }
    fn covered(&mut self, id: i64, area: Rect, ignore: &BTreeSet<i64>) -> bool {
        same!(self, "covered", covered(id, area, ignore))
    }
    fn obscurers(&mut self, id: i64, area: Rect, ignore: &BTreeSet<i64>, limit: usize) -> Option<Vec<i64>> {
        same!(self, "obscurers", obscurers(id, area, ignore, limit))
    }
    fn draw(&mut self) -> DisplayList {
        let a = self.tape.draw();
        let b = self.port.draw();
        self.check("draw", wire(&a) == wire(&b), || list_difference(&a, &b));
        a
    }
    fn draw_skipping(&mut self, skip: &[i64], backgrounds: &[i64]) -> DisplayList {
        let a = self.tape.draw_skipping(skip, backgrounds);
        let b = self.port.draw_skipping(skip, backgrounds);
        self.check("draw_skipping", wire(&a) == wire(&b), || list_difference(&a, &b));
        a
    }
    fn draw_layer(&mut self, id: i64, skip: &[i64]) -> (DisplayList, Rect) {
        let a = self.tape.draw_layer(id, skip);
        let b = self.port.draw_layer(id, skip);
        let alike = wire(&a.0) == wire(&b.0) && bits(&a.1) == bits(&b.1);
        self.check("draw_layer", alike, || format!("bounds C++ {:?} / Rust {:?}; {}", a.1, b.1, list_difference(&a.0, &b.0)));
        a
    }
    fn paint_key(&mut self) -> (u64, u32) {
        let a = self.tape.paint_key();
        let b = self.port.paint_key();
        let doc = self.checks.borrow_mut().same_doc(b.0, a.0);
        self.check("paint_key", doc && a.1 == b.1, || format!("C++ {a:?} / Rust {b:?}"));
        a
    }
    fn hit_within(&mut self, id: i64, x: f32, y: f32, mut placement: Option<&mut Rect>) -> i64 {
        let mut mine = placement.as_deref().copied();
        let a = self.tape.hit_within(id, x, y, placement.as_deref_mut());
        let b = self.port.hit_within(id, x, y, mine.as_mut());
        let theirs = placement.as_deref().copied();
        let alike = a == b && theirs.map(|r| bits(&r)) == mine.map(|r| bits(&r));
        self.check("hit_within", alike, || format!("C++ {a} {theirs:?} / Rust {b} {mine:?}"));
        a
    }
}

/// The C++ documents' addresses in creation order, as the tape shows them in the keys (0 where no key showed one).
pub fn document_ids(entries: &[Entry]) -> Vec<u64> {
    let mut ids: Vec<u64> = Vec::new();
    let mut current: Option<usize> = None;
    for e in entries {
        let seen = match e {
            Entry::Create { .. } => {
                ids.push(0);
                current = Some(ids.len() - 1);
                None
            }
            Entry::PaintKey(doc, _, _) => Some(*doc),
            Entry::RunsKey(k) => Some(k[0]),
            Entry::SelectableKey(k) => Some(k[0]),
            _ => None,
        };
        if let (Some(p), Some(c)) = (seen, current)
            && p != 0
            && ids[c] == 0
        {
            ids[c] = p;
        }
    }
    ids
}

/// What a comparison of scripts found.
#[derive(Debug, Default)]
pub struct Outcome {
    /// The checked run's tallies and the end-to-end run's.
    pub checked: Tally,
    pub whole: Tally,
    pub differences: Vec<String>,
    /// End to end: scripts alike, scripts that differ.
    pub alike: usize,
    pub differ: usize,
}

fn first_difference(cpp: &[u8], rust: &[u8]) -> String {
    let at = cpp.iter().zip(rust).position(|(a, b)| a != b).unwrap_or(cpp.len().min(rust.len()));
    let near = |b: &[u8]| {
        let piece = &b[at.saturating_sub(24).min(b.len())..(at + 64).min(b.len())];
        if piece.iter().all(|c| (0x20..0x7f).contains(c)) { format!("{:?}", String::from_utf8_lossy(piece)) } else { hex(piece) }
    };
    format!("transcripts {} / {} bytes, first difference at {at}: C++ …{} / Rust …{}", cpp.len(), rust.len(), near(cpp), near(rust))
}

/// Every script through the oracle, then through the port checked against the tape and end to end; panics with the
/// first differences unless all are alike.
pub fn compare_engines(oracle: &Oracle, scripts: &[(Script, String)], name: &str) -> Outcome {
    let mut out = Outcome::default();
    for chunk in scripts.chunks(CHUNK) {
        let input: Vec<u8> = chunk.iter().flat_map(|(s, _)| s.record()).collect();
        let answers = oracle.run(input, chunk.len());
        // One C++ process ran the chunk: one name table and declaration cache for each of the port's runs.
        let checked_process = Arc::new(Process::default());
        let whole_process = Arc::new(Process::default());
        for ((script, what), (status, answer)) in chunk.iter().zip(&answers) {
            let (tape_bytes, cpp) = split(answer);
            let entries = match tape::parse(tape_bytes) {
                Ok(e) => e,
                Err(e) => {
                    out.differences.push(format!("{what}: the tape does not parse: {e}"));
                    out.differ += 1;
                    continue;
                }
            };
            let ids = document_ids(&entries);
            let reused = ids.windows(2).filter(|w| w[0] != 0 && w[0] == w[1]).count();
            out.checked.add_n("documents made where the last one was", reused);
            out.checked.add_n("documents made", ids.len());
            // Checked: both engines asked, the C++'s answers followed.
            let shared = Rc::new(RefCell::new(Shared { entries: VecDeque::from(entries), ..Shared::default() }));
            let checks = Rc::new(RefCell::new(Checks { what: what.clone(), ..Checks::default() }));
            let engine = Checked {
                tape: TapeEngine { tape: shared.clone() },
                port: DocumentEngine::new(checked_process.clone(), FontStub::default()),
                checks: checks.clone(),
            };
            let r = run_on(script, Some(shared), engine, &mut out.checked);
            if *status != 1 || !r.errors.is_empty() || cpp != r.transcript.as_slice() {
                out.differences.push(format!("{what}: the tape replay: {}; {}", r.errors.join("; "), first_difference(cpp, &r.transcript)));
            }
            let c = std::mem::take(&mut *checks.borrow_mut());
            out.checked.merge(c.tally);
            out.differences.extend(c.differences);
            // End to end: the port's engine alone.
            let mut engine = DocumentEngine::new(whole_process.clone(), FontStub::default());
            engine.give_document_ids(ids);
            let r: Run = run_on(script, None, engine, &mut out.whole);
            if cpp == r.transcript.as_slice() {
                out.alike += 1;
            } else {
                out.differ += 1;
                if out.differences.len() < 12 {
                    out.differences.push(format!("{what}: end to end: {}", first_difference(cpp, &r.transcript)));
                }
            }
        }
    }
    let mut s = String::new();
    for (k, v) in &out.checked.0 {
        if k.starts_with("compared ") || k.starts_with("DIFFER ") || k.starts_with("documents made") {
            let _ = write!(s, " {k}={v}");
        }
    }
    eprintln!("{name}: {} scripts; end to end {} alike, {} differ; answers:{s}", scripts.len(), out.alike, out.differ);
    let _ = report;
    let differing: usize = out.checked.0.iter().filter(|(k, _)| k.starts_with("DIFFER ")).map(|(_, v)| *v).sum();
    assert!(
        out.differences.is_empty() && differing == 0 && out.differ == 0,
        "{name}: {differing} answers and {} scripts differ:\n{}",
        out.differ,
        out.differences[..out.differences.len().min(12)].join("\n")
    );
    out
}
