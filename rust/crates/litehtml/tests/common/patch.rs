//! The Rust side of lhdump's `b` record (stage L19, document patching, with stage L18's incremental relayout): a
//! document as `createUpdatableFromString` makes it, then patches and what happens between them.
//!
//! - `b` f32 width, height, dpr, u8 flags (bit 0: the container sanitizes elements, logging `E` calls), master, html,
//!   u32 steps, each:
//!   - `u` str html: `update_from_string`; `s` u32 count and each u32 element (the k-th of the walk from the root,
//!     u32::MAX none) and str style: `update_styles`; `H` u32 count and each u32 element, u8 text, str name, str
//!     value: `update_holes`. Each answers u8 ok, u8 changed, u8 `last_patch_paint_only`, u8 `last_patch_regions`
//!     confined, u32 regions and each i32 element (-1 none, -2 out of the tree), u8 `template_html` ok and the str.
//!   - `m` f32 width, height: `media_changed` (u8). `h`, `a`, `x` u32 k: as `l`.
//!   - `w` f32 width, u8 how: as `l` (f32), then `layout_dirty_regions`: u8 ok, u32 count, 4 f32 each.
//!   - `d`: the cascade (`cascade_out`), the render tree (`tree_out`), the layout (`layout_out`), then what patching
//!     keeps: u8 template ok, u32 holes and each (u8 text | src << 1 | class << 2, name, attribute, u8 quote, value,
//!     i32 owner, u32 text elements and each i32), i32 selector scope, i32 style attributes selectable, u64 match
//!     epoch, the classes, ids and attributes selectors read of others (u32 and each name, sorted), u64 style epoch,
//!     positioned epoch and tree, u32 marked items, u32 restyle regions and each i32, u8 restyle confined, u8 boxes
//!     before valid, u32 boxes before, i32 generated boxes, u64 bytes; for each tag of the walk its match memos (u32,
//!     each u64 epoch, u32 attributes, three u32 rule counts) and class flips (u32, each value, u64 epoch, i32 parent
//!     share id, u32 other attributes, u32 children's styles, u32 classes, u8 base, u32 regions and each value, i32).
//!   - `f`: the layout against a fresh document of the HTML the document stands for (the last that patched, a
//!     template's after a hole patch) laid out at the last width: u8 1 alike or 2 not, u32 items of each, u32 the
//!     first that differs (0 the document's size). An item is its class, element, parent, children count, place,
//!     margins, padding, borders and skip (`geometry`).
//!
//!   Then the names the case interned and the container's calls by phase, as `l`.

use std::collections::BTreeMap;

use super::cascade::*;
use super::layout::{LayoutTally, layout_out};
use super::render::{RenderTally, tree_out};
use super::*;
use twidget_litehtml::document::{Document, StyleCx, Switches};
use twidget_litehtml::element::{ElementId, ElementKind};
use twidget_litehtml::patch::HoleChange;
use twidget_litehtml::render_item::RenderId;
use twidget_litehtml::types::{Margins, Position, RenderType};

/// One step of a `b` record.
#[derive(Clone, Debug)]
pub enum PatchStep {
    /// `update_from_string`.
    Html(Vec<u8>),
    /// `update_styles`: elements by their place in the walk (`u32::MAX` none) and styles.
    Styles(Vec<(u32, Vec<u8>)>),
    /// `update_holes`: element, text, name, value.
    Holes(Vec<(u32, bool, Vec<u8>, Vec<u8>)>),
    Media(f32, f32),
    Hover(u32),
    Active(u32),
    Leave(u32),
    /// A layout at the width within the renderer's budget.
    Layout(f32),
    Dump,
    Fresh,
}

/// A `b` record's input.
pub fn patch_record(width: f32, height: f32, dpr: f32, sanitizing: bool, master: &[u8], html: &[u8], steps: &[PatchStep]) -> Vec<u8> {
    let mut o = Out::default();
    o.f32(width).f32(height).f32(dpr).u8(u8::from(sanitizing)).str(master).str(html).u32(steps.len() as u32);
    for step in steps {
        match step {
            PatchStep::Html(html) => {
                o.u8(b'u').str(html);
            }
            PatchStep::Styles(list) => {
                o.u8(b's').u32(list.len() as u32);
                for (k, style) in list {
                    o.u32(*k).str(style);
                }
            }
            PatchStep::Holes(list) => {
                o.u8(b'H').u32(list.len() as u32);
                for (k, text, name, value) in list {
                    o.u32(*k).u8(u8::from(*text)).str(name).str(value);
                }
            }
            PatchStep::Media(w, h) => {
                o.u8(b'm').f32(*w).f32(*h);
            }
            PatchStep::Hover(k) => {
                o.u8(b'h').u32(*k);
            }
            PatchStep::Active(k) => {
                o.u8(b'a').u32(*k);
            }
            PatchStep::Leave(k) => {
                o.u8(b'x').u32(*k);
            }
            PatchStep::Layout(w) => {
                o.u8(b'w').f32(*w).u8(0);
            }
            PatchStep::Dump => {
                o.u8(b'd');
            }
            PatchStep::Fresh => {
                o.u8(b'f');
            }
        }
    }
    o.take()
}

/// The DOM's elements by their walk from the root.
pub struct ElementIndex {
    pub nodes: Vec<(ElementId, i32)>,
    pub index: BTreeMap<ElementId, i32>,
}

impl ElementIndex {
    pub fn new(doc: &Document) -> ElementIndex {
        let mut nodes = Vec::new();
        if let Some(root) = doc.root {
            cascade_walk(doc, root, -1, &mut nodes);
        }
        let index = nodes.iter().enumerate().map(|(k, &(id, _))| (id, k as i32)).collect();
        ElementIndex { nodes, index }
    }

    pub fn of(&self, el: Option<ElementId>) -> i32 {
        match el {
            None => -1,
            Some(el) => self.index.get(&el).copied().unwrap_or(-2),
        }
    }

    pub fn pick(&self, k: u32) -> Option<ElementId> {
        if k == u32::MAX || self.nodes.is_empty() {
            return None;
        }
        Some(self.nodes[k as usize % self.nodes.len()].0)
    }
}

fn pos_out(o: &mut Out, p: &Position) {
    o.f32(p.x.value()).f32(p.y.value()).f32(p.width.value()).f32(p.height.value());
}

fn sides_out(o: &mut Out, m: &Margins) {
    o.f32(m.left.value()).f32(m.right.value()).f32(m.top.value()).f32(m.bottom.value());
}

/// lhdump's `write_patch_answer`.
pub fn patch_answer_out(o: &mut Out, doc: &Document, ok: bool, changed: bool) {
    let w = ElementIndex::new(doc);
    mark(o, "patch answer", -2);
    o.u8(u8::from(ok)).u8(u8::from(changed)).u8(u8::from(doc.last_patch_paint_only()));
    let regions = doc.last_patch_regions();
    o.u8(u8::from(regions.is_some()));
    let regions = regions.unwrap_or_default();
    o.u32(regions.len() as u32);
    for el in regions {
        o.i32(w.of(Some(el)));
    }
    mark(o, "template html", -2);
    let html = doc.template_html();
    o.u8(u8::from(html.is_some())).str(&html.unwrap_or_default());
}

fn names_sorted_out(o: &mut Out, names: &Names, ids: &std::collections::BTreeSet<StringId>) {
    let mut list: Vec<Vec<u8>> = ids.iter().map(|&id| names.s(id).to_vec()).collect();
    list.sort();
    o.u32(list.len() as u32);
    for name in list {
        o.str(&name);
    }
}

/// lhdump's `write_patch_state`.
fn patch_state_out(o: &mut Out, names: &Names, doc: &Document) {
    use twidget_litehtml::element::ElementState;
    let w = ElementIndex::new(doc);
    mark(o, "holes", -2);
    o.u8(u8::from(doc.template_ok)).u32(doc.holes.len() as u32);
    for hole in &doc.holes {
        o.u8(u8::from(hole.text) | u8::from(hole.src) << 1 | u8::from(hole.cls) << 2);
        o.str(&hole.name).str(&hole.attr).u8(hole.close).str(&hole.value).i32(w.of(hole.owner));
        o.u32(hole.text_elements.len() as u32);
        for &el in &hole.text_elements {
            o.i32(w.of(Some(el)));
        }
    }
    mark(o, "selector scope", -2);
    let p = &doc.patch;
    o.i32(p.selector_scope).i32(p.style_attributes_selectable.get()).u64(p.match_epoch);
    names_sorted_out(o, names, &p.left_classes);
    names_sorted_out(o, names, &p.left_ids);
    names_sorted_out(o, names, &p.left_attrs);
    mark(o, "epochs and the layout's patch state", -2);
    o.u64(doc.layout.style_epoch).u64(doc.layout.positioned_epoch).u64(doc.layout.positioned_tree);
    o.u32(p.layout_marked.len() as u32).u32(p.restyle_regions.len() as u32);
    for &el in &p.restyle_regions {
        o.i32(w.of(Some(el)));
    }
    o.u8(u8::from(p.restyle_confined)).u8(u8::from(p.layout_before_valid)).u32(p.layout_before.len() as u32);
    o.i32(doc.generated_boxes).u64(doc.generated_bytes as u64);
    for (k, &(el, _)) in w.nodes.iter().enumerate() {
        if !doc.kind(el).is_html_tag() {
            continue;
        }
        mark(o, "memos and flips", k as i32);
        let e = doc.el(el);
        o.u32(e.patch.match_memos.len() as u32);
        for memo in &e.patch.match_memos {
            o.u64(memo.epoch).u32(memo.attrs.len() as u32);
            for rules in &memo.rules {
                o.u32(rules.len() as u32);
            }
        }
        o.u32(e.patch.flips.len() as u32);
        for flip in &e.patch.flips {
            o.str(&flip.value).u64(flip.epoch).i32(flip.parent_id).u32(flip.others.len() as u32).u32(flip.kids.len() as u32);
            o.u32(flip.classes.len() as u32).u8(u8::from(flip.has_base)).u32(flip.regions.len() as u32);
            for (from, &region) in &flip.regions {
                o.str(from).i32(region);
            }
        }
    }
    let _ = ElementState::None;
}

/// lhdump's `write_geometry`: the document's size, then each item of the tree from the root.
pub fn geometry(doc: &Document) -> Vec<Vec<u8>> {
    let w = ElementIndex::new(doc);
    let mut items: Vec<RenderId> = Vec::new();
    let mut item_index: BTreeMap<RenderId, i32> = BTreeMap::new();
    if let Some(root) = doc.render.root {
        let mut pending = vec![root];
        while let Some(cur) = pending.pop() {
            if item_index.contains_key(&cur) {
                continue;
            }
            item_index.insert(cur, items.len() as i32);
            items.push(cur);
            pending.extend(doc.render.children(cur).iter().rev());
        }
    }
    let mut anon: BTreeMap<ElementId, i32> = BTreeMap::new();
    let mut out = Vec::new();
    let mut head = Out::default();
    head.f32(doc.layout.size.width.value()).f32(doc.layout.size.height.value());
    out.push(head.take());
    for &id in &items {
        let it = doc.render.item(id);
        let mut o = Out::default();
        o.u8(it.kind as u8);
        let el = it.element;
        let el_index = match w.index.get(&el) {
            Some(&k) => k,
            None => {
                let next = -2 - anon.len() as i32;
                *anon.entry(el).or_insert(next)
            }
        };
        let parent = match doc.render.parent(id) {
            None => -1,
            Some(p) => item_index.get(&p).copied().unwrap_or(-3),
        };
        o.i32(el_index).i32(parent).u32(it.children.len() as u32);
        pos_out(&mut o, &it.pos);
        for m in [&it.margins, &it.padding, &it.borders] {
            sides_out(&mut o, m);
        }
        o.u8(u8::from(it.skip));
        out.push(o.take());
    }
    out
}

/// What the patch records reached, for the report.
#[derive(Default)]
pub struct PatchTally(pub BTreeMap<String, usize>);

impl PatchTally {
    pub fn add(&mut self, what: impl Into<String>) {
        *self.0.entry(what.into()).or_default() += 1;
    }
    pub fn count(&self, what: &str) -> usize {
        self.0.get(what).copied().unwrap_or(0)
    }
    pub fn merge(&mut self, other: PatchTally) {
        for (k, v) in other.0 {
            *self.0.entry(k).or_default() += v;
        }
    }
    pub fn report(&self) -> String {
        self.0.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", ")
    }
}

/// Where a patched document's layout and a fresh one's part: the first item that differs, as text.
pub fn describe_fresh(names: &Names, doc: &Document, fresh: &Document) -> String {
    let a = geometry(doc);
    let b = geometry(fresh);
    let first = a.iter().zip(&b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
    let show = |g: &[Vec<u8>], d: &Document| -> String {
        if first == 0 {
            return format!("size {:?}", d.layout.size);
        }
        let Some(bytes) = g.get(first) else { return "none".to_string() };
        let mut i = In::new(bytes);
        let (kind, el, parent, children) = (i.u8(), i.i32(), i.i32(), i.u32());
        let floats: Vec<f32> = (0..16).map(|_| i.f32()).collect();
        let w = ElementIndex::new(d);
        let tag = if el >= 0 { String::from_utf8_lossy(&d.get_tag_name(names, w.nodes[el as usize].0)).to_string() } else { String::new() };
        format!(
            "kind {kind} element {el} <{tag}> parent {parent} children {children} pos {:?} margins {:?} padding {:?} borders {:?}",
            &floats[0..4],
            &floats[4..8],
            &floats[8..12],
            &floats[12..16]
        )
    };
    format!("{} items patched, {} fresh; item {first}: patched {} / fresh {}", a.len(), b.len(), show(&a, doc), show(&b, fresh))
}

/// Why a patched document's layout parts from a fresh one's, as a kind (for the report): the first item that differs.
pub fn classify_fresh(names: &Names, doc: &Document, fresh: &Document) -> String {
    let a = geometry(doc);
    let b = geometry(fresh);
    if doc.layout.exhausted || fresh.layout.exhausted {
        return "past the layout budget".into();
    }
    if a.len() != b.len() {
        return format!("render trees of {} and {} items", a.len(), b.len());
    }
    let Some(first) = a.iter().zip(&b).position(|(x, y)| x != y) else { return "alike".into() };
    if first == 0 {
        return if a.len() == 1 { "no render tree: the last layout's size kept".into() } else { "the document's size alone".into() };
    }
    let mut items: Vec<RenderId> = Vec::new();
    if let Some(root) = doc.render.root {
        let mut pending = vec![root];
        while let Some(cur) = pending.pop() {
            if items.contains(&cur) {
                continue;
            }
            items.push(cur);
            pending.extend(doc.render.children(cur).iter().rev());
        }
    }
    let id = items[first - 1];
    let it = doc.render.item(id);
    let (pa, pb) = (&a[first][13..29], &b[first][13..29]);
    let only_place = a[first][..13] == b[first][..13] && pa[8..] == pb[8..] && a[first][29..] == b[first][29..];
    let tag = String::from_utf8_lossy(&doc.get_tag_name(names, it.element)).into_owned();
    let pieces = it.text.as_ref().is_some_and(|t| !t.fragments.is_empty());
    if it.text.is_some() && only_place {
        let space = if doc.is_white_space(it.element) {
            "a collapsible space"
        } else if doc.is_space(it.element) {
            "a space"
        } else {
            "a word"
        };
        let skipped = it.skip;
        return format!("{space}'s own place (cut into pieces: {pieces}, skipped: {skipped})");
    }
    if only_place {
        return format!("the place of a {tag} ({:?})", it.kind);
    }
    format!("the boxes of a {tag} ({:?})", it.kind)
}

/// A fresh document of the HTML, as lhdump's `f` makes it, laid out at the width.
pub fn fresh_document(
    lh: &Lh,
    switches: Switches,
    container: &CascadeContainer,
    master: &[u8],
    html: &[u8],
    width: f32,
) -> (Document, CascadeContainer) {
    let other = CascadeContainer::new(container.width.get(), container.height.get(), container.dpr.get());
    other.sanitizing.set(container.sanitizing.get());
    let doc = {
        let cx = StyleCx { names: &lh.names, container: &other, cache: &lh.cache, switches };
        let mut fresh = Document::create_updatable(html, master, &cx);
        let budget = renderer_layout_budget(&fresh);
        fresh.set_layout_budget(budget);
        fresh.layout_document(&cx, Px(width));
        fresh
    };
    (doc, other)
}

/// What sees the document after each step: the step's index, and after `f` the fresh document.
pub type Observer<'a> = dyn FnMut(usize, &StyleCx, &Document, Option<&Document>) + 'a;

/// What lhdump answers to a `b` record, with the process's name table and declaration cache; `observe` sees the
/// document after each step (with the step's index and, after `f`, the fresh document).
pub fn patch_answer_with(lh: &Lh, switches: Switches, input: &[u8], tally: &mut PatchTally, observe: &mut Observer) -> (u8, Vec<u8>) {
    let names = &lh.names;
    let mut i = In::new(input);
    let (width, height, dpr) = (i.f32(), i.f32(), i.f32());
    let names_before = names.usage().names;
    let container = CascadeContainer::new(width, height, dpr);
    let flags = i.u8();
    container.sanitizing.set(flags & 1 != 0);
    let master = i.str();
    let mut current = i.str();
    let cx = StyleCx { names, container: &container, cache: &lh.cache, switches };
    let mut doc = Document::create_updatable(&current, &master, &cx);
    let mut o = Out::default();
    o.u8(doc.mode as u8);
    let mut phases = vec![container.calls.borrow().len()];
    let mut last_width = width;
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
        let mut fresh_doc = None;
        match op {
            b'u' => {
                let html = i.str();
                let mut changed = true;
                let ok = doc.update_from_string(&cx, &html, &mut changed);
                if ok {
                    current = html;
                }
                tally.add(format!("update_from_string {}", if ok { "patched" } else { "refused" }));
                note_patch(tally, &doc, ok, changed);
                patch_answer_out(&mut o, &doc, ok, changed);
            }
            b's' | b'H' => {
                let w = ElementIndex::new(&doc);
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
                if ok && let Some(html) = doc.template_html() {
                    current = html;
                }
                let kind = if op == b's' { "update_styles" } else { "update_holes" };
                tally.add(format!("{kind} {}", if ok { "patched" } else { "refused" }));
                note_patch(tally, &doc, ok, changed);
                patch_answer_out(&mut o, &doc, ok, changed);
            }
            b'd' => {
                mark(&o, "cascade", -2);
                cascade_out(&mut o, names, &doc);
                mark(&o, "render tree", -2);
                tree_out(&mut o, names, &doc, &mut RenderTally::default());
                mark(&o, "layout", -2);
                layout_out(&mut o, &cx, &doc, &mut LayoutTally::default());
                patch_state_out(&mut o, names, &doc);
                tally.add("dumps");
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
                last_width = width;
                let mut regions = Vec::new();
                let dirty = doc.layout_dirty_regions(&mut regions);
                o.u8(u8::from(dirty));
                if dirty {
                    tally.add(if regions.is_empty() { "layouts dirtying nothing" } else { "layouts with dirty regions" });
                }
                o.u32(regions.len() as u32);
                for r in &regions {
                    pos_out(&mut o, r);
                }
            }
            b'm' => {
                container.width.set(i.f32());
                container.height.set(i.f32());
                let changed = doc.root.is_some() && doc.media_changed(&cx);
                o.u8(u8::from(changed));
            }
            b'f' => {
                let (fresh, _other) = fresh_document(lh, switches, &container, &master, &current, last_width);
                let a = geometry(&doc);
                let b = geometry(&fresh);
                let first = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
                mark(&o, "fresh comparison", -2);
                o.u8(if a == b { 1 } else { 2 }).u32(a.len() as u32).u32(b.len() as u32).u32(first as u32);
                tally.add(if a == b { "fresh layouts alike" } else { "fresh layouts differ" });
                fresh_doc = Some(fresh);
            }
            _ => {
                let k = i.u32();
                let w = ElementIndex::new(&doc);
                let tags: Vec<ElementId> = w.nodes.iter().map(|n| n.0).filter(|&id| doc.kind(id).is_html_tag()).collect();
                if tags.is_empty() {
                    o.u8(0);
                    observe(step, &cx, &doc, None);
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
        observe(step, &cx, &doc, fresh_doc.as_ref());
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

/// What lhdump answers to a `b` record.
pub fn patch_answer(lh: &Lh, switches: Switches, input: &[u8], tally: &mut PatchTally) -> (u8, Vec<u8>) {
    patch_answer_with(lh, switches, input, tally, &mut |_, _, _, _| {})
}

fn note_patch(tally: &mut PatchTally, doc: &Document, ok: bool, changed: bool) {
    if !ok {
        return;
    }
    if !changed {
        tally.add("patches that changed nothing");
        return;
    }
    if doc.last_patch_paint_only() {
        tally.add("paint-only patches");
        if let Some(regions) = doc.last_patch_regions() {
            tally.add("confined patches");
            if regions.iter().any(|&el| doc.kind(el) == ElementKind::el_text) {
                tally.add("patches repainting words");
            }
        }
    }
}
