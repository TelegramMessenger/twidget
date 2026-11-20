//! The Rust side of lhdump's `o` record with `r` (stage L6, the render tree): an `l` record whose dump steps dump
//! the document's render tree instead of the cascade.
//!
//! A dump numbers the items the document keeps: the tree from the root in preorder (an item met again keeps its
//! number), then each tabular item the tree does not reach with the items under it; elements are numbered by the
//! DOM's walk from the root, the anonymous ones the tree made -2, -3, … by first use (-1 none). Then:
//!
//! - u32 items, each u8 class (`RenderKind`), i32 element, i32 parent (its weak pointer: -1 none or gone, -3 alive
//!   but not numbered), u32 children and each i32, the constructor's margins, padding and borders (12 f32: left,
//!   right, top, bottom each), u8 skip; a `render_item_table`'s f32 spacing x and y, u8 grid, and the grid: i32 rows
//!   and columns, each row (i32 item, f32 height, border top and bottom, top, bottom, min height, its CSS height),
//!   each cell row by row (u8 there, i32 item, i32 colspan and rowspan, six f32 sizes, four f32 borders), each column
//!   (seven f32, its CSS width), u32 captions and each i32, f32 captions' heights;
//! - i32 the root, u32 tabular items and each i32, i32 table cells taken, u8 the table limit met, u64 the tree's
//!   serial;
//! - u32 DOM elements, each u8 has `list_index` (and its value), its computed width (`len_out`), its render items
//!   (u32 count, each i32: -1 gone, -3 alive but not numbered) and i32 `get_render_item`;
//! - each anonymous element by its number after a u8 1, then a u8 0: u8 class, i32 parent, tag, its computed style
//!   (`css_out` with its share id as it is), for a tag u32 matched selectors, style, u8 has base, base style, i32
//!   counter changes, u8 anonymous, u32 attributes; its counters (name, i32), its render items as above.

use std::collections::BTreeMap;

use super::cascade::*;
use super::*;
use twidget_litehtml::document::{Document, StyleCx, Switches};
use twidget_litehtml::render_item::{RenderId, RenderKind};

/// What the render trees reached, for the report.
#[derive(Default)]
pub struct RenderTally {
    pub documents: usize,
    pub dumps: usize,
    pub items: usize,
    pub kinds: BTreeMap<&'static str, usize>,
    pub counts: BTreeMap<&'static str, usize>,
}

impl RenderTally {
    fn count(&mut self, what: &'static str, yes: bool) {
        if yes {
            *self.counts.entry(what).or_default() += 1;
        }
    }

    pub fn report(&self) -> String {
        let kinds: Vec<String> = self.kinds.iter().map(|(k, n)| format!("{k} {n}")).collect();
        let counts: Vec<String> = self.counts.iter().map(|(k, n)| format!("{k} {n}")).collect();
        format!("{} documents, {} dumps, {} items; {}; {}", self.documents, self.dumps, self.items, kinds.join(", "), counts.join(", "))
    }

    /// The classes and the branches the tree shows it took.
    fn note(&mut self, doc: &Document, items: &[RenderId], from_root: usize, dom: &BTreeMap<ElementId, i32>) {
        self.dumps += 1;
        let mut listed: BTreeMap<RenderId, usize> = BTreeMap::new();
        self.items += items.len();
        let r = &doc.render;
        let mut elements_of_inlines: BTreeMap<ElementId, usize> = BTreeMap::new();
        for &id in items {
            let it = r.item(id);
            *self.kinds.entry(kind_name(it.kind)).or_default() += 1;
            let anon = !dom.contains_key(&it.element);
            let el = doc.el(it.element);
            let d = el.css.m_display;
            use twidget_litehtml::css_values::StyleDisplay as D;
            self.count("anonymous blocks", anon && d == D::display_block.index());
            self.count("anonymous table cells", anon && d == D::display_table_cell.index());
            self.count("anonymous table rows", anon && d == D::display_table_row.index());
            self.count("anonymous row groups", anon && d == D::display_table_row_group.index());
            self.count("anonymous tables", anon && d == D::display_table.index());
            self.count("items of an anonymous element's anonymous child", anon && el.parent.is_some_and(|p| !dom.contains_key(&p)));
            self.count("items whose parent has gone", it.parent.is_some() && r.parent(id).is_none());
            self.count("items under the root not reached from it", r.root.is_none_or(|root| root != id) && it.parent.is_none());
            self.count("display:none images and svgs", it.kind == RenderKind::render_item_image && d == D::display_none.index());
            self.count("floats", el.css.m_float != 0);
            if it.kind == RenderKind::render_item_inline {
                *elements_of_inlines.entry(it.element).or_default() += 1;
            }
            if let Some(grid) = it.table.as_ref().and_then(|t| t.grid.as_ref()) {
                self.count("table grids", true);
                self.count("tables with captions", !grid.captions.is_empty());
                let spans = grid.cells.iter().flatten().filter(|c| c.el.is_some() && (c.colspan > 1 || c.rowspan > 1)).count();
                self.count("tables with spans", spans > 0);
                self.count("grid places no cell takes", grid.cells.iter().flatten().any(|c| c.el.is_none()));
                self.count(
                    "row groups taken as cells",
                    grid.cells
                        .iter()
                        .flatten()
                        .any(|c| c.el.is_some_and(|e| r.get(e).is_some_and(|i| i.kind == RenderKind::render_item_table_part))),
                );
            } else if it.kind == RenderKind::render_item_table {
                self.count("tables without a grid", true);
            }
            for &child in &it.children {
                *listed.entry(child).or_default() += 1;
            }
        }
        self.count("items two lists of children hold", listed.values().any(|&n| n > 1));
        let reached: std::collections::BTreeSet<RenderId> = items[..from_root].iter().copied().collect();
        self.count("tabular items the root does not reach", r.tabular.iter().any(|t| !reached.contains(t)));
        self.count("inlines split around a block", elements_of_inlines.values().any(|&n| n > 1));
        self.count("table limit met", r.table_limited);
        for (&el, _) in dom.iter() {
            let e = doc.el(el);
            self.count("list items numbered", e.attrs.contains_key(&b"list_index"[..]));
            self.count("elements whose first render item has gone", !e.renders.is_empty() && doc.get_render_item(el).is_none());
            self.count("elements with two render items or more", e.renders.len() > 1);
            self.count("cells given their column's width", e.css.m_css_width != e.css.m_cell_width);
        }
    }
}

fn kind_name(kind: RenderKind) -> &'static str {
    match kind {
        RenderKind::render_item => "render_item",
        RenderKind::render_item_block => "block",
        RenderKind::render_item_block_context => "block context",
        RenderKind::render_item_inline_context => "inline context",
        RenderKind::render_item_inline => "inline",
        RenderKind::render_text => "text",
        RenderKind::render_item_image => "image",
        RenderKind::render_item_table => "table",
        RenderKind::render_item_table_part => "table part",
        RenderKind::render_item_table_row => "table row",
        RenderKind::render_item_flex => "flex",
        RenderKind::render_item_grid => "grid",
    }
}

/// lhdump's `TreeIndex`.
struct TreeIndex {
    items: Vec<RenderId>,
    item_index: BTreeMap<RenderId, i32>,
    dom: BTreeMap<ElementId, i32>,
    anon: Vec<ElementId>,
    anon_index: BTreeMap<ElementId, i32>,
}

impl TreeIndex {
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
}

fn renders_out(o: &mut Out, t: &TreeIndex, doc: &Document, el: ElementId) {
    let renders = &doc.el(el).renders;
    o.u32(renders.len() as u32);
    for &r in renders {
        o.i32(t.of(doc, Some(r)));
    }
    o.i32(t.of(doc, doc.get_render_item(el)));
}

fn grid_out(o: &mut Out, t: &TreeIndex, doc: &Document, item: RenderId) {
    let table = doc.render.item(item).table.as_ref().expect("a table's own");
    o.f32(table.border_spacing_x.value()).f32(table.border_spacing_y.value());
    let Some(grid) = &table.grid else {
        o.u8(0);
        return;
    };
    o.u8(1).i32(grid.rows_count).i32(grid.cols_count);
    for row in grid.rows.iter().take(grid.rows_count.max(0) as usize) {
        o.i32(t.of(doc, row.el_row));
        for v in [row.height, row.border_top, row.border_bottom, row.top, row.bottom, row.min_height] {
            o.f32(v.value());
        }
        len_out(o, &row.css_height);
    }
    for r in 0..grid.rows_count {
        for c in 0..grid.cols_count {
            let Some(cell) = grid.cell(c, r) else {
                o.u8(0);
                continue;
            };
            o.u8(1).i32(t.of(doc, cell.el)).i32(cell.colspan).i32(cell.rowspan);
            for v in [cell.min_width, cell.min_height, cell.max_width, cell.max_height, cell.width, cell.height] {
                o.f32(v.value());
            }
            for v in [cell.borders.left, cell.borders.right, cell.borders.top, cell.borders.bottom] {
                o.f32(v.value());
            }
        }
    }
    for col in grid.columns.iter().take(grid.cols_count.max(0) as usize) {
        for v in [col.min_width, col.max_width, col.width, col.border_left, col.border_right, col.left, col.right] {
            o.f32(v.value());
        }
        len_out(o, &col.css_width);
    }
    o.u32(grid.captions.len() as u32);
    for &cap in &grid.captions {
        o.i32(t.of(doc, Some(cap)));
    }
    o.f32(grid.top_captions_height.value()).f32(grid.bottom_captions_height.value());
}

/// lhdump's `write_render_tree`.
pub fn tree_out(o: &mut Out, names: &Names, doc: &Document, tally: &mut RenderTally) {
    let mut nodes = Vec::new();
    if let Some(root) = doc.root {
        cascade_walk(doc, root, -1, &mut nodes);
    }
    let mut t = TreeIndex {
        items: Vec::new(),
        item_index: BTreeMap::new(),
        dom: nodes.iter().enumerate().map(|(k, &(id, _))| (id, k as i32)).collect(),
        anon: Vec::new(),
        anon_index: BTreeMap::new(),
    };
    let r = &doc.render;
    if let Some(root) = r.root {
        t.visit(doc, root);
    }
    let from_root = t.items.len();
    for &item in &r.tabular {
        t.visit(doc, item);
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
        mark(o, "item's margins, padding, borders", -1);
        for m in [&it.margins, &it.padding, &it.borders] {
            for v in [m.left, m.right, m.top, m.bottom] {
                o.f32(v.value());
            }
        }
        o.u8(u8::from(it.skip));
        if it.kind == RenderKind::render_item_table {
            mark(o, "table", -1);
            grid_out(o, &t, doc, id);
        }
    }
    mark(o, "root, tabular, cells", -2);
    o.i32(t.of(doc, r.root));
    o.u32(r.tabular.len() as u32);
    for &item in &r.tabular {
        o.i32(t.of(doc, Some(item)));
    }
    o.i32(r.table_cells).u8(u8::from(r.table_limited)).u64(r.serial);
    o.u32(nodes.len() as u32);
    for (index, &(id, _)) in nodes.iter().enumerate() {
        mark(o, "DOM element", index as i32);
        match doc.get_attr(id, b"list_index") {
            Some(index) => {
                o.u8(1).str(index);
            }
            None => {
                o.u8(0);
            }
        }
        len_out(o, &doc.el(id).css.m_css_width);
        mark(o, "DOM element's render items", -1);
        renders_out(o, &t, doc, id);
    }
    let mut k = 0;
    while k < t.anon.len() {
        let id = t.anon[k];
        mark(o, "anonymous element", k as i32);
        k += 1;
        let el = doc.el(id);
        o.u8(1).u8(el.kind as u8);
        let parent = t.of_el(el.parent);
        o.i32(parent).str(&names.s(doc.tag(id)));
        css_out(o, names, &el.css, el.css.m_shared_id);
        mark(o, "anonymous element's styles", -1);
        if el.kind.is_html_tag() {
            o.u32(el.used_styles.len() as u32);
            style_map_out(o, names, &el.style);
            o.u8(u8::from(el.has_style_base));
            style_map_out(o, names, &el.style_base);
            o.i32(el.counter_changes).u8(u8::from(el.anonymous_style)).u32(el.attrs.len() as u32);
        }
        o.u32(el.counter_values.len() as u32);
        for (&name, &value) in &el.counter_values {
            o.str(&names.s(name)).i32(value);
        }
        mark(o, "anonymous element's render items", -1);
        renders_out(o, &t, doc, id);
    }
    o.u8(0);
    tally.note(doc, &t.items, from_root, &t.dom);
}

/// What lhdump answers to an `o` record of the render tree (`r`, then an `l` record's input).
pub fn render_answer(lh: &Lh, switches: Switches, input: &[u8], tally: &mut RenderTally) -> (u8, Vec<u8>) {
    assert_eq!(input.first(), Some(&b'r'), "a render tree record");
    tally.documents += 1;
    record_answer(lh, switches, &input[1..], &mut |o: &mut Out, cx: &StyleCx, doc: &Document| tree_out(o, cx.names, doc, tally))
}

/// An `o` record of the render tree.
pub fn render_record(width: f32, height: f32, dpr: f32, master: &[u8], user: &[u8], html: &[u8], steps: &[Step]) -> Vec<u8> {
    [&b"r"[..], &cascade_record(width, height, dpr, master, user, html, steps)].concat()
}

/// A dump's items as text, one line each, for a difference to be read: class, element, parent, children, and a
/// table's grid as rows × columns with its cells' items.
pub fn describe_items(dump: &[u8]) -> Vec<String> {
    fn len_skip(i: &mut In) {
        let _ = (i.u8(), i.i32(), i.u8(), i.f32());
        if i.u8() != 0 {
            let _ = (i.f32(), i.f32(), i.str());
        }
    }
    let mut i = In::new(dump);
    let mut out = Vec::new();
    let n = i.u32();
    for k in 0..n {
        let (kind, el, parent) = (i.u8(), i.i32(), i.i32());
        let children: Vec<i32> = (0..i.u32()).map(|_| i.i32()).collect();
        let boxes: Vec<f32> = (0..12).map(|_| i.f32()).collect();
        let skip = i.u8();
        let mut line = format!("{k}: kind {kind} el {el} parent {parent} children {children:?} boxes {boxes:?} skip {skip}");
        if kind == 7 {
            let (sx, sy) = (i.f32(), i.f32());
            line += &format!(" spacing {sx} {sy}");
            if i.u8() != 0 {
                let (rows, cols) = (i.i32(), i.i32());
                line += &format!(" grid {rows}x{cols}");
                for _ in 0..rows {
                    let row = i.i32();
                    let v: Vec<f32> = (0..6).map(|_| i.f32()).collect();
                    len_skip(&mut i);
                    line += &format!(" row {row} {v:?}");
                }
                for _ in 0..rows * cols {
                    if i.u8() != 0 {
                        let (c, cs, rs) = (i.i32(), i.i32(), i.i32());
                        let v: Vec<f32> = (0..10).map(|_| i.f32()).collect();
                        line += &format!(" cell {c} {cs}x{rs} {v:?}");
                    }
                }
                for _ in 0..cols {
                    let v: Vec<f32> = (0..7).map(|_| i.f32()).collect();
                    len_skip(&mut i);
                    line += &format!(" col {v:?}");
                }
                let caps: Vec<i32> = (0..i.u32()).map(|_| i.i32()).collect();
                let _ = (i.f32(), i.f32());
                line += &format!(" captions {caps:?}");
            } else {
                line += " no grid";
            }
        }
        out.push(line);
    }
    out
}
