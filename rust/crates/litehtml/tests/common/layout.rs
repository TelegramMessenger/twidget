//! The Rust side of lhdump's `Y` record (stages L7–L10, layout): an `l` record whose steps may lay the document out
//! (`w` f32 width, u8 how: bit 0 the renderer's budget or a u64 that follows, the rest the `render_type`; answered with
//! the f32 natural width) and whose dump steps dump the layout.
//!
//! A dump: u8 a root item, f32 the document's width and height, u64 boxes laid out of the budget, u8 exhausted, u8
//! positioned boxes, u64 the layout epoch, u32 relayout roots left, u8 rendered once, u32 fixed boxes' redraw boxes (4
//! f32 each); u32 items, numbered as the render tree's dump numbers them (`render`), each: u8 class, i32 element, i32
//! parent, u32 children and each i32, its place (4 f32), margins, padding, borders (12 f32), u8 skip; its cache (u8
//! dirty, u8 relayout pending, u8 out of flow, u64 epoch, the last layout (`entry_out`), its place, f32 x, y, its
//! margins, padding, borders, the two measures, i32 the next measure, i32 resolved-layout safety); u32 positioned items
//! in z-order and each i32; u8 a scroll view and its viewport, f32 size, left, top; a sticky box's f32 offset (stage
//! L9); its placement (4 f32); an inline's or a text's boxes (u32, 4 f32 each); a block context's f32 vertical-align
//! shift; a text's u8 piece, its text, i32 cut, u32 pieces (each place, margins, padding, borders, u8 skip, u8 piece,
//! text, i32 cut, i32 parent, u32 pieces, boxes); an inline context's f32 trailing, f32 natural and min width, f32
//! vertical-align shift, u8 cut, u32 ellipses (4 f32), u32 cut texts (an item ref), u32 lines, each f32 top, left,
//! right, height, width, baseline, i32 text-align, u8 rtl, f32 natural and min width, f32 line height, u8 its CSS
//! value a keyword, the font metrics, u32 items, each u8 type, u8 wrapped, an item ref, f32 min width, items' top and
//! bottom, a marker's own place, f32 left, right, top, bottom, width, height, min width, natural width. A flex's (stage
//! L10) f32 first and last baselines, u32 lines, each f32 cross start, main size, cross size, base size, i32 auto
//! margins at the main start and end, the first and last baselines (f32, u8 type), u8 main and cross axes reversed, f32
//! main gap, u32 items, each u8 row class, i32 the item, f32 base and min size, f32 max size and u8 its default, f32 main
//! size, i32 grow and shrink, f32 scaled shrink factor, u8 frozen, u8 clamp state, i32 order and source order, the main
//! start and end auto margins (f32, u8 default), u8 cross start and end auto margins, i32 alignment. An item ref is
//! i32 the item and i32 -1, or for a text's piece i32 the text and i32 which piece. Then u32 DOM elements, each f32 line
//! height, i32 first render item, a text's u8 breaker kept; u32 anonymous elements, each f32 line height and i32 first
//! render item.

use std::collections::BTreeMap;

use super::cascade::*;
use super::*;
use twidget_litehtml::css_values::*;
use twidget_litehtml::document::{Document, StyleCx, Switches};
use twidget_litehtml::flex_line::FlexLine;
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
    pub flex_lines: usize,
    pub flex_items: usize,
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
        self.flex_lines += other.flex_lines;
        self.flex_items += other.flex_items;
        for (k, n) in other.counts {
            *self.counts.entry(k).or_default() += n;
        }
    }

    pub fn report(&self) -> String {
        let counts: Vec<String> = self.counts.iter().map(|(k, n)| format!("{k} {n}")).collect();
        let skipped: Vec<String> = self.skipped.iter().map(|(k, n)| format!("{k} {n}")).collect();
        format!(
            "{} documents, {} dumps, {} items, {} lines, {} flex lines of {} items; not compared (a later stage's layout): {}; {}",
            self.documents,
            self.dumps,
            self.items,
            self.lines,
            self.flex_lines,
            self.flex_items,
            if skipped.is_empty() { "none".to_string() } else { skipped.join(", ") },
            counts.join(", ")
        )
    }

    /// The branches a flex container's layout shows it took (stage L10).
    fn flex_note(
        &mut self,
        add: &mut dyn FnMut(&str, bool),
        doc: &Document,
        it: &twidget_litehtml::render_item::RenderItem,
        css: &twidget_litehtml::css_properties::CssProperties,
        lines: &[FlexLine],
    ) {
        use twidget_litehtml::flex_item::FlexClampState;
        use twidget_litehtml::types::BaselineType;
        if lines.is_empty() {
            return;
        }
        self.flex_lines += lines.len();
        let direction = ["row", "row-reverse", "column", "column-reverse"].get(css.m_flex_direction as usize).copied().unwrap_or("?");
        add(&format!("flex-direction {direction}"), true);
        let wrap = ["nowrap", "wrap", "wrap-reverse"].get(css.m_flex_wrap as usize).copied().unwrap_or("?");
        add(&format!("flex-wrap {wrap}"), true);
        add("flex containers of several lines", lines.len() > 1);
        add(&format!("justify-content {}", css.m_flex_justify_content), lines.iter().any(|l| l.items.len() > 1));
        add(&format!("align-content {}", css.m_flex_align_content), lines.len() > 1);
        add("inline flex containers", css.m_display == StyleDisplay::display_inline_flex.index());
        add("flex main gaps", lines[0].main_gap != Px(0.0));
        add("flex lines with first baselines", lines.iter().any(|l| l.first_baseline.ty != BaselineType::baseline_type_none));
        add("flex lines with last baselines", lines.iter().any(|l| l.last_baseline.ty != BaselineType::baseline_type_none));
        add("flex lines with main auto margins", lines.iter().any(|l| l.num_auto_margin_main_start + l.num_auto_margin_main_end > 0));
        add("flex containers measured", it.layout.measured.iter().any(|m| m.valid));
        add("fixed flex boxes looked up for their resolved input", it.layout.resolved_layout_safe >= 0);
        let mut sorted = false;
        let mut previous = -1;
        for line in lines {
            for item in &line.items {
                sorted |= item.src_order < previous;
                previous = item.src_order;
                self.flex_items += 1;
                add("flex items grown", item.main_size > item.base_size && item.grow > 0);
                add("flex items shrunk", item.main_size < item.base_size && item.shrink > 0);
                add("flex items frozen inflexible", item.clamp_state == FlexClampState::flex_clamp_state_inflexible);
                add("flex items clamped by their minimum", item.clamp_state == FlexClampState::flex_clamp_state_min_violation);
                add("flex items clamped by their maximum", item.clamp_state == FlexClampState::flex_clamp_state_max_violation);
                add("flex items with a maximum", !item.max_size.is_default());
                add("flex items with fractional factors", item.grow % 1000 != 0 || item.shrink % 1000 != 0);
                add("flex items with cross auto margins", item.auto_margin_cross_start || item.auto_margin_cross_end);
                add("flex items with main auto margins given room", item.auto_margin_main_start.value > Px(0.0));
                add(&format!("align-self {:#x}", item.align), true);
                let item_css = &doc.el(doc.render.item(item.el).element).css;
                let basis = &item_css.m_flex_basis;
                let basis_kind = if basis.is_predefined() {
                    ["auto", "content", "fit-content", "min-content", "max-content"].get(basis.predef() as usize).copied().unwrap_or("?")
                } else if basis.units() == CssUnits::css_units_percentage {
                    "%"
                } else if basis.val() < 0.0 {
                    "negative"
                } else {
                    "length"
                };
                add(&format!("flex-basis {basis_kind}"), true);
            }
        }
        add("flex items out of source order", sorted);
    }

    /// The branches positioned layout shows it took (stage L9): the rule of CSS 2.2 10.6.4 and 10.3.7 each absolute or
    /// fixed box was placed by, what holds it, the order of the positioned lists, sticky shifts.
    fn positioned_note(&mut self, add: &mut dyn FnMut(&str, bool), doc: &Document, id: RenderId, order: &BTreeMap<RenderId, usize>) {
        let r = &doc.render;
        let it = r.item(id);
        let css = &doc.el(it.element).css;
        let position = css.m_el_position;
        let absolute = position == ElementPosition::element_position_absolute.index();
        let fixed = position == ElementPosition::element_position_fixed.index();
        if (absolute || fixed) && css.m_display != StyleDisplay::display_none.index() {
            add(if absolute { "absolute boxes" } else { "fixed boxes" }, true);
            let o = &css.m_css_offsets;
            let rule = |a: bool, b: bool, size: bool| match (a, b, size) {
                (true, false, true) => 1,
                (true, true, false) => 2,
                (false, true, true) => 3,
                (true, false, false) => 4,
                (false, false, true) => 5,
                (false, true, false) => 6,
                (true, true, true) => 7,
                _ => 8,
            };
            let v = rule(o.top.is_predefined(), o.bottom.is_predefined(), css.m_css_height.is_predefined());
            let v_name = [
                "",
                "top and height auto",
                "top and bottom auto",
                "height and bottom auto",
                "top auto",
                "height auto",
                "bottom auto",
                "all auto",
                "none auto",
            ][v];
            add(&format!("vertical rule {v} ({v_name})"), true);
            let h = rule(o.left.is_predefined(), o.right.is_predefined(), css.m_css_width.is_predefined());
            let h_name = [
                "",
                "left and width auto",
                "left and right auto",
                "width and right auto",
                "left auto",
                "width auto",
                "right auto",
                "all auto",
                "none auto",
            ][h];
            add(&format!("horizontal rule {h} ({h_name})"), true);
            add("absolute replaced boxes", doc.is_replaced(it.element));
            let flex = css.m_display == StyleDisplay::display_flex.index() || css.m_display == StyleDisplay::display_inline_flex.index();
            add("absolute flex boxes", flex && (css.m_css_width.is_predefined() || css.m_css_height.is_predefined()));
            let m = &css.m_css_margins;
            add(
                "auto margins solved",
                (v == 8 && (m.top.is_predefined() || m.bottom.is_predefined()))
                    || (h == 8 && (m.left.is_predefined() || m.right.is_predefined())),
            );
            add("percentage offsets", [&o.left, &o.right, &o.top, &o.bottom].iter().any(|l| l.units() == CssUnits::css_units_percentage));
        }
        if position == ElementPosition::element_position_sticky.index() {
            let shift = doc.sticky_offset(id);
            add("sticky boxes", true);
            add("sticky boxes moved", shift.x != Px(0.0) || shift.y != Px(0.0));
        }
        if !it.positioned.is_empty() {
            let element = it.element;
            let root = r.parent(id).is_none();
            let static_holder = doc.el(element).css.m_el_position == ElementPosition::element_position_static.index();
            add("positioned held by static boxes", static_holder && !root);
            let indexes: Vec<usize> = it.positioned.iter().map(|p| order.get(p).copied().unwrap_or(usize::MAX)).collect();
            add("positioned lists out of tree order", indexes.windows(2).any(|w| w[0] > w[1]));
            let zs: std::collections::BTreeSet<i32> = it.positioned.iter().map(|&p| doc.el(r.item(p).element).css.get_z_index()).collect();
            add("positioned lists of several z-indexes", zs.len() > 1);
            let viewport = root && !doc.is_positioned(element);
            for &p in &it.positioned {
                let pc = &doc.el(r.item(p).element).css;
                let placed = pc.m_el_position == ElementPosition::element_position_absolute.index()
                    && pc.m_display != StyleDisplay::display_none.index();
                add("boxes placed against the viewport", placed && viewport);
            }
        }
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
        let order: BTreeMap<RenderId, usize> = items.iter().enumerate().map(|(k, &id)| (id, k)).collect();
        for &id in items {
            let it = r.item(id);
            let css = &doc.el(it.element).css;
            self.positioned_note(&mut add, doc, id, &order);
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
            if let Some(flex) = it.flex.as_ref() {
                self.flex_note(&mut add, doc, it, css, &flex.lines);
            }
            if let Some(t) = it.text.as_ref() {
                add("text pieces", !t.fragments.is_empty());
                add("texts cut by an ellipsis", t.cut >= 0);
                add("bidi texts", twidget_litehtml::css_properties::strong_direction(&doc.el(it.element).text) > 0);
            }
        }
        add("layouts exhausted", doc.layout.exhausted);
        if !doc.layout.fixed_boxes.is_empty() {
            add("fixed boxes' redraw boxes", true);
        }
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

/// lhdump's `write_flex`.
fn flex_out(o: &mut Out, doc: &Document, t: &Index, id: RenderId) {
    mark(o, "flex's baselines", -1);
    o.f32(doc.get_first_baseline(id).value()).f32(doc.get_last_baseline(id).value());
    let lines = doc.render.item(id).flex.as_ref().map_or(&[][..], |f| &f.lines[..]);
    o.u32(lines.len() as u32);
    for (n, line) in lines.iter().enumerate() {
        mark(o, "flex line", n as i32);
        for v in [line.cross_start, line.main_size, line.cross_size, line.base_size] {
            o.f32(v.value());
        }
        o.i32(line.num_auto_margin_main_start).i32(line.num_auto_margin_main_end);
        for b in [&line.first_baseline, &line.last_baseline] {
            o.f32(b.value.value()).u8(b.ty as u8);
        }
        o.u8(u8::from(line.reverse_main)).u8(u8::from(line.reverse_cross)).f32(line.main_gap.value());
        o.u32(line.items.len() as u32);
        for (k, item) in line.items.iter().enumerate() {
            mark(o, "flex item", k as i32);
            o.u8(u8::from(item.row)).i32(t.of(doc, Some(item.el)));
            o.f32(item.base_size.value()).f32(item.min_size.value());
            o.f32(item.max_size.value.value()).u8(u8::from(item.max_size.is_default()));
            o.f32(item.main_size.value()).i32(item.grow).i32(item.shrink).f32(item.scaled_flex_shrink_factor.value());
            o.u8(u8::from(item.frozen)).u8(item.clamp_state as u8).i32(item.order).i32(item.src_order);
            for m in [&item.auto_margin_main_start, &item.auto_margin_main_end] {
                o.f32(m.value.value()).u8(u8::from(m.is_default()));
            }
            o.u8(u8::from(item.auto_margin_cross_start)).u8(u8::from(item.auto_margin_cross_end)).i32(item.align);
        }
    }
}

/// lhdump's `write_layout`.
pub fn layout_out(o: &mut Out, cx: &StyleCx, doc: &Document, tally: &mut LayoutTally) {
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
    let fixed_boxes = doc.get_fixed_boxes();
    o.u32(fixed_boxes.len() as u32);
    for b in &fixed_boxes {
        pos_out(o, b);
    }
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
        if doc.el(it.element).css.m_el_position == ElementPosition::element_position_sticky.index() {
            mark(o, "sticky offset", -1);
            let sticky = doc.sticky_offset(id);
            o.f32(sticky.x.value()).f32(sticky.y.value());
        }
        mark(o, "item's placement", -1);
        pos_out(o, &doc.get_placement(cx, id));
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
        if it.kind == RenderKind::render_item_flex {
            flex_out(o, doc, &t, id);
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
    let answer = record_answer(lh, switches, input, &mut |o: &mut Out, cx: &StyleCx, doc: &Document| {
        if unported.is_none() {
            unported = doc.layout.unported;
        }
        layout_out(o, cx, doc, tally);
    });
    (answer, unported)
}

/// A `Y` record.
pub fn layout_record(width: f32, height: f32, dpr: f32, master: &[u8], user: &[u8], html: &[u8], steps: &[Step]) -> Vec<u8> {
    cascade_record(width, height, dpr, master, user, html, steps)
}
