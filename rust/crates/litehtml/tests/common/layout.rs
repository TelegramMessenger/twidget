//! The Rust side of lhdump's `Y` record (stages L7 and L8, layout): an `l` record whose steps may lay the document out
//! (`w` f32 width, u8 budget: 0 the renderer's, 1 a u64 that follows; answered with the f32 natural width) and whose
//! dump steps dump the layout.
//!
//! A dump: u8 a root item, f32 the document's width and height, u64 boxes laid out of the budget, u8 exhausted, u8
//! positioned boxes, u64 the layout epoch, u32 relayout roots left, u8 rendered once; u32 items, numbered as the render
//! tree's dump numbers them (`render`), each: u8 class, i32 element, i32 parent, u32 children and each i32, its place
//! (4 f32), margins, padding, borders (12 f32), u8 skip; its cache (u8 dirty, u8 relayout pending, u8 out of flow,
//! u64 epoch, the last layout (`entry_out`), its place, f32 x, y, its margins, padding, borders, the two measures, i32
//! the next measure, i32 resolved-layout safety); u32 positioned items and each i32; u8 a scroll view and its
//! viewport, f32 size, left, top; an inline's or a text's boxes (u32, 4 f32 each); a block context's f32 vertical-align
//! shift; a text's u8 piece, its text, i32 cut, u32 pieces (each place, margins, padding, borders, u8 skip, u8 piece,
//! text, i32 cut, i32 parent, u32 pieces, boxes); an inline context's f32 trailing, f32 natural and min width, f32
//! vertical-align shift, u8 cut, u32 ellipses (4 f32), u32 cut texts (an item ref), u32 lines, each f32 top, left,
//! right, height, width, baseline, i32 text-align, u8 rtl, f32 natural and min width, f32 line height, u8 its CSS
//! value a keyword, the font metrics, u32 items, each u8 type, u8 wrapped, an item ref, f32 min width, items' top and
//! bottom, a marker's own place, f32 left, right, top, bottom, width, height, min width, natural width. An item ref is
//! i32 the item and i32 -1, or for a text's piece i32 the text and i32 which piece. Then u32 DOM elements, each f32 line
//! height, i32 first render item, a text's u8 breaker kept; u32 anonymous elements, each f32 line height and i32 first
//! render item.

use std::collections::BTreeMap;

use super::cascade::*;
use super::*;
use twidget_litehtml::css_values::*;
use twidget_litehtml::document::{Document, Switches};
use twidget_litehtml::line_box::LbiType;
use twidget_litehtml::render_item::{LayoutEntry, RenderId, RenderKind};
use twidget_litehtml::types::{ContainingBlockContext, Margins, Position, TypedPixel};

/// What the layouts reached, for the report.
#[derive(Default)]
pub struct LayoutTally {
    pub documents: usize,
    pub dumps: usize,
    pub items: usize,
    pub lines: usize,
    pub skipped: BTreeMap<&'static str, usize>,
    pub counts: BTreeMap<String, usize>,
}

impl LayoutTally {
    fn count(&mut self, what: &str, n: usize) {
        if n > 0 {
            *self.counts.entry(what.to_string()).or_default() += n;
        }
    }

    /// Another case's, but its documents.
    pub fn merge(&mut self, other: LayoutTally) {
        self.dumps += other.dumps;
        self.items += other.items;
        self.lines += other.lines;
        for (k, n) in other.counts {
            *self.counts.entry(k).or_default() += n;
        }
    }

    pub fn report(&self) -> String {
        let counts: Vec<String> = self.counts.iter().map(|(k, n)| format!("{k} {n}")).collect();
        let skipped: Vec<String> = self.skipped.iter().map(|(k, n)| format!("{k} {n}")).collect();
        format!(
            "{} documents, {} dumps, {} items, {} lines; not compared (a later stage's layout): {}; {}",
            self.documents,
            self.dumps,
            self.items,
            self.lines,
            if skipped.is_empty() { "none".to_string() } else { skipped.join(", ") },
            counts.join(", ")
        )
    }

    /// The branches the layout shows it took.
    fn note(&mut self, doc: &Document, items: &[RenderId]) {
        self.dumps += 1;
        self.items += items.len();
        let r = &doc.render;
        let mut c: BTreeMap<String, usize> = BTreeMap::new();
        let mut add = |what: &str, yes: bool| {
            if yes {
                *c.entry(what.to_string()).or_default() += 1;
            }
        };
        for &id in items {
            let it = r.item(id);
            let css = &doc.el(it.element).css;
            add("floats left", css.m_float == ElementFloat::float_left.index());
            add("floats right", css.m_float == ElementFloat::float_right.index());
            add("boxes clearing", css.m_clear != ElementClear::clear_none.index());
            add("relatively positioned", css.m_el_position == ElementPosition::element_position_relative.index());
            add("negative margins", it.margins.top < Px(0.0) || it.margins.bottom < Px(0.0) || it.margins.left < Px(0.0));
            add("auto side margins", css.m_css_margins.left.is_predefined() && it.margins.left > Px(0.0));
            add("percentage widths", css.m_css_width.units() == CssUnits::css_units_percentage);
            add("aspect ratios", css.m_aspect_ratio > 0.0);
            add("scroll views", it.scroll_view.is_some());
            add("cached layouts", it.layout.last_layout.valid);
            add("images", it.kind == RenderKind::render_item_image);
            add("items laid out at height 0", it.pos.height == Px(0.0) && it.kind != RenderKind::render_text);
            // Margins that collapsed show as a parent's margin taken from its first child or a last child's zeroed.
            if it.kind == RenderKind::render_item_block_context {
                if let Some(&first) = it.children.first() {
                    let f = r.item(first);
                    add("first child's top margin collapsed into the parent", f.margins.top > Px(0.0) && it.margins.top == f.margins.top);
                }
                if let Some(&last) = it.children.last() {
                    let l = r.item(last);
                    let css_bottom = &doc.el(l.element).css.m_css_margins.bottom;
                    add("last child's bottom margin collapsed through", l.margins.bottom == Px(0.0) && css_bottom.val() > 0.0);
                }
                for pair in it.children.windows(2) {
                    let (a, b) = (r.item(pair[0]), r.item(pair[1]));
                    add("sibling margins collapsed", a.margins.bottom > Px(0.0) && b.margins.top > Px(0.0) && b.top() < a.bottom());
                }
            }
            if let Some(st) = it.inline.as_ref() {
                self.lines += st.line_boxes.len();
                add("inline contexts of several lines", st.line_boxes.len() > 1);
                add("ellipses", !st.ellipses.is_empty());
                add("texts cut into pieces", !st.fragmented_texts.is_empty());
                let ws = css.m_white_space;
                let ws_name = ["normal", "nowrap", "pre", "pre-line", "pre-wrap"].get(ws as usize).copied().unwrap_or("?");
                add(&format!("white-space {ws_name}"), !st.line_boxes.is_empty());
                for line in &st.line_boxes {
                    add("right-to-left lines", line.rtl);
                    let align = ["left", "right", "center", "justify", "start", "end", "th-center"]
                        .get(line.text_align as usize)
                        .copied()
                        .unwrap_or("?");
                    add(&format!("text-align {align}"), true);
                    for lbi in &line.items {
                        add("wrapped text items", lbi.wrapped);
                        add("inline starts", lbi.ty == LbiType::type_inline_start);
                        add("inline continuations", lbi.ty == LbiType::type_inline_continue);
                        let lcss = &doc.el(r.item(lbi.element).element).css;
                        if lcss.m_vertical_align != 0 {
                            add(&format!("vertical-align {}", lcss.m_vertical_align), true);
                        }
                        add("breaks", doc.is_break(r.item(lbi.element).element));
                        add("inline boxes on lines", doc.is_inline_box(r.item(lbi.element).element));
                    }
                }
            }
            if let Some(t) = it.text.as_ref() {
                add("text pieces", !t.fragments.is_empty());
                add("texts cut by an ellipsis", t.cut >= 0);
                add("bidi texts", twidget_litehtml::css_properties::strong_direction(&doc.el(it.element).text) > 0);
            }
        }
        add("layouts exhausted", doc.layout.exhausted);
        for (k, n) in c {
            self.count(&k, n);
        }
        self.count("cache hits", doc.layout.hits as usize);
    }
}

fn pos_out(o: &mut Out, p: &Position) {
    o.f32(p.x.value()).f32(p.y.value()).f32(p.width.value()).f32(p.height.value());
}

fn sides_out(o: &mut Out, m: &Margins) {
    o.f32(m.left.value()).f32(m.right.value()).f32(m.top.value()).f32(m.bottom.value());
}

fn typed_out(o: &mut Out, t: &TypedPixel) {
    o.f32(t.value.value()).u8(t.ty as u8);
}

fn cb_out(o: &mut Out, cb: &ContainingBlockContext) {
    for t in [&cb.width, &cb.render_width, &cb.min_width, &cb.max_width, &cb.height, &cb.min_height, &cb.max_height] {
        typed_out(o, t);
    }
    o.i32(cb.context_idx).u32(cb.size_mode);
}

fn entry_out(o: &mut Out, e: &LayoutEntry) {
    o.u8(u8::from(e.valid)).u8(u8::from(e.second_pass));
    cb_out(o, &e.cb);
    o.f32(e.ret.natural_width.value()).f32(e.ret.min_width.value()).f32(e.outer_height.value());
}

/// lhdump's `TreeIndex` as the render tree's dump numbers items.
struct Index {
    items: Vec<RenderId>,
    item_index: BTreeMap<RenderId, i32>,
    dom: BTreeMap<ElementId, i32>,
    anon: Vec<ElementId>,
    anon_index: BTreeMap<ElementId, i32>,
    pieces: BTreeMap<RenderId, (i32, i32)>,
}

impl Index {
    fn visit(&mut self, doc: &Document, item: RenderId) {
        let mut pending = vec![item];
        while let Some(cur) = pending.pop() {
            if self.item_index.contains_key(&cur) {
                continue;
            }
            self.item_index.insert(cur, self.items.len() as i32);
            self.items.push(cur);
            pending.extend(doc.render.children(cur).iter().rev());
        }
    }

    fn of(&self, doc: &Document, item: Option<RenderId>) -> i32 {
        match item.filter(|&i| doc.render.alive(i)) {
            None => -1,
            Some(i) => self.item_index.get(&i).copied().unwrap_or(-3),
        }
    }

    fn of_el(&mut self, el: Option<ElementId>) -> i32 {
        let Some(el) = el else { return -1 };
        if let Some(&k) = self.dom.get(&el) {
            return k;
        }
        if let Some(&k) = self.anon_index.get(&el) {
            return k;
        }
        let k = -2 - self.anon.len() as i32;
        self.anon_index.insert(el, k);
        self.anon.push(el);
        k
    }

    fn item_ref(&self, o: &mut Out, doc: &Document, item: RenderId) {
        match self.pieces.get(&item) {
            Some(&(text, k)) => o.i32(text).i32(k),
            None => o.i32(self.of(doc, Some(item))).i32(-1),
        };
    }
}

fn inline_boxes_out(o: &mut Out, boxes: &[Position]) {
    o.u32(boxes.len() as u32);
    for b in boxes {
        pos_out(o, b);
    }
}

/// lhdump's `write_layout`.
pub fn layout_out(o: &mut Out, doc: &Document, tally: &mut LayoutTally) {
    let mut nodes = Vec::new();
    if let Some(root) = doc.root {
        cascade_walk(doc, root, -1, &mut nodes);
    }
    let mut t = Index {
        items: Vec::new(),
        item_index: BTreeMap::new(),
        dom: nodes.iter().enumerate().map(|(k, &(id, _))| (id, k as i32)).collect(),
        anon: Vec::new(),
        anon_index: BTreeMap::new(),
        pieces: BTreeMap::new(),
    };
    let r = &doc.render;
    if let Some(root) = r.root {
        t.visit(doc, root);
    }
    for &item in &r.tabular {
        t.visit(doc, item);
    }
    for k in 0..t.items.len() {
        if let Some(text) = r.item(t.items[k]).text.as_ref() {
            for (f, &piece) in text.fragments.iter().enumerate() {
                t.pieces.insert(piece, (k as i32, f as i32));
            }
        }
    }
    let l = &doc.layout;
    mark(o, "document's layout", -2);
    o.u8(u8::from(r.root.is_some())).f32(l.size.width.value()).f32(l.size.height.value()).u64(l.spent).u8(u8::from(l.exhausted));
    o.u8(u8::from(l.has_positioned)).u64(l.epoch).u32(l.relayout_roots.len() as u32).u8(u8::from(l.after_render));
    o.u32(t.items.len() as u32);
    for k in 0..t.items.len() {
        let id = t.items[k];
        let it = r.item(id);
        mark(o, "item", k as i32);
        o.u8(it.kind as u8);
        let el = t.of_el(Some(it.element));
        o.i32(el).i32(t.of(doc, r.parent(id)));
        o.u32(it.children.len() as u32);
        for &child in &it.children {
            o.i32(t.of(doc, Some(child)));
        }
        mark(o, "item's place and boxes", -1);
        pos_out(o, &it.pos);
        for m in [&it.margins, &it.padding, &it.borders] {
            sides_out(o, m);
        }
        o.u8(u8::from(it.skip));
        mark(o, "item's cache", -1);
        let c = &it.layout;
        o.u8(u8::from(c.layout_dirty)).u8(u8::from(c.relayout_pending)).u8(u8::from(c.out_of_flow)).u64(c.layout_epoch);
        entry_out(o, &c.last_layout);
        pos_out(o, &c.last_pos);
        o.f32(c.last_x.value()).f32(c.last_y.value());
        for m in [&c.last_margins, &c.last_padding, &c.last_borders] {
            sides_out(o, m);
        }
        for e in &c.measured {
            entry_out(o, e);
        }
        o.i32(c.measured_next).i32(c.resolved_layout_safe);
        mark(o, "item's positioned and scroll view", -1);
        o.u32(it.positioned.len() as u32);
        for &p in &it.positioned {
            o.i32(t.of(doc, Some(p)));
        }
        match &it.scroll_view {
            Some(view) => {
                o.u8(1);
                pos_out(o, &view.viewport);
                o.f32(view.scroll_size.width.value()).f32(view.scroll_size.height.value()).f32(view.left.value()).f32(view.top.value());
            }
            None => {
                o.u8(0);
            }
        }
        if matches!(it.kind, RenderKind::render_item_inline | RenderKind::render_text) {
            mark(o, "inline's boxes", -1);
            inline_boxes_out(o, &it.inline_boxes);
        }
        if it.kind == RenderKind::render_item_block_context {
            o.f32(it.va_applied.value());
        }
        if let Some(text) = it.text.as_ref() {
            mark(o, "text's pieces", -1);
            o.u8(u8::from(text.is_fragment)).str(&text.fragment_text).i32(text.cut).u32(text.fragments.len() as u32);
            for &f in &text.fragments {
                let fi = r.item(f);
                pos_out(o, &fi.pos);
                for m in [&fi.margins, &fi.padding, &fi.borders] {
                    sides_out(o, m);
                }
                let ft = fi.text.as_ref().expect("a text");
                o.u8(u8::from(fi.skip)).u8(u8::from(ft.is_fragment)).str(&ft.fragment_text).i32(ft.cut).i32(t.of(doc, r.parent(f)));
                o.u32(ft.fragments.len() as u32);
                inline_boxes_out(o, &fi.inline_boxes);
            }
        }
        if let Some(st) = it.inline.as_ref() {
            mark(o, "inline context", -1);
            o.f32(st.trailing.value()).f32(st.rendered_width.natural_width.value()).f32(st.rendered_width.min_width.value());
            o.f32(st.va_applied.value()).u8(u8::from(st.cut));
            o.u32(st.ellipses.len() as u32);
            for e in &st.ellipses {
                pos_out(o, e);
            }
            o.u32(st.fragmented_texts.len() as u32);
            for &text in &st.fragmented_texts {
                t.item_ref(o, doc, text);
            }
            o.u32(st.line_boxes.len() as u32);
            for (n, line) in st.line_boxes.iter().enumerate() {
                mark(o, "line", n as i32);
                for v in [line.top, line.left, line.right, line.height, line.width, line.baseline] {
                    o.f32(v.value());
                }
                o.i32(line.text_align).u8(u8::from(line.rtl));
                o.f32(line.rendered_width.natural_width.value()).f32(line.rendered_width.min_width.value());
                o.f32(line.default_line_height.computed_value.value()).u8(u8::from(line.default_line_height.css_value.is_predefined()));
                metrics_out(o, &line.font_metrics);
                o.u32(line.items.len() as u32);
                for lbi in &line.items {
                    o.u8(lbi.ty as u8).u8(u8::from(lbi.wrapped));
                    t.item_ref(o, doc, lbi.element);
                    o.f32(lbi.rendered_min_width.value()).f32(lbi.items_top.value()).f32(lbi.items_bottom.value());
                    if lbi.ty != LbiType::type_text_part {
                        pos_out(o, &lbi.pos);
                    }
                    for v in [
                        doc.lbi_left(lbi),
                        doc.lbi_right(lbi),
                        doc.lbi_top(lbi),
                        doc.lbi_bottom(lbi),
                        doc.lbi_width(lbi),
                        doc.lbi_height(lbi),
                        doc.lbi_rendered_min_width(lbi),
                        doc.lbi_rendered_natural_width(lbi),
                    ] {
                        o.f32(v.value());
                    }
                }
            }
        }
    }
    mark(o, "DOM elements", -2);
    o.u32(nodes.len() as u32);
    for (index, &(id, _)) in nodes.iter().enumerate() {
        mark(o, "DOM element", index as i32);
        let el = doc.el(id);
        o.f32(el.css.m_line_height.computed_value.value()).i32(t.of(doc, doc.get_render_item(id)));
        if doc.is_text(id) {
            o.u8(u8::from(el.text_state.breaker != 0));
        }
    }
    mark(o, "anonymous elements", -2);
    o.u32(t.anon.len() as u32);
    for k in 0..t.anon.len() {
        let id = t.anon[k];
        o.f32(doc.el(id).css.m_line_height.computed_value.value()).i32(t.of(doc, doc.get_render_item(id)));
    }
    tally.note(doc, &t.items);
}

/// What lhdump answers to a `Y` record, and the first layout a later stage ports when the port reached one.
pub fn layout_answer(lh: &Lh, switches: Switches, input: &[u8], tally: &mut LayoutTally) -> ((u8, Vec<u8>), Option<&'static str>) {
    let mut unported = None;
    let answer = record_answer(lh, switches, input, &mut |o: &mut Out, _names: &Names, doc: &Document| {
        if unported.is_none() {
            unported = doc.layout.unported;
        }
        layout_out(o, doc, tally);
    });
    (answer, unported)
}

/// A `Y` record.
pub fn layout_record(width: f32, height: f32, dpr: f32, master: &[u8], user: &[u8], html: &[u8], steps: &[Step]) -> Vec<u8> {
    cascade_record(width, height, dpr, master, user, html, steps)
}
