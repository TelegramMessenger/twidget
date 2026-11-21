//! The Rust side of lhdump's `W` record (stage L16, hit testing and the mouse state): a document as
//! `createUpdatableFromString` makes it (as `b`), then pointer steps between patches, layouts and paints.
//!
//! - `W` f32 width, height, dpr, u8 flags (bit 0: the container sanitizes elements), master, html, u32 steps, each:
//!   - `u`, `s`, `H`, `m`, `w`: as `b` (a patch answered as `b` answers it; a layout's f32 and its dirty regions).
//!   - `p` and a paint step: as `Q`.
//!   - `d`: the cascade (`cascade_out`), the layout (`layout_out`), what painting keeps (`paint_out`), then i32 the
//!     hovered and i32 the active element (the walk's numbering: -1 none, -2 out of the tree).
//!   - `g` u8 check (0 none, 1 the renderer's `hittable`: no `pointer-events: none` up the chain, 2 `is_v_scrollable`
//!     and 3 `is_h_scrollable` of the delta, 4 any, 5 the items of even number), f32 delta, u8 from (0 the root item, 1 the k-th element's item
//!     with the point moved as the renderer's `element_at_within` moves it, 2 the k-th item, 3 `get_child_by_point` of
//!     the k-th item in the pass `flag` of the z-index), u32 k, u8 flag, i32 z-index, f32 x, y, client x, client y:
//!     `get_element_by_point`, answered with i32 the element. Each check asked logs `?` (i32 the item; n its answer).
//!   - `o`, `b`, `U` f32 x, y, client x, client y: `on_mouse_over`, `on_lbutton_down`, `on_lbutton_up`; `c`
//!     `on_button_cancel`, `l` `on_mouse_leave`: u8 the answer, u32 redraw boxes and each (4 f32), i32 the hovered and
//!     i32 the active element.
//!   - `S` f32 dx, dy, x, y, client x, client y: `on_scroll`: u8 0 without a render tree (not called: the C++ would
//!     read through a null pointer) or 1, then u32 values, each f32 dx, dy and the box.
//!   - `T` u8 what, u32 k, f32 a, b: on the k-th item (0 `scroll_to(a, b)`, 3 `is_h_scrollable(a)` u8, 4
//!     `is_v_scrollable(a)` u8, 5 `scroll_box` of (a, b, 0, 0) the box, 6 `is_point_inside(a, b)` u8) or element (1
//!     `h_scroll(a)`, 2 `v_scroll(a)`, f32): u8 found, then the answer.
//!   - `v` u32 k, u8 at (0 its placement, 1 the box of the 4 f32 that follow), f32 x, y: the k-th element's
//!     `el_svg::element_at` (i32 the element; -3 not an `<svg>`).
//!
//!   The floats of boxes and scroll values are written as `Q` writes them (every NaN as one). Then the names the case
//!   interned and the container's calls by phase, among them `J` `point_in_clip_path` (the path, f32 x, y; n the
//!   answer: inside the box of its points, its left and top edges in), `M` `on_mouse_event` (i32 element; n the
//!   event), `^` `set_cursor` (the cursor), `@` `on_anchor_click` (the url; i32 element), `!` `on_element_click` (i32
//!   element; n taken: a button's or a label's).

use std::collections::BTreeMap;

use super::cascade::*;
use super::layout::{LayoutTally, layout_out};
use super::paint::{PaintIndex, PaintStep, paint_out, paint_step};
use super::patch::{ElementIndex, patch_answer_out};
use super::svg::Answer;
use super::*;
use twidget_litehtml::document::{Document, StyleCx, Switches};
use twidget_litehtml::element::{ElementId, ElementKind};
use twidget_litehtml::paint::DrawFlag;
use twidget_litehtml::patch::HoleChange;
use twidget_litehtml::render_item::RenderId;
use twidget_litehtml::types::{Position, RenderType};

/// A `g` step: what is asked where.
#[derive(Clone, Copy, Debug)]
pub struct At {
    pub check: u8,
    pub delta: f32,
    pub from: u8,
    pub k: u32,
    pub flag: u8,
    pub zindex: i32,
    pub x: f32,
    pub y: f32,
    pub client_x: f32,
    pub client_y: f32,
}

impl At {
    /// The root's answer at the point (the client point the same), with a check.
    pub fn root(check: u8, x: f32, y: f32) -> At {
        At { check, delta: 0.0, from: 0, k: 0, flag: 0, zindex: 0, x, y, client_x: x, client_y: y }
    }

    /// The renderer's `element_at_within` of the k-th element at the document point.
    pub fn within(k: u32, x: f32, y: f32) -> At {
        At { check: 1, delta: 0.0, from: 1, k, flag: 0, zindex: 0, x, y, client_x: x, client_y: y }
    }
}

/// One step of a `W` record.
#[derive(Clone, Debug)]
pub enum HitStep {
    Html(Vec<u8>),
    Styles(Vec<(u32, Vec<u8>)>),
    Holes(Vec<(u32, bool, Vec<u8>, Vec<u8>)>),
    Media(f32, f32),
    /// A layout at the width within the renderer's budget.
    Layout(f32),
    Paint(Box<PaintStep>),
    Dump,
    At(At),
    /// `on_mouse_over`, `on_lbutton_down`, `on_lbutton_up` at x, y, client x, client y.
    Over([f32; 4]),
    Down([f32; 4]),
    Up([f32; 4]),
    Cancel,
    Leave,
    /// `on_scroll`: dx, dy, x, y, client x, client y.
    Scroll([f32; 6]),
    Item {
        what: u8,
        k: u32,
        a: f32,
        b: f32,
    },
    Svg {
        k: u32,
        at: Option<[f32; 4]>,
        x: f32,
        y: f32,
    },
}

/// A `W` record's input.
pub fn hit_record(width: f32, height: f32, dpr: f32, sanitizing: bool, master: &[u8], html: &[u8], steps: &[HitStep]) -> Vec<u8> {
    let mut o = Out::default();
    o.f32(width).f32(height).f32(dpr).u8(u8::from(sanitizing)).str(master).str(html).u32(steps.len() as u32);
    for step in steps {
        match step {
            HitStep::Html(html) => {
                o.u8(b'u').str(html);
            }
            HitStep::Styles(list) => {
                o.u8(b's').u32(list.len() as u32);
                for (k, style) in list {
                    o.u32(*k).str(style);
                }
            }
            HitStep::Holes(list) => {
                o.u8(b'H').u32(list.len() as u32);
                for (k, text, name, value) in list {
                    o.u32(*k).u8(u8::from(*text)).str(name).str(value);
                }
            }
            HitStep::Media(w, h) => {
                o.u8(b'm').f32(*w).f32(*h);
            }
            HitStep::Layout(w) => {
                o.u8(b'w').f32(*w).u8(0);
            }
            HitStep::Paint(p) => {
                o.u8(b'p');
                p.write(&mut o);
            }
            HitStep::Dump => {
                o.u8(b'd');
            }
            HitStep::At(a) => {
                o.u8(b'g').u8(a.check).f32(a.delta).u8(a.from).u32(a.k).u8(a.flag).i32(a.zindex);
                o.f32(a.x).f32(a.y).f32(a.client_x).f32(a.client_y);
            }
            HitStep::Over(p) | HitStep::Down(p) | HitStep::Up(p) => {
                let op = match step {
                    HitStep::Over(_) => b'o',
                    HitStep::Down(_) => b'b',
                    _ => b'U',
                };
                o.u8(op);
                for v in p {
                    o.f32(*v);
                }
            }
            HitStep::Cancel => {
                o.u8(b'c');
            }
            HitStep::Leave => {
                o.u8(b'l');
            }
            HitStep::Scroll(v) => {
                o.u8(b'S');
                for x in v {
                    o.f32(*x);
                }
            }
            HitStep::Item { what, k, a, b } => {
                o.u8(b'T').u8(*what).u32(*k).f32(*a).f32(*b);
            }
            HitStep::Svg { k, at, x, y } => {
                o.u8(b'v').u32(*k).u8(u8::from(at.is_some()));
                if let Some(b) = at {
                    for v in b {
                        o.f32(*v);
                    }
                }
                o.f32(*x).f32(*y);
            }
        }
    }
    o.take()
}

/// What the pointer steps reached, for the report.
#[derive(Default)]
pub struct HitTally(pub BTreeMap<String, usize>);

impl HitTally {
    pub fn add(&mut self, what: impl Into<String>) {
        self.add_n(what, 1);
    }
    pub fn add_n(&mut self, what: impl Into<String>, n: usize) {
        if n > 0 {
            *self.0.entry(what.into()).or_default() += n;
        }
    }
    pub fn count(&self, what: &str) -> usize {
        self.0.get(what).copied().unwrap_or(0)
    }
    pub fn merge(&mut self, other: HitTally) {
        for (k, v) in other.0 {
            *self.0.entry(k).or_default() += v;
        }
    }
    pub fn report(&self) -> String {
        self.0.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", ")
    }
}

fn raw_pos_out(o: &mut Out, p: &Position) {
    o.f32(p.x.value()).f32(p.y.value()).f32(p.width.value()).f32(p.height.value());
}

fn box_out(o: &mut Out, p: &Position) {
    o.a32(p.x.value()).a32(p.y.value()).a32(p.width.value()).a32(p.height.value());
}

fn hit_state_out(o: &mut Out, doc: &Document) {
    let w = ElementIndex::new(doc);
    mark(o, "hovered and active elements", -2);
    o.i32(w.of(doc.get_over_element())).i32(w.of(doc.get_active_element()));
}

/// The renderer's `hittable`: no `pointer-events: none` up the chain (`computed_pointer_events` of computed_style.cpp:
/// the first raw declaration, trimmed, that is not empty, `inherit` or `unset`; `initial` is `auto`).
pub fn hittable(names: &Names, doc: &Document, el: ElementId) -> bool {
    let mut cur = Some(el);
    while let Some(e) = cur {
        let v = if doc.kind(e).is_html_tag() { twidget_css::trim(&doc.raw_declaration(names, e, b"pointer-events")) } else { Vec::new() };
        if !v.is_empty() && v != b"inherit" && v != b"unset" {
            return (if v == b"initial" { &b"auto"[..] } else { &v[..] }) != b"none";
        }
        cur = doc.parent(e);
    }
    true
}

fn draw_flag(flag: u8) -> DrawFlag {
    match flag % 5 {
        0 => DrawFlag::draw_root,
        1 => DrawFlag::draw_block,
        2 => DrawFlag::draw_floats,
        3 => DrawFlag::draw_inlines,
        _ => DrawFlag::draw_positioned,
    }
}

/// What lhdump answers to a `W` record, with the process's name table and declaration cache; `observe` sees the
/// document after each step with the step's index.
pub fn hit_answer_with(
    lh: &Lh,
    switches: Switches,
    input: &[u8],
    tally: &mut HitTally,
    observe: &mut dyn FnMut(usize, &StyleCx, &Document),
) -> (u8, Vec<u8>) {
    let names = &lh.names;
    let mut i = In::new(input);
    let (width, height, dpr) = (i.f32(), i.f32(), i.f32());
    let names_before = names.usage().names;
    let container = CascadeContainer::new(width, height, dpr);
    let flags = i.u8();
    container.sanitizing.set(flags & 1 != 0);
    let master = i.str();
    let current = i.str();
    let cx = StyleCx { names, container: &container, cache: &lh.cache, switches };
    let mut doc = Document::create_updatable(&current, &master, &cx);
    let mut o = Out::default();
    o.u8(doc.mode as u8);
    let mut phases = vec![container.calls.borrow().len()];
    let steps = i.u32();
    let mut s = 0;
    let mut last_op = 0;
    while s < steps && i.at < i.s.len() {
        if s > 0 {
            phases.push(container.calls.borrow().len());
        }
        let step = s as usize;
        s += 1;
        let op = i.u8();
        mark(&o, "step", step as i32);
        if last_op != b'w' {
            doc.layout.grid_trace = Some(Vec::new());
        }
        last_op = op;
        let w = ElementIndex::new(&doc);
        *container.hit_elements.borrow_mut() = w.index.clone();
        *container.hit_takers.borrow_mut() = (0..doc.elements.len() as u32)
            .map(ElementId)
            .filter(|&el| doc.kind(el).is_html_tag() && matches!(&lowcase(&doc.get_tag_name(names, el))[..], b"button" | b"label"))
            .collect();
        match op {
            b'u' => {
                let html = i.str();
                let mut changed = true;
                let ok = doc.update_from_string(&cx, &html, &mut changed);
                tally.add(format!("update_from_string {}", if ok { "patched" } else { "refused" }));
                patch_answer_out(&mut o, &doc, ok, changed);
            }
            b's' | b'H' => {
                let n = i.u32();
                let mut styles = Vec::new();
                let mut changes = Vec::new();
                let mut k = 0;
                while k < n && i.at < i.s.len() {
                    k += 1;
                    let el = w.pick(i.u32());
                    if op == b's' {
                        styles.push((el, i.str()));
                        continue;
                    }
                    let text = i.u8() != 0;
                    let name = i.str();
                    let value = i.str();
                    changes.push(HoleChange { owner: el, name, text, value });
                }
                let mut changed = true;
                let ok =
                    if op == b's' { doc.update_styles(&cx, &styles, &mut changed) } else { doc.update_holes(&cx, &changes, &mut changed) };
                let kind = if op == b's' { "update_styles" } else { "update_holes" };
                tally.add(format!("{kind} {}", if ok { "patched" } else { "refused" }));
                patch_answer_out(&mut o, &doc, ok, changed);
            }
            b'm' => {
                container.width.set(i.f32());
                container.height.set(i.f32());
                let changed = doc.root.is_some() && doc.media_changed(&cx);
                o.u8(u8::from(changed));
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
                mark(&o, "layout's answer", -2);
                let ret = doc.layout_document_as(&cx, Px(width), rt);
                o.f32(ret.value());
                let mut regions = Vec::new();
                let dirty = doc.layout_dirty_regions(&mut regions);
                o.u8(u8::from(dirty)).u32(regions.len() as u32);
                for r in &regions {
                    raw_pos_out(&mut o, r);
                }
            }
            b'p' => {
                container.hit_elements.borrow_mut().clear();
                paint_step(&mut i, &mut o, &cx, &mut doc, &container.paint);
                tally.add("paints");
            }
            b'd' => {
                mark(&o, "cascade", -2);
                cascade_out(&mut o, names, &doc);
                mark(&o, "layout", -2);
                layout_out(&mut o, &cx, &doc, &mut LayoutTally::default());
                paint_out(&mut o, &cx, &doc);
                hit_state_out(&mut o, &doc);
                tally.add("dumps");
            }
            b'g' => {
                let check = i.u8();
                let delta = Px(i.f32());
                let from = i.u8();
                let k = i.u32();
                let flag = i.u8();
                let zindex = i.i32();
                let (x, y, client_x, client_y) = (Px(i.f32()), Px(i.f32()), Px(i.f32()), Px(i.f32()));
                let index = PaintIndex::new(&doc);
                let checks = std::cell::Cell::new((0usize, 0usize));
                let checker = |doc: &Document, ri: RenderId| -> bool {
                    let ok = match check {
                        1 => hittable(names, doc, doc.render.item(ri).element),
                        2 => doc.is_v_scrollable(ri, delta),
                        3 => doc.is_h_scrollable(ri, delta),
                        5 => index.of(doc, ri) % 2 == 0,
                        _ => true,
                    };
                    container.calls.borrow_mut().push((b'?', Vec::new(), index.of(doc, ri).to_le_bytes().to_vec(), u64::from(ok)));
                    let (asked, refused) = checks.get();
                    checks.set((asked + 1, refused + usize::from(!ok)));
                    ok
                };
                let check_fn: Option<twidget_litehtml::hit::HitCheck> = if check != 0 { Some(&checker) } else { None };
                let hit = match from {
                    0 => doc.render.root.and_then(|root| doc.get_element_by_point(&cx, root, x, y, client_x, client_y, check_fn)),
                    1 => match w.pick(k).and_then(|el| doc.get_render_item(el)) {
                        Some(ri) => {
                            let placement = doc.get_placement(&cx, ri);
                            let sticky = doc.sticky_offset(ri);
                            let pos = doc.render.item(ri).pos;
                            let px = x - (placement.x - pos.x - sticky.x);
                            let py = y - (placement.y - pos.y - sticky.y);
                            doc.get_element_by_point(&cx, ri, px, py, client_x, client_y, check_fn)
                        }
                        None => None,
                    },
                    _ if index.items.is_empty() => None,
                    2 => {
                        let ri = index.items[k as usize % index.items.len()];
                        doc.get_element_by_point(&cx, ri, x, y, client_x, client_y, check_fn)
                    }
                    _ => {
                        let ri = index.items[k as usize % index.items.len()];
                        doc.get_child_by_point(&cx, ri, x, y, client_x, client_y, draw_flag(flag), zindex, check_fn)
                    }
                };
                mark(&o, "the element at the point", -2);
                o.i32(w.of(hit));
                let (asked, refused) = checks.get();
                tally.add_n("checks asked", asked);
                tally.add_n("checks refused", refused);
                tally.add(match (from, hit.is_some()) {
                    (0, true) => "points over an element",
                    (0, false) => "points over nothing",
                    (1, true) => "points within an element over one",
                    (1, false) => "points within an element over nothing",
                    (2, true) => "points from an item over an element",
                    (2, false) => "points from an item over nothing",
                    (_, true) => "children's points over an element",
                    (_, false) => "children's points over nothing",
                });
                if let Some(hit) = hit {
                    tally.add(format!("hits on {:?}", doc.kind(hit)));
                }
            }
            b'o' | b'b' | b'U' | b'c' | b'l' => {
                let mut p = [Px(0.0); 4];
                if matches!(op, b'o' | b'b' | b'U') {
                    for v in &mut p {
                        *v = Px(i.f32());
                    }
                }
                let mut boxes = Vec::new();
                let mut redraw = |b: Position| boxes.push(b);
                let over_before = doc.get_over_element();
                let ret = match op {
                    b'o' => doc.doc_on_mouse_over(&cx, p[0], p[1], p[2], p[3], &mut redraw),
                    b'b' => doc.doc_on_lbutton_down(&cx, p[0], p[1], p[2], p[3], &mut redraw),
                    b'U' => doc.doc_on_lbutton_up(&cx, p[0], p[1], p[2], p[3], &mut redraw),
                    b'c' => doc.doc_on_button_cancel(&cx, &mut redraw),
                    _ => doc.doc_on_mouse_leave(&cx, &mut redraw),
                };
                mark(&o, "the pointer's answer", -2);
                o.u8(u8::from(ret)).u32(boxes.len() as u32);
                for b in &boxes {
                    box_out(&mut o, b);
                }
                hit_state_out(&mut o, &doc);
                let name = match op {
                    b'o' => "on_mouse_over",
                    b'b' => "on_lbutton_down",
                    b'U' => "on_lbutton_up",
                    b'c' => "on_button_cancel",
                    _ => "on_mouse_leave",
                };
                tally.add(format!("{name} {}", if ret { "restyled" } else { "not restyled" }));
                tally.add_n("redraw boxes", boxes.len());
                if over_before != doc.get_over_element() {
                    tally.add("hovered element changed");
                }
            }
            b'S' => {
                let v: Vec<Px> = (0..6).map(|_| Px(i.f32())).collect();
                mark(&o, "the scroll", -2);
                if doc.render.root.is_none() {
                    o.u8(0);
                    observe(step, &cx, &doc);
                    continue;
                }
                let values = doc.doc_on_scroll(&cx, v[0], v[1], v[2], v[3], v[4], v[5]);
                o.u8(1).u32(values.len() as u32);
                for value in &values {
                    o.a32(value.dx.value()).a32(value.dy.value());
                    box_out(&mut o, &value.scroll_box);
                }
                tally.add(format!("scrolls with {} values", values.len()));
                tally.add_n("scrolls that moved", values.iter().filter(|v| v.dx != Px(0.0) || v.dy != Px(0.0)).count());
            }
            b'T' => {
                let what = i.u8();
                let k = i.u32();
                let (a, b) = (Px(i.f32()), Px(i.f32()));
                mark(&o, "the item's answer", -2);
                if what == 1 || what == 2 {
                    let el = w.pick(k);
                    o.u8(u8::from(el.is_some()));
                    if let Some(el) = el {
                        let moved = if what == 1 { doc.element_h_scroll(el, a) } else { doc.element_v_scroll(el, a) };
                        o.a32(moved.value());
                        tally.add_n("element scrolls that moved", usize::from(moved != Px(0.0)));
                    }
                    observe(step, &cx, &doc);
                    continue;
                }
                let index = PaintIndex::new(&doc);
                o.u8(u8::from(!index.items.is_empty()));
                if index.items.is_empty() {
                    observe(step, &cx, &doc);
                    continue;
                }
                let ri = index.items[k as usize % index.items.len()];
                match what {
                    0 => doc.scroll_to(ri, a, b),
                    3 => {
                        let yes = doc.is_h_scrollable(ri, a);
                        o.u8(u8::from(yes));
                        tally.add_n("items h-scrollable", usize::from(yes));
                    }
                    4 => {
                        let yes = doc.is_v_scrollable(ri, a);
                        o.u8(u8::from(yes));
                        tally.add_n("items v-scrollable", usize::from(yes));
                    }
                    5 => {
                        let mut bx = Position::new(a, b, Px(0.0), Px(0.0));
                        doc.scroll_box(ri, &mut bx);
                        box_out(&mut o, &bx);
                    }
                    6 => {
                        let yes = doc.is_point_inside(&cx, ri, a, b);
                        o.u8(u8::from(yes));
                        tally.add(format!("is_point_inside of {:?} {yes}", doc.render.item(ri).kind));
                    }
                    _ => {}
                }
            }
            b'v' => {
                let el = w.pick(i.u32());
                let at = i.u8();
                let mut pos = Position::default();
                if at == 1 {
                    pos = Position::new(Px(i.f32()), Px(i.f32()), Px(i.f32()), Px(i.f32()));
                }
                let (x, y) = (Px(i.f32()), Px(i.f32()));
                mark(&o, "the svg's element at the point", -2);
                match el.filter(|&el| doc.kind(el) == ElementKind::el_svg) {
                    None => {
                        o.i32(-3);
                    }
                    Some(svg) => {
                        if at == 0 {
                            pos = doc.element_placement(&cx, svg);
                        }
                        let hit = doc.el_svg_element_at(&cx, svg, &pos, x, y);
                        o.i32(w.of(hit));
                        tally.add(if hit.is_some() { "svg points over a shape" } else { "svg points over nothing" });
                    }
                }
            }
            other => panic!("no W step {}", char::from(other)),
        }
        observe(step, &cx, &doc);
    }
    if s > 0 {
        phases.push(container.calls.borrow().len());
    }
    container.hit_elements.borrow_mut().clear();
    let names_after = names.usage().names;
    mark(&o, "names the case interned", -2);
    o.u32((names_after - names_before) as u32);
    for k in names_before..names_after {
        o.str(&names.s(StringId(k as u32)));
    }
    let calls = container.calls.borrow();
    for (kind, _, _, n) in calls.iter() {
        match kind {
            b'J' => tally.add(if *n != 0 { "clip path points inside" } else { "clip path points outside" }),
            b'M' => tally.add(if *n == 0 { "mouse events enter" } else { "mouse events leave" }),
            b'^' => tally.add("cursors set"),
            b'@' => tally.add("anchor clicks"),
            b'!' => tally.add(if *n != 0 { "element clicks taken" } else { "element clicks passed on" }),
            _ => {}
        }
    }
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

/// What lhdump answers to a `W` record.
pub fn hit_answer(lh: &Lh, switches: Switches, input: &[u8], tally: &mut HitTally) -> (u8, Vec<u8>) {
    hit_answer_with(lh, switches, input, tally, &mut |_, _, _| {})
}
