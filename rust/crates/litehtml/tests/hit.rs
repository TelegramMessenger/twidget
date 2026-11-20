//! Hit testing and the mouse state, ported (stage L16): documents made as the renderer makes them
//! (`createUpdatableFromString`), laid out and painted, then pointed at through `oracle/lhdump` (`W`) and the port:
//! `get_element_by_point` from the root over a grid of points with and without the renderer's `hittable` check, from
//! elements as the renderer's `element_at_within` asks it, from render items and their children's passes with every
//! check (each check asked logged in order); `on_mouse_over`, the button's down, up and cancel and `on_mouse_leave` in
//! sequences, with their answers, redraw boxes, the hovered and active elements, the cursor, the mouse events and the
//! clicks the container gets; `on_scroll` of every scroller and the render items' scrolling, `is_point_inside` of
//! every class, an `<svg>`'s `element_at`; patches and relayouts between the steps (the hovered and active elements
//! remapped through `update_structure`), and after each change of state the restyled cascade, the layout with its
//! scroll positions, a paint of what scrolled and what painting keeps, with every container call compared in order.
//! The documents: widget-sandbox's corpus light and dark at several widths, written shapes for each branch (z-index
//! stacking with negative layers, transforms, overflow clips with radii, `pointer-events: none`, clip paths, fixed and
//! sticky boxes, scrolled and nested scrollers, tables, inline text across lines, floats, SVG shapes and `<use>`,
//! form controls, right to left), generated documents, the layout switches, and a deep document on a small stack.

mod common;

use common::cascade::*;
use common::gumbo::{corpus_pages, snapshot};
use common::hit::*;
use common::paint::{PaintIndex, PaintStep, compare_paint_calls};
use common::patch::ElementIndex;
use common::*;
use twidget_litehtml::document::{Document, StyleCx, Switches};
use twidget_litehtml::element::ElementKind;
use twidget_litehtml::types::Position;

/// A case: its record, the port's answer and what it is.
type Case = (Vec<u8>, (u8, Vec<u8>), String);

/// `W` cases through lhdump and the port, checked in batches of about 16 MB; settings (`E`) open every batch.
struct Run {
    name: String,
    lh: Lh,
    switches: Switches,
    settings: Vec<u8>,
    batch: Vec<Case>,
    bytes: usize,
    cases: usize,
    steps: usize,
    tally: HitTally,
    oracle: Option<Oracle>,
}

impl Run {
    fn new(name: &str) -> Run {
        Run {
            name: name.to_string(),
            lh: Lh::new(),
            switches: Switches::default(),
            settings: Vec::new(),
            batch: Vec::new(),
            bytes: 0,
            cases: 0,
            steps: 0,
            tally: HitTally::default(),
            oracle: Oracle::find("lhdump"),
        }
    }

    /// A run with a switch set on both sides.
    fn with_switch(name: &str, switch: &str) -> Run {
        let mut run = Run::new(name);
        run.settings = framed(b'E', format!("{switch}=1").as_bytes());
        match switch {
            "LH_NO_LAYOUT_CACHE" => run.switches.layout_cache = false,
            "LH_NO_POSITIONED_REUSE" => run.switches.positioned_reuse = false,
            "LH_NO_FIXED_LAYOUT_REUSE" => run.switches.fixed_layout_reuse = false,
            "LH_LAYOUT_CACHE_CHECK" => run.switches.layout_cache_check = true,
            "LH_NO_COLUMN_REUSE" => run.switches.column_reuse = false,
            "LH_NO_TRANSFORMED_CULLING" => run.switches.transformed_culling = false,
            other => panic!("no switch {other}"),
        }
        run
    }

    fn case(&mut self, width: f32, height: f32, master: &[u8], html: &[u8], steps: &[HitStep], what: impl Into<String>) {
        let input = hit_record(width, height, 2.0, false, master, html, steps);
        let mut tally = HitTally::default();
        let rust = hit_answer(&self.lh, self.switches, &input, &mut tally);
        self.cases += 1;
        self.steps += steps.len();
        self.tally.merge(tally);
        self.bytes += input.len() + rust.1.len();
        if self.oracle.is_some() {
            self.batch.push((input, rust, what.into()));
        }
        if self.bytes > 16 << 20 {
            self.flush();
        }
    }

    fn flush(&mut self) {
        let batch = std::mem::take(&mut self.batch);
        self.bytes = 0;
        let Some(oracle) = &self.oracle else {
            self.lh = Lh::new();
            return;
        };
        if batch.is_empty() {
            return;
        }
        let mut input = self.settings.clone();
        for (case, _, _) in &batch {
            input.extend(framed(b'W', case));
        }
        let answers = oracle.run(input, batch.len());
        // Every batch is a process of its own: the next one starts with a fresh name table and declaration cache.
        self.lh = Lh::new();
        let mut differences = Vec::new();
        for ((case, rust, what), cpp) in batch.iter().zip(&answers) {
            let (cpp_body, _) = split_answer(&cpp.1);
            let (rust_body, _) = split_answer(&rust.1);
            if cpp.0 != rust.0 || cpp_body != rust_body {
                differences.push(explain(self.switches, case, &(cpp.0, cpp_body), &(rust.0, rust_body), what));
            } else if let Some(calls) = compare_paint_calls(&cpp.1, &rust.1) {
                differences.push(format!("{calls}\n  in {}", what.chars().take(3000).collect::<String>()));
            }
            if differences.len() >= 3 {
                break;
            }
        }
        assert!(differences.is_empty(), "{}: answers differ, the first:\n{}", self.name, differences.join("\n"));
    }

    fn finish(mut self) -> HitTally {
        self.flush();
        let compared = if self.oracle.is_some() { "alike with lhdump" } else { "pointed (no oracle)" };
        eprintln!("{}: {} cases, {} steps {compared}; {}", self.name, self.cases, self.steps, self.tally.report());
        std::mem::take(&mut self.tally)
    }
}

/// Where two answers' bodies part: the field the Rust answer was writing there.
fn explain(switches: Switches, case: &[u8], cpp: &(u8, Vec<u8>), rust: &(u8, Vec<u8>), what: &str) -> String {
    let at = cpp.1.iter().zip(&rust.1).position(|(a, b)| a != b).unwrap_or(cpp.1.len().min(rust.1.len()));
    let marks = {
        start_marks();
        hit_answer(&Lh::new(), switches, case, &mut HitTally::default());
        take_marks()
    };
    let item = marks.iter().rfind(|m| m.0 <= at && m.2 >= 0).map_or(-1, |m| m.2);
    let field = marks.iter().rfind(|m| m.0 <= at).map_or("", |m| m.1);
    let step = marks.iter().rfind(|m| m.0 <= at && m.1 == "step").map_or(-1, |m| m.2);
    let near = |b: &[u8]| {
        let piece = &b[at.saturating_sub(16).min(b.len())..(at + 64).min(b.len())];
        hex(&piece[..piece.len().min(48)])
    };
    format!(
        "status C++ {} / Rust {}, {} / {} bytes, first difference at byte {at}, step {step}, {field} (item or element {item}):\n  C++  {}\n  Rust {}\n  in {}",
        cpp.0,
        rust.0,
        cpp.1.len(),
        rust.1.len(),
        near(&cpp.1),
        near(&rust.1),
        what.chars().take(3000).collect::<String>()
    )
}

// ---- Aiming: the port's document laid out, to aim the steps at what is there -------------------------------------

/// What a laid-out document offers to point at: its elements' border boxes, its scrollers' padding boxes, its
/// `<svg>`s' boxes and how many render items it has.
struct Aim {
    width: f32,
    height: f32,
    doc_height: f32,
    /// Each tag with a render item: its place in the walk and its border box in the document.
    elements: Vec<(u32, Position)>,
    /// Each item with a scroll view that can scroll: its place among the items, its element's in the walk, its padding
    /// box in the document.
    scrollers: Vec<(u32, u32, Position)>,
    svgs: Vec<(u32, Position)>,
    /// The elements with an `id`: their place in the walk and border box.
    ids: std::collections::BTreeMap<Vec<u8>, (u32, Position)>,
    /// Each element's first render item's place among the items.
    item_of: std::collections::BTreeMap<u32, u32>,
    items: usize,
}

impl Aim {
    fn new(width: f32, height: f32, master: &[u8], html: &[u8], switches: Switches) -> Aim {
        let lh = Lh::new();
        let container = CascadeContainer::new(width, height, 2.0);
        let cx = StyleCx { names: &lh.names, container: &container, cache: &lh.cache, switches };
        let mut doc = Document::create_updatable(html, master, &cx);
        let budget = renderer_layout_budget(&doc);
        doc.set_layout_budget(budget);
        doc.layout_document(&cx, twidget_litehtml::Px(width));
        Aim::of(&cx, &doc, width, height)
    }

    fn of(cx: &StyleCx, doc: &Document, width: f32, height: f32) -> Aim {
        let w = ElementIndex::new(doc);
        let index = PaintIndex::new(doc);
        let mut elements = Vec::new();
        let mut svgs = Vec::new();
        let mut ids = std::collections::BTreeMap::new();
        let mut item_of = std::collections::BTreeMap::new();
        for (k, &(el, _)) in w.nodes.iter().enumerate() {
            if !doc.kind(el).is_html_tag() {
                continue;
            }
            let Some(ri) = doc.get_render_item(el) else { continue };
            if let Some(&n) = index.item_index.get(&ri) {
                item_of.insert(k as u32, n as u32);
            }
            let it = doc.render.item(ri);
            let mut b = doc.get_placement(cx, ri);
            b.grow(&it.padding);
            b.grow(&it.borders);
            if !(b.width.value().is_finite() && b.height.value().is_finite() && b.x.value().is_finite() && b.y.value().is_finite()) {
                continue;
            }
            elements.push((k as u32, b));
            if let Some(id) = doc.get_attr(el, b"id") {
                ids.entry(id.to_vec()).or_insert((k as u32, b));
            }
            if doc.kind(el) == ElementKind::el_svg {
                svgs.push((k as u32, b));
            }
        }
        let mut scrollers = Vec::new();
        for (n, &ri) in index.items.iter().enumerate() {
            let it = doc.render.item(ri);
            let Some(view) = it.scroll_view else { continue };
            if view.get_max_h_scroll() <= twidget_litehtml::Px(0.0) && view.get_max_v_scroll() <= twidget_litehtml::Px(0.0) {
                continue;
            }
            let mut b = doc.get_placement(cx, ri);
            b.grow(&it.padding);
            scrollers.push((n as u32, w.of(Some(it.element)).max(0) as u32, b));
        }
        Aim { width, height, doc_height: doc.layout.size.height.value(), elements, scrollers, svgs, ids, item_of, items: index.items.len() }
    }

    /// A box's centre.
    fn centre(b: &Position) -> (f32, f32) {
        (b.x.value() + b.width.value() / 2.0, b.y.value() + b.height.value() / 2.0)
    }

    /// The grid of points over the document: `columns` across the viewport, `rows` down the document (at most three
    /// viewports deep), the last ones a little past the edges.
    fn grid(&self, columns: usize, rows: usize) -> Vec<(f32, f32)> {
        let deep = self.doc_height.clamp(self.height * 0.5, self.height * 3.0);
        let mut out = Vec::new();
        for r in 0..rows {
            for c in 0..columns {
                let x = (c as f32 + 0.5) * self.width / columns as f32;
                let y = (r as f32 + 0.37) * deep / rows as f32;
                out.push((x.floor() + 0.5, y.floor() + 0.25));
            }
        }
        out.push((-1.0, 10.0));
        out.push((self.width + 1.0, 10.0));
        out.push((10.0, -1.0));
        out
    }
}

/// A paint of the viewport (scrolled down by `y`).
fn paint(width: f32, height: f32, y: f32) -> HitStep {
    let mut p = PaintStep::viewport(width, height);
    p.y = -y;
    HitStep::Paint(Box::new(p))
}

/// The pointer steps over a laid-out document: the grid with and without the renderer's check, elements as the
/// renderer asks within them, items and their children's passes, hovers and presses with a relayout, a paint and a
/// dump after the first changes, every scroller scrolled (and the paint and the dump after), the `<svg>`s, and each
/// item class's `is_point_inside`.
fn pointer_steps(r: &mut Lcg, aim: &Aim, columns: usize, rows: usize) -> Vec<HitStep> {
    let (width, height) = (aim.width, aim.height);
    let mut steps = vec![HitStep::Layout(width), paint(width, height, 0.0)];
    for (x, y) in aim.grid(columns, rows) {
        steps.push(HitStep::At(At::root(0, x, y)));
        steps.push(HitStep::At(At::root(1, x, y)));
    }
    // Edges: an element's left and top edges are in, its right and bottom ones out.
    for _ in 0..aim.elements.len().min(6) {
        let (k, b) = aim.elements[r.below(aim.elements.len())];
        let (x0, y0) = (b.x.value(), b.y.value());
        let (x1, y1) = (x0 + b.width.value(), y0 + b.height.value());
        for (x, y) in [(x0, y0), (x1, y1), (x1 - 0.00005, y0 + 0.00005), (x0 - 0.0002, y1 - 1.0)] {
            steps.push(HitStep::At(At::root(0, x, y)));
        }
        let (cx, cy) = Aim::centre(&b);
        steps.push(HitStep::At(At::within(k, cx, cy)));
        steps.push(HitStep::At(At { check: 0, ..At::within(k, x0 + 1.0, y1 - 1.0) }));
    }
    for _ in 0..6 {
        let x = r.below(width as usize + 1) as f32;
        let y = r.below(aim.doc_height.max(1.0) as usize + 1) as f32;
        let k = r.below(aim.items.max(1)) as u32;
        let check = [0u8, 1, 4, 5][r.below(4)];
        steps.push(HitStep::At(At { check, from: 2, k, ..At::root(check, x, y) }));
        let flag = r.below(5) as u8;
        let zindex = [0, 1, -1, 2][r.below(4)];
        steps.push(HitStep::At(At { check, from: 3, k, flag, zindex, ..At::root(check, x, y) }));
        // A client point apart from the point: what a fixed box is placed against.
        steps.push(HitStep::At(At { client_y: y - height * 0.5, ..At::root(check, x, y) }));
    }
    // Hovers, a press and a click, a press dragged away, a cancel and the pointer leaving.
    let mut targets: Vec<(f32, f32)> = Vec::new();
    for _ in 0..4 {
        if aim.elements.is_empty() {
            break;
        }
        let (_, b) = aim.elements[r.below(aim.elements.len())];
        targets.push(Aim::centre(&b));
    }
    while targets.len() < 4 {
        targets.push((r.below(width as usize) as f32, r.below(height as usize) as f32));
    }
    let at = |p: (f32, f32)| [p.0, p.1, p.0, p.1];
    steps.push(HitStep::Over(at(targets[0])));
    steps.extend([HitStep::Layout(width), paint(width, height, 0.0), HitStep::Dump]);
    steps.push(HitStep::Over(at(targets[1])));
    steps.push(HitStep::Over(at(targets[1])));
    steps.push(HitStep::Down(at(targets[1])));
    steps.extend([HitStep::Layout(width), HitStep::Dump]);
    steps.push(HitStep::Up(at(targets[1])));
    steps.push(HitStep::Down(at(targets[2])));
    steps.push(HitStep::Over(at(targets[3])));
    steps.push(HitStep::Up(at(targets[3])));
    steps.push(HitStep::Up(at(targets[3])));
    steps.extend([HitStep::Layout(width), paint(width, height, 0.0)]);
    steps.push(HitStep::Down(at(targets[0])));
    steps.push(HitStep::Cancel);
    steps.push(HitStep::Over(at(targets[2])));
    steps.push(HitStep::Leave);
    steps.push(HitStep::Leave);
    steps.extend([HitStep::Layout(width), HitStep::Dump]);
    // Every scroller (the first few), each way, past its ends, and the paint after.
    for (n, &(item, el, b)) in aim.scrollers.iter().take(4).enumerate() {
        let (cx, cy) = Aim::centre(&b);
        steps.push(HitStep::Item { what: 4, k: item, a: 30.0, b: 0.0 });
        steps.push(HitStep::Item { what: 3, k: item, a: 30.0, b: 0.0 });
        steps.push(HitStep::Scroll([0.0, 40.0, cx, cy, cx, cy]));
        steps.push(HitStep::Scroll([25.0, 0.0, cx, cy, cx, cy]));
        steps.push(HitStep::Scroll([10.0, 10.0, cx, cy, cx, cy]));
        steps.push(HitStep::At(At::root(1, cx, cy)));
        if n == 0 {
            steps.extend([paint(width, height, 0.0), HitStep::Dump]);
        }
        steps.push(HitStep::Scroll([0.0, -100000.0, cx, cy, cx, cy]));
        steps.push(HitStep::Scroll([100000.0, 100000.0, cx, cy, cx, cy]));
        steps.push(HitStep::Item { what: 5, k: item, a: cx, b: cy });
        steps.push(HitStep::Item { what: 2, k: el, a: -15.0, b: 0.0 });
        steps.push(HitStep::Item { what: 1, k: el, a: -15.0, b: 0.0 });
        steps.push(HitStep::Item { what: 0, k: item, a: 3.5, b: 7.25 });
        steps.push(HitStep::Over([cx, cy, cx, cy]));
    }
    if !aim.scrollers.is_empty() {
        steps.extend([HitStep::Layout(width), paint(width, height, 0.0), HitStep::Dump]);
    }
    steps.push(HitStep::Scroll([0.0, 0.0, 5.0, 5.0, 5.0, 5.0]));
    steps.push(HitStep::Scroll([0.0, 30.0, 5.0, 5.0, 5.0, 5.0]));
    // The <svg>s: points over their boxes, at their placement and moved.
    for &(k, b) in aim.svgs.iter().take(3) {
        for (fx, fy) in [(0.25, 0.25), (0.5, 0.5), (0.75, 0.6), (0.1, 0.9)] {
            let (x, y) = (b.x.value() + b.width.value() * fx, b.y.value() + b.height.value() * fy);
            steps.push(HitStep::Svg { k, at: None, x, y });
        }
        steps.push(HitStep::Svg { k, at: Some([0.0, 0.0, 100.0, 50.0]), x: 50.0, y: 25.0 });
    }
    for _ in 0..8 {
        let k = r.below(aim.items.max(1)) as u32;
        let (x, y) = (r.below(width as usize + 1) as f32, r.below(aim.doc_height.max(1.0) as usize + 1) as f32);
        steps.push(HitStep::Item { what: 6, k, a: x, b: y });
    }
    // The items' and elements' scrolling where most have no view, or no render item.
    for what in [0u8, 1, 2, 3, 4, 5] {
        let k = r.below(aim.items.max(1) * 2) as u32;
        steps.push(HitStep::Item { what, k, a: 12.0, b: -3.0 });
    }
    steps
}

// ---- The corpus ---------------------------------------------------------------------------------------------------

#[test]
fn the_corpus_points_alike() {
    let mut run = Run::new("corpus");
    let pages = corpus_pages();
    assert!(pages.len() > 300, "widget-sandbox's corpus: {} pages", pages.len());
    let light = themed_master(false);
    let dark = themed_master(true);
    let mut r = Lcg::new(0x5eed_0016);
    for (name, text) in pages.iter() {
        let snap = snapshot(text);
        for (dark_theme, (master, vars)) in [(false, &light), (true, &dark)] {
            let html = themed_html(&snap, dark_theme, vars);
            let (width, height) = if dark_theme { (412.0, 915.0) } else { (375.0, 667.0) };
            let aim = Aim::new(width, height, master, &html, run.switches);
            let steps = pointer_steps(&mut r, &aim, 6, 10);
            let theme = if dark_theme { "dark" } else { "light" };
            run.case(width, height, master, &html, &steps, format!("{name} ({theme})"));
        }
    }
    let tally = run.finish();
    assert!(tally.count("points over an element") > 10000);
    assert!(tally.count("scrolls that moved") > 0);
}

/// The corpus again at narrower and wider viewports: light at 150 and 768 px, dark at 1440 and 240 px, with a media
/// change between and a coarser grid.
#[test]
fn the_corpus_points_alike_at_more_widths() {
    let mut run = Run::new("corpus at more widths");
    let pages = corpus_pages();
    let light = themed_master(false);
    let dark = themed_master(true);
    let mut r = Lcg::new(0x5eed_0116);
    for (name, text) in pages.iter() {
        let snap = snapshot(text);
        for (dark_theme, (master, vars)) in [(false, &light), (true, &dark)] {
            let html = themed_html(&snap, dark_theme, vars);
            let (width, other) = if dark_theme { (1440.0, 240.0) } else { (150.0, 768.0) };
            let aim = Aim::new(width, 900.0, master, &html, run.switches);
            let mut steps = pointer_steps(&mut r, &aim, 4, 6);
            steps.extend([HitStep::Media(other, 900.0), HitStep::Layout(other)]);
            let aim = Aim { width: other, ..aim };
            for (x, y) in aim.grid(4, 6) {
                steps.push(HitStep::At(At::root(1, x, y)));
                steps.push(HitStep::Over([x, y, x, y]));
            }
            steps.extend([HitStep::Layout(other), paint(other, 900.0, 0.0), HitStep::Dump]);
            let theme = if dark_theme { "dark" } else { "light" };
            run.case(width, 900.0, master, &html, &steps, format!("{name} ({theme}, {width} and {other} px)"));
        }
    }
    let tally = run.finish();
    assert!(tally.count("points over an element") > 10000);
}

/// The corpus's SVGs (each page that writes `<svg`, and every `<svg>…</svg>` the corpus writes alone in a page, as
/// the paint's comparison takes them; the renderer's snapshots name SVG's elements `x-unknown`): a fine grid over each
/// `<svg>` through `el_svg::element_at` and the document's walk.
#[test]
fn the_corpus_svgs_point_alike() {
    let mut run = Run::new("corpus SVGs");
    let texts = common::svg::corpus_texts();
    let mut documents: Vec<(String, Vec<u8>)> = texts
        .iter()
        .filter(|(name, text)| {
            name.ends_with(".html")
                && !name.contains("render-nested-")
                && text.len() < 300_000
                && text.windows(4).any(|w| w.eq_ignore_ascii_case(b"<svg"))
        })
        .cloned()
        .collect();
    for (name, snippet) in common::svg::corpus_svg_snippets(&texts) {
        let mut html = b"<!doctype html><html><body>".to_vec();
        html.extend_from_slice(&snippet);
        html.extend_from_slice(b"</body></html>");
        documents.push((name, html));
    }
    assert!(documents.len() > 250, "the corpus's SVGs are there ({})", documents.len());
    let (master, _) = themed_master(false);
    for (k, (name, html)) in documents.iter().enumerate() {
        let width = [375.0, 768.0][k % 2];
        let aim = Aim::new(width, 667.0, &master, html, run.switches);
        let mut steps = vec![HitStep::Layout(width)];
        for &(svg, b) in aim.svgs.iter().take(6) {
            for gy in 0..6 {
                for gx in 0..6 {
                    let x = b.x.value() + b.width.value() * (gx as f32 + 0.5) / 6.0;
                    let y = b.y.value() + b.height.value() * (gy as f32 + 0.5) / 6.0;
                    steps.push(HitStep::Svg { k: svg, at: None, x, y });
                    if (gx + gy) % 3 == 0 {
                        steps.push(HitStep::At(At::root(1, x, y)));
                    }
                }
            }
            steps.push(HitStep::Svg { k: svg, at: Some([10.0, 20.0, 50.0, 30.0]), x: 30.0, y: 30.0 });
            let (cx, cy) = Aim::centre(&b);
            steps.push(HitStep::Over([cx, cy, cx, cy]));
        }
        steps.push(HitStep::Leave);
        run.case(width, 667.0, &master, html, &steps, format!("{name} ({width} px)"));
    }
    let tally = run.finish();
    assert!(tally.count("svg points over a shape") > 1000, "{}", tally.report());
}

// ---- Written shapes -----------------------------------------------------------------------------------------------

fn page(sheet: &str, body: &str) -> Vec<u8> {
    format!("<!DOCTYPE html><html><head><style>{sheet}</style></head><body>{body}</body></html>").into_bytes()
}

/// Shapes for each branch of hit testing, written out: (what, sheet, body).
fn written_documents() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        (
            "overlapping z-indexes, negative ones too",
            "body{margin:0} div{width:120px;height:80px} .p{position:absolute} #a{left:10px;top:10px;z-index:2;background:red}
             #b{left:40px;top:30px;z-index:1;background:blue} #c{left:70px;top:50px;background:green}
             #d{left:20px;top:60px;z-index:-1;background:gray} #e{left:5px;top:5px;z-index:-2;width:300px;height:200px}
             #f{position:relative;z-index:0;top:100px} #g{position:absolute;z-index:-1;left:10px;top:10px;width:50px;height:30px}
             .n{position:relative;width:200px;height:60px;margin-top:150px} .n div{position:absolute;width:40px;height:40px}
             :hover{outline:1px solid red}",
            "<div class=p id=e></div><div class=p id=d></div><div class=p id=c>c</div><div class=p id=b>b</div><div class=p id=a>a</div>
             <div id=f>f<div id=g>g</div></div>
             <div class=n id=n><div id=n1 style=\"z-index:3;left:0\">1</div><div id=n2 style=\"z-index:-3;left:20px\">2</div>
             <span id=n3 style=\"position:relative;z-index:1\">inline</span><div id=n4 style=\"left:30px;z-index:auto\">4</div></div>",
        ),
        (
            "transforms",
            "body{margin:0} div{width:100px;height:60px;margin:10px;background:#ccc}
             #r{transform:rotate(30deg)} #s{transform:scale(0)} #k{transform:skewX(20deg) translate(10%,5px)}
             #m{transform:matrix(2,0,0,0.5,10,20);transform-origin:0 0} #x{transform:rotateX(90deg)} #y{transform:rotateY(60deg)}
             #o{transform:scale(1.5) rotate(-10deg);transform-origin:right bottom} #f{position:fixed;left:200px;top:300px;transform:rotate(45deg)}
             #n{transform:translate(20px,10px)} #n div{transform:rotate(90deg);width:40px;height:20px}
             #i{display:inline-block;transform:scale(2)}",
            "<div id=r>rotated</div><div id=s>zero</div><div id=k>skewed</div><div id=m>matrix</div><div id=x>x</div><div id=y>y</div>
             <div id=o>origin</div><div id=f>fixed</div><div id=n>outer<div id=n2>inner</div></div><p>a <span id=i>box</span> b</p>",
        ),
        (
            "overflow clips with radii",
            "body{margin:0} .c{width:120px;height:80px;padding:10px;border:5px solid;border-radius:30px;margin:10px}
             .c div{width:300px;height:300px;background:pink} #h{overflow:hidden} #cl{overflow:clip} #ct{contain:paint}
             #v{overflow:visible} #a{overflow:auto} .abs{position:absolute;left:100px;top:-20px;width:60px;height:60px}",
            "<div class=c id=h><div id=h1>hidden</div></div><div class=c id=cl><div id=c1>clip</div></div>
             <div class=c id=ct><div id=t1>contain</div><span class=abs id=t2>abs</span></div>
             <div class=c id=v><div id=v1>visible</div></div><div class=c id=a><div id=a1>auto</div></div>",
        ),
        (
            "pointer-events",
            "body{margin:0} div{width:150px;height:60px;margin:5px} #o{position:absolute;left:0;top:0;width:300px;height:300px;pointer-events:none}
             #o2{pointer-events:auto;width:50px;height:50px} #p{pointer-events:none} #p span{pointer-events:auto}
             #q{pointer-events:initial} #u{pointer-events:unset} #i{pointer-events:inherit} #w{pointer-events: none }",
            "<div id=base><button id=btn>button</button></div><div id=p>none <span id=ps>auto</span></div><div id=q>initial</div>
             <div id=w><div id=i>inherit</div><div id=u>unset</div></div>
             <div id=o>overlay<div id=o2>back</div></div>",
        ),
        (
            "clip paths",
            "body{margin:0} div{width:120px;height:80px;margin:10px;background:#abc} #c{clip-path:circle(30px at 50% 50%)}
             #i{clip-path:inset(10px 20px)} #p{clip-path:polygon(0 0,100% 0,50% 100%)} #e{clip-path:ellipse(40px 20px)}
             #n{clip-path:inset(5px);position:relative} #n div{position:absolute;left:60px;top:40px;width:80px;height:80px}
             #z{clip-path:polygon(0 0)}",
            "<div id=c>circle</div><div id=i>inset</div><div id=p>polygon</div><div id=e>ellipse</div>
             <div id=n>clip<div id=n1>child</div></div><div id=z>point</div>",
        ),
        (
            "fixed and sticky boxes",
            "body{margin:0;height:2000px} #f{position:fixed;top:0;left:0;right:0;height:40px;background:#333}
             #t{transform:translate(0,0);margin-top:100px;height:100px} #t div{position:fixed;top:10px;left:10px;width:50px;height:20px}
             #s{position:sticky;top:0;height:30px;background:yellow} #sc{overflow:auto;height:100px} #sc .st{position:sticky;top:5px;height:20px}
             #sc .tall{height:400px} #b{position:fixed;bottom:0;width:100px;height:30px;z-index:-1}",
            "<div id=f>header<a id=fa href=\"#top\">link</a></div><div id=s>sticky</div><div id=t>t<div id=tf>fixed in transform</div></div>
             <div id=sc><div class=st id=st>sticky in scroller</div><div class=tall>tall</div></div><div id=b>bottom</div>",
        ),
        (
            "scrolled and nested scrollers",
            "body{margin:0} .s{overflow:auto;width:200px;height:120px;border:4px solid;padding:6px} .tall{height:500px;width:400px}
             #in{overflow:scroll;width:150px;height:80px;margin:10px} #h{overflow-x:auto;overflow-y:hidden;white-space:nowrap}
             #hid{overflow:hidden;width:100px;height:50px} textarea{width:100px;height:40px}",
            "<div class=s id=out><div id=in><div class=tall id=deep>inner content</div></div><div class=tall>outer</div></div>
             <div class=s id=h>a very long line of text that does not wrap at all and goes on and on</div>
             <div id=hid><div class=tall>hidden but scrollable</div></div><textarea id=ta>a\nb\nc\nd\ne\nf\ng\nh</textarea>",
        ),
        (
            "tables, rows and cells",
            "body{margin:0} table{border-spacing:6px;border:2px solid} td{border:1px solid;padding:4px} .c table{border-collapse:collapse}
             caption{caption-side:bottom} #dr{display:table-row} #dc{display:table-cell;padding:5px} tr:hover{background:yellow}",
            "<table id=t><caption id=cap>caption</caption><thead><tr id=r0><th>h1</th><th>h2</th></tr></thead>
             <tbody><tr id=r1><td id=c11 rowspan=2>a</td><td id=c12>b</td></tr><tr id=r2><td id=c22>c</td></tr>
             <tr id=r3><td colspan=2 id=c31>wide</td></tr></tbody></table>
             <div class=c><table id=t2><tr id=r4><td id=c41>x</td><td id=c42>y</td></tr></table></div>
             <div style=\"display:table\"><div id=dr><div id=dc>cell</div><div id=dc2 style=\"display:table-cell\">two</div></div></div>",
        ),
        (
            "inline text across lines",
            "body{margin:0;width:160px;font-size:16px} a{color:blue} .ib{display:inline-block;width:40px;height:20px;background:#eee}
             .if{display:inline-flex} .it{display:inline-table} .ig{display:inline-grid} span:hover{text-decoration:underline}",
            "<p id=p>Some <span id=s1>text that wraps across <b id=b1>several lines of the</b> paragraph</span> and
             <a id=a1 href=\"https://example.org/x\">a link that wraps too</a> then <span class=ib id=ib>ib</span>
             <span class=if id=if><i>flex</i></span> <span class=it id=it>table</span> <span class=ig id=ig>grid</span><br id=br>end</p>
             <p><a id=a2 href=\"\">empty href</a> <a id=a3>no href <button id=bt>in</button></a> <label id=lb>label <i id=li>it</i></label></p>",
        ),
        (
            "floats",
            "body{margin:0} .l{float:left;width:60px;height:60px;background:red} .r{float:right;width:50px;height:90px}
             p{margin:0} .pf{float:left;position:relative;z-index:1;width:30px;height:30px}",
            "<div class=l id=l1>left</div><div class=r id=r1>right</div><p id=p1>text beside the floats that wraps around them and
             goes below <span class=l id=l2>inline float</span> more text</p><div class=pf id=pf>pos</div><div id=after>after</div>",
        ),
        (
            "SVG shapes and use",
            "body{margin:0} svg{display:block} .h:hover{fill:red}",
            "<svg id=s1 width=200 height=120 viewBox=\"0 0 100 60\"><rect id=rc x=5 y=5 width=30 height=20 class=h/>
             <circle id=ci cx=60 cy=20 r=10/><ellipse id=el cx=80 cy=45 rx=12 ry=6/><line id=ln x1=0 y1=58 x2=100 y2=58 stroke=black stroke-width=4/>
             <polyline id=pl points=\"5,40 15,50 25,40\" fill=none stroke=blue/><polygon id=pg points=\"40,40 50,55 30,55\"/>
             <path id=pa d=\"M70 5 h20 v10 z\"/><path id=pz d=\"Z\"/><path id=pm d=\"M\"/><path id=p1 d=\"M5 5\"/><text id=tx x=5 y=35 font-size=8>text<tspan id=ts dx=2>span</tspan></text>
             <g id=g1 transform=\"translate(50 0) scale(0.5)\"><rect id=gr x=0 y=80 width=40 height=20/></g>
             <rect id=hid x=0 y=0 width=100 height=60 visibility=hidden/><rect id=none x=0 y=0 width=10 height=10 display=none/></svg>
             <svg id=s2 width=150 height=150 viewBox=\"0 0 50 100\" preserveAspectRatio=\"xMaxYMin slice\">
             <defs><symbol id=sy viewBox=\"0 0 10 10\"><circle id=syc cx=5 cy=5 r=4/></symbol><rect id=dr width=10 height=10/></defs>
             <use id=u1 href=\"#sy\" x=5 y=5 width=20 height=20/><use id=u2 href=\"#dr\" x=30 y=60/><use id=u3 href=\"#g1\" x=0 y=0/>
             <svg id=in x=10 y=40 width=20 height=20><rect id=inr width=20 height=20/></svg><g opacity=0.5><rect id=op x=0 y=80 width=50 height=10 stroke-width=6 stroke=red/></g></svg>",
        ),
        (
            "form controls",
            "body{margin:0} input,select,textarea,button,meter{display:block;margin:4px} fieldset{width:200px}
             button:active{background:red} input:hover{border-color:blue}",
            "<input id=in value=text><input id=cb type=checkbox><input id=rg type=range><button id=bt>press <b id=bb>bold</b></button>
             <select id=se><option>one</option><option>two</option></select><textarea id=ta>area</textarea><meter id=me value=0.5></meter>
             <fieldset id=fs><legend id=lg>legend</legend><label id=lb for=in>label</label></fieldset>",
        ),
        (
            "right to left",
            "body{margin:0;direction:rtl;width:200px} .b{width:80px;height:30px;background:#ddd} .l{float:left;width:40px;height:40px}
             .ab{position:absolute;right:10px;top:100px;width:30px;height:30px}",
            "<div class=b id=b1>rtl</div><p id=p1>نص عربي <span id=s1>with english</span> mixed across a line that wraps</p>
             <div class=l id=l1>f</div><div class=ab id=ab>abs</div><div dir=ltr id=ltr>ltr inside <span id=s2>span</span></div>",
        ),
        (
            "hover and active restyles that move things",
            "body{margin:0} #a{width:100px;height:40px;background:#eee} #a:hover{width:200px;height:80px;cursor:pointer}
             #b:active{display:none} #c:hover::before{content:'before ';cursor:text} #d{cursor:  move } #d:hover #e{display:block}
             #e{display:none;position:absolute;left:0;top:200px;width:100px;height:50px;background:red} li:hover{margin-left:20px}",
            "<div id=a>grows</div><div id=b>hides when pressed</div><div id=c>gets content</div><div id=d>shows a box<div id=e>shown</div></div>
             <ul><li id=l1>one</li><li id=l2>two</li></ul>",
        ),
        (
            "positioned boxes a static box holds",
            "body{margin:0} #t{transform:translate(0);height:20px} #c{contain:paint;height:20px} .a{position:absolute;width:60px;height:60px}
             #z{position:relative;z-index:1;height:20px} #zt{transform:scale(1);height:10px}",
            "<div id=t><div class=a id=ta style=\"left:200px;top:200px\">abs in a transform</div></div>
             <div id=c><div class=a id=ca style=\"left:200px;top:300px;z-index:1\">abs in contain</div></div>
             <div id=z>z<div id=zt><div class=a id=za style=\"left:100px;top:100px;z-index:1\">same z</div><div class=a id=zb style=\"left:250px;top:100px;z-index:5\">other z</div></div></div>",
        ),
        (
            "scrollers with nothing to scroll and sticky boxes scrolled",
            "body{margin:0} #e{overflow:auto;width:100px;height:50px} #z{overflow:scroll;width:0;height:0;padding:0}
             #s{overflow:auto;height:120px;padding-top:10px} .st{position:sticky;top:0;bottom:4px;height:20px;background:#eee} .tall{height:600px}",
            "<div id=e></div><div id=z></div><div id=s><div class=tall>a</div><div class=st id=st>sticky</div><div class=tall>b</div></div>",
        ),
        (
            "grid and flex items",
            "body{margin:0} #g{display:grid;grid-template-columns:50px 100px;gap:5px} #g div{height:30px;background:#ccc}
             #f{display:flex;flex-wrap:wrap;width:150px} #f div{width:60px;height:25px;margin:2px} #o{order:-1}",
            "<div id=g><div id=g1>1</div><div id=g2>2</div><div id=g3>3</div></div><div id=f><div id=f1>a</div><div id=o>b</div>text<div id=f3>c</div></div>",
        ),
    ]
}

/// The steps of a written shape: the grid, a finer one, each element's centre and corners hovered, pressed and
/// released, and the generic pointer steps.
fn written_steps(r: &mut Lcg, aim: &Aim) -> Vec<HitStep> {
    let mut steps = pointer_steps(r, aim, 8, 12);
    for (x, y) in aim.grid(16, 20) {
        steps.push(HitStep::At(At::root(1, x, y)));
        steps.push(HitStep::At(At { client_x: x + 7.0, client_y: y - 33.0, ..At::root(0, x, y) }));
    }
    for (_, &(k, b)) in aim.ids.iter() {
        let (cx, cy) = Aim::centre(&b);
        steps.push(HitStep::Over([cx, cy, cx, cy]));
        steps.push(HitStep::At(At::within(k, cx, cy)));
        steps.push(HitStep::At(At::within(k, b.x.value() + 0.5, b.y.value() + 0.5)));
        steps.push(HitStep::Item { what: 1, k, a: 7.0, b: 0.0 });
        steps.push(HitStep::Item { what: 2, k, a: -7.0, b: 0.0 });
        if let Some(&item) = aim.item_of.get(&k) {
            steps.push(HitStep::Item { what: 6, k: item, a: cx, b: cy });
            steps.push(HitStep::Item { what: 6, k: item, a: b.x.value() + 2.0, b: b.y.value() + 6.0 });
            steps.push(HitStep::At(At { check: 5, from: 2, k: item, ..At::root(5, cx, cy) }));
        }
        steps.push(HitStep::Down([cx, cy, cx, cy]));
        steps.push(HitStep::Up([cx, cy, cx, cy]));
    }
    steps.extend([HitStep::Layout(aim.width), paint(aim.width, aim.height, 0.0), HitStep::Dump, HitStep::Leave]);
    steps
}

/// A document pointed at while patched: what it is, its HTML and the HTML of each patch.
type Remap = (&'static str, Vec<u8>, Vec<Vec<u8>>);

/// Patches while an element is hovered and pressed: `update_structure` moves both to what stays attached.
fn remap_cases() -> Vec<Remap> {
    let sheet = "body{margin:0} div{padding:4px;margin:2px;min-height:20px} .x:hover{background:red} .y:active{color:blue}";
    let base = "<div id=o class=x><div id=m class=y><div id=i class=x>inner</div></div><div id=s>sibling</div></div>";
    vec![
        ("the hovered element removed", page(sheet, base), vec![page(sheet, "<div id=o class=x><div id=s>sibling</div></div>")]),
        (
            "the hovered element kept, its siblings changed",
            page(sheet, base),
            vec![page(sheet, "<div id=o class=x><div id=m class=y><div id=i class=x>inner</div></div><div id=s>sib</div><p>new</p></div>")],
        ),
        ("everything replaced", page(sheet, base), vec![page(sheet, "<p id=n>new</p>")]),
        (
            "a retag and a text, then a removal",
            page(sheet, base),
            vec![
                page(
                    sheet,
                    "<div id=o class=x><section id=m class=y><div id=i class=x>inner text</div></section><div id=s>sibling</div></div>",
                ),
                page(sheet, "<div id=o class=x></div>"),
            ],
        ),
    ]
}

#[test]
fn written_shapes_point_alike() {
    let mut run = Run::new("written shapes");
    let master = master_css();
    let (light, _) = themed_master(false);
    let mut r = Lcg::new(0x5eed_1616);
    for (what, sheet, body) in written_documents() {
        let html = page(sheet, body);
        for (width, height, m) in [(375.0, 667.0, &master), (240.0, 400.0, &light)] {
            let aim = Aim::new(width, height, m, &html, run.switches);
            let steps = written_steps(&mut r, &aim);
            run.case(width, height, m, &html, &steps, format!("{what} at {width} px"));
        }
    }
    // Sticky boxes, which only a trusted sheet may declare (the renderer's sanitizer drops `position: sticky` from the
    // document's): in the page and in scrollers, scrolled.
    let sticky_master = [&master[..], b" .st{position:sticky;top:0;bottom:4px} .sl{position:sticky;left:5px}"].concat();
    let sticky_html = page(
        "body{margin:0} #s{overflow:auto;height:120px;padding-top:10px} .st{height:20px;background:#eee} .tall{height:600px}
         #h{overflow:auto;width:150px;white-space:nowrap} .sl{display:inline-block;width:40px}",
        "<div class=st id=top>page sticky</div><div id=s><div class=tall>a</div><div class=st id=st>sticky</div><div class=tall>b</div></div>
         <div id=h><span class=sl id=sl>left</span> a long line that scrolls sideways past the end of its box</div><div class=tall></div>",
    );
    for width in [375.0, 200.0] {
        let aim = Aim::new(width, 400.0, &sticky_master, &sticky_html, run.switches);
        let steps = written_steps(&mut r, &aim);
        run.case(width, 400.0, &sticky_master, &sticky_html, &steps, format!("sticky boxes from the master sheet at {width} px"));
    }
    // Documents without a render tree (a root of display none): the pointer finds no document.
    for html in [
        &b"<!DOCTYPE html><html style=\"display:none\"><body><p id=p>x</p></body></html>"[..],
        b"<html style=\"display:none\"><a href=x>y</a></html>",
    ] {
        let p = [10.0, 10.0, 10.0, 10.0];
        let steps = vec![
            HitStep::Layout(375.0),
            HitStep::At(At::root(1, 10.0, 10.0)),
            HitStep::Over(p),
            HitStep::Down(p),
            HitStep::Up(p),
            HitStep::Cancel,
            HitStep::Leave,
            HitStep::Scroll([0.0, 10.0, 10.0, 10.0, 10.0, 10.0]),
            HitStep::Item { what: 1, k: 3, a: 5.0, b: 0.0 },
            HitStep::Item { what: 6, k: 0, a: 5.0, b: 5.0 },
            HitStep::Dump,
        ];
        run.case(375.0, 667.0, &master, html, &steps, format!("no render tree: {}", String::from_utf8_lossy(html)));
    }
    for (what, html, patches) in remap_cases() {
        let aim = Aim::new(375.0, 667.0, &master, &html, run.switches);
        for target in ["i", "m", "s", "o"] {
            let Some(&(_, b)) = aim.ids.get(target.as_bytes()) else { continue };
            let (cx, cy) = Aim::centre(&b);
            let mut steps = vec![HitStep::Layout(375.0), HitStep::Over([cx, cy, cx, cy]), HitStep::Down([cx, cy, cx, cy]), HitStep::Dump];
            for patch in &patches {
                steps.extend([HitStep::Html(patch.clone()), HitStep::Layout(375.0), HitStep::Dump, HitStep::Over([cx, cy, cx, cy])]);
            }
            steps.extend([HitStep::Up([cx, cy, cx, cy]), HitStep::Dump, HitStep::Leave, HitStep::Dump]);
            run.case(375.0, 667.0, &master, &html, &steps, format!("{what}, pointer on #{target}"));
        }
    }
    let tally = run.finish();
    for name in [
        "anchor clicks",
        "element clicks taken",
        "clip path points inside",
        "clip path points outside",
        "scrolls that moved",
        "svg points over a shape",
        "checks refused",
        "update_from_string patched",
    ] {
        assert!(tally.count(name) > 0, "no {name}: {}", tally.report());
    }
}

// ---- Generated documents ------------------------------------------------------------------------------------------

fn pick<'a>(r: &mut Lcg, from: &[&'a str]) -> &'a str {
    from[r.below(from.len())]
}

/// A declaration hit testing reads.
fn hit_declaration(r: &mut Lcg) -> String {
    let n = r.below(40) as i32 - 5;
    match r.below(22) {
        0 => format!("position:{}", pick(r, &["static", "relative", "absolute", "fixed", "sticky"])),
        1 => format!("z-index:{}", pick(r, &["auto", "0", "1", "2", "-1", "-2", "10"])),
        2 => format!("left:{n}px;top:{}px", n * 2),
        3 => format!(
            "transform:{}",
            pick(
                r,
                &[
                    "rotate(20deg)",
                    "scale(0)",
                    "scale(1.5,0.5)",
                    "translate(10px,-5px)",
                    "skew(10deg,5deg)",
                    "matrix(1,0.2,0.3,1,4,5)",
                    "rotateX(90deg)"
                ]
            )
        ),
        4 => format!("overflow:{}", pick(r, &["hidden", "auto", "scroll", "clip", "visible"])),
        5 => format!("clip-path:{}", pick(r, &["circle(20px)", "inset(5px 10px)", "polygon(0 0,100% 0,0 100%)", "ellipse(30% 20%)"])),
        6 => format!("pointer-events:{}", pick(r, &["none", "auto", "inherit", "initial", "unset"])),
        7 => format!("float:{}", pick(r, &["left", "right", "none"])),
        8 => format!(
            "display:{}",
            pick(r, &["block", "inline", "inline-block", "flex", "inline-flex", "grid", "table", "table-cell", "none", "inline-table"])
        ),
        9 => format!("width:{}px", 20 + r.below(200)),
        10 => format!("height:{}px", 10 + r.below(150)),
        11 => format!("margin:{n}px"),
        12 => format!("padding:{}px", r.below(12)),
        13 => format!("border:{}px solid", r.below(6)),
        14 => format!("border-radius:{}px", r.below(30)),
        15 => format!("visibility:{}", pick(r, &["visible", "hidden"])),
        16 => format!("cursor:{}", pick(r, &["pointer", "text", "move", "default"])),
        17 => format!("transform-origin:{}", pick(r, &["0 0", "100% 50%", "left bottom"])),
        18 => format!("contain:{}", pick(r, &["paint", "none"])),
        19 => format!("direction:{}", pick(r, &["rtl", "ltr"])),
        20 => format!("white-space:{}", pick(r, &["nowrap", "normal"])),
        _ => format!("opacity:{}", pick(r, &["0.5", "1", "0"])),
    }
}

fn hit_element(r: &mut Lcg, out: &mut String, depth: u32, id: &mut u32) {
    let tag = pick(r, &["div", "div", "span", "p", "a", "b", "section", "button", "table", "ul", "img", "svg", "input"]);
    *id += 1;
    let mut style = String::new();
    for _ in 0..r.below(5) {
        style += &hit_declaration(r);
        style.push(';');
    }
    let class = pick(r, &["a", "b", "c", ""]);
    match tag {
        "img" => {
            out.push_str(&format!("<img id=e{id} class=\"{class}\" style=\"{style}\" src=\"x{}.png\" width=30 height=20>", r.below(5)));
            return;
        }
        "input" => {
            out.push_str(&format!("<input id=e{id} style=\"{style}\" value=v>"));
            return;
        }
        "svg" => {
            out.push_str(&format!(
                "<svg id=e{id} style=\"{style}\" width=60 height=40 viewBox=\"0 0 30 20\"><rect id=e{id}r x=2 y=2 width={} height=8/><circle id=e{id}c cx=20 cy=12 r={} stroke-width=3 stroke=red/><use href=\"#e{id}r\" x=5 y=8/></svg>",
                r.below(20),
                r.below(8)
            ));
            return;
        }
        "table" => {
            out.push_str(&format!(
                "<table id=e{id} style=\"{style}\"><tr><td id=e{id}a>a</td><td id=e{id}b rowspan=2>b</td></tr><tr><td>c d e</td></tr></table>"
            ));
            return;
        }
        _ => {}
    }
    let attrs = if tag == "a" && r.chance(2) { " href=\"https://x.test/\"" } else { "" };
    out.push_str(&format!("<{tag} id=e{id} class=\"{class}\" style=\"{style}\"{attrs}>"));
    if r.chance(2) {
        out.push_str(pick(r, &["text ", "some longer words that wrap ", "x ", "عربي "]));
    }
    if depth < 4 {
        for _ in 0..r.below(4) {
            hit_element(r, out, depth + 1, id);
        }
    }
    if r.chance(3) {
        out.push_str("tail");
    }
    out.push_str(&format!("</{tag}>"));
}

fn hit_document(r: &mut Lcg) -> Vec<u8> {
    let mut sheet = String::from("body{margin:0}");
    for class in ["a", "b", "c"] {
        let state = pick(r, &["", ":hover", ":active"]);
        sheet += &format!(".{class}{state}{{");
        for _ in 0..1 + r.below(3) {
            sheet += &hit_declaration(r);
            sheet.push(';');
        }
        sheet.push('}');
    }
    let mut body = String::new();
    let mut id = 0;
    for _ in 0..1 + r.below(5) {
        hit_element(r, &mut body, 0, &mut id);
    }
    page(&sheet, &body)
}

/// Steps of a generated document: a grid with and without the check, a few hovers and presses, every scroller, the
/// svgs, then, while an element may be hovered or active, a style patch and an element added (the hovered and active
/// elements remapped), each relayout and pointed at again, and a dump and paint after.
fn generated_steps(r: &mut Lcg, aim: &Aim, html: &[u8]) -> Vec<HitStep> {
    let mut steps = vec![HitStep::Layout(aim.width), paint(aim.width, aim.height, 0.0)];
    for (x, y) in aim.grid(5, 8) {
        steps.push(HitStep::At(At::root(r.below(2) as u8, x, y)));
    }
    for _ in 0..6 {
        if aim.elements.is_empty() {
            break;
        }
        let (k, b) = aim.elements[r.below(aim.elements.len())];
        let (cx, cy) = Aim::centre(&b);
        let p = [cx, cy, cx, cy];
        steps.push(match r.below(5) {
            0 => HitStep::Down(p),
            1 => HitStep::Up(p),
            2 => HitStep::At(At::within(k, cx, cy)),
            3 => HitStep::Item { what: 6, k: r.below(aim.items.max(1)) as u32, a: cx, b: cy },
            _ => HitStep::Over(p),
        });
    }
    for &(item, _, b) in aim.scrollers.iter().take(2) {
        let (cx, cy) = Aim::centre(&b);
        steps.push(HitStep::Scroll([r.below(60) as f32 - 20.0, r.below(80) as f32 - 20.0, cx, cy, cx, cy]));
        steps.push(HitStep::Item { what: 0, k: item, a: 5.0, b: 5.0 });
    }
    for &(k, b) in aim.svgs.iter().take(2) {
        let (cx, cy) = Aim::centre(&b);
        steps.push(HitStep::Svg { k, at: None, x: cx, y: cy });
        steps.push(HitStep::Svg { k, at: None, x: b.x.value() + 3.0, y: b.y.value() + 3.0 });
    }
    if let Some(&(k, b)) = aim.elements.get(r.below(aim.elements.len().max(1))) {
        let (cx, cy) = Aim::centre(&b);
        steps.push(HitStep::Styles(vec![(k, b"outline:1px solid;width:40px".to_vec())]));
        steps.extend([HitStep::Layout(aim.width), HitStep::Over([cx, cy, cx, cy]), HitStep::At(At::root(1, cx, cy))]);
    }
    if let Some(at) = html.windows(7).rposition(|w| w == b"</body>") {
        let added = b"<div id=added style=\"position:absolute;left:5px;top:5px;width:30px;height:30px\">added</div>";
        let patched = [&html[..at], &added[..], &html[at..]].concat();
        steps.extend([
            HitStep::Html(patched),
            HitStep::Layout(aim.width),
            HitStep::Over([10.0, 10.0, 10.0, 10.0]),
            HitStep::Up([10.0, 10.0, 10.0, 10.0]),
        ]);
        if r.chance(3) {
            // Everything the pointer was over replaced.
            let body = html.windows(6).position(|w| w == b"<body>").map_or(0, |b| b + 6);
            let replaced = [&html[..body], &b"<p id=only>only</p>"[..], &html[at..]].concat();
            steps.extend([HitStep::Down([10.0, 10.0, 10.0, 10.0]), HitStep::Html(replaced), HitStep::Layout(aim.width), HitStep::Dump]);
        }
    }
    if r.chance(3) {
        steps.push(HitStep::Cancel);
    }
    steps.extend([HitStep::Layout(aim.width), paint(aim.width, aim.height, 0.0), HitStep::Dump, HitStep::Leave]);
    steps
}

#[test]
fn generated_documents_point_alike() {
    let mut run = Run::new("generated documents");
    let master = master_css();
    let mut r = Lcg::new(0x5eed_2516);
    for k in 0..2500 {
        let html = hit_document(&mut r);
        let width = [375.0, 300.0, 520.0][k % 3];
        let aim = Aim::new(width, 600.0, &master, &html, run.switches);
        let steps = generated_steps(&mut r, &aim, &html);
        run.case(width, 600.0, &master, &html, &steps, format!("generated {k}: {}", String::from_utf8_lossy(&html)));
    }
    let tally = run.finish();
    assert!(tally.count("points over an element") > 10000);
}

/// The layout switches and the transformed culling's over written shapes and generated documents.
#[test]
fn the_switches_point_alike() {
    let master = master_css();
    for switch in [
        "LH_NO_LAYOUT_CACHE",
        "LH_NO_POSITIONED_REUSE",
        "LH_NO_FIXED_LAYOUT_REUSE",
        "LH_LAYOUT_CACHE_CHECK",
        "LH_NO_COLUMN_REUSE",
        "LH_NO_TRANSFORMED_CULLING",
    ] {
        let mut run = Run::with_switch(switch, switch);
        let mut r = Lcg::new(0x5eed_3016);
        for (what, sheet, body) in written_documents() {
            let html = page(sheet, body);
            let aim = Aim::new(375.0, 667.0, &master, &html, run.switches);
            let steps = generated_steps(&mut r, &aim, &html);
            run.case(375.0, 667.0, &master, &html, &steps, format!("{what} under {switch}"));
        }
        for k in 0..120 {
            let html = hit_document(&mut r);
            let aim = Aim::new(375.0, 600.0, &master, &html, run.switches);
            let steps = generated_steps(&mut r, &aim, &html);
            run.case(375.0, 600.0, &master, &html, &steps, format!("generated {k} under {switch}"));
        }
        let tally = run.finish();
        assert!(tally.count("points over an element") > 1000, "{switch}");
    }
}

/// A document 124 levels deep of positioned, transformed, clipped, floating and inline boxes is pointed at on a thread
/// whose stack is the size the layout's measures asked (1 MiB unoptimized, 512 KiB optimized).
#[test]
fn a_deep_document_points_on_a_small_stack() {
    let stack = if cfg!(debug_assertions) { 1 << 20 } else { 512 << 10 };
    let thread = std::thread::Builder::new().stack_size(stack).spawn(|| {
        let mut html = String::from("<!DOCTYPE html><html><head></head><body style=\"margin:0\">");
        let opens = [
            "<div style=\"position:relative;z-index:1;padding-left:1px\">a ",
            "<div style=\"overflow:hidden;border-radius:2px\">b ",
            "<span style=\"display:inline-block\">c ",
            "<div style=\"float:left\">d ",
            "<div style=\"position:relative;z-index:3;transform:translate(1px)\">e ",
            "<div style=\"clip-path:inset(0)\">f ",
            "<span>g ",
            "<div style=\"position:relative;left:1px;top:1px;z-index:2\">h ",
        ];
        let closes = ["</div>", "</div>", "</span>", "</div>", "</div>", "</div>", "</span>", "</div>"];
        for k in 0..124 {
            html += opens[k % 8];
        }
        html += "<div id=deep style=\"width:50px;height:50px\">deep</div>";
        for k in (0..124).rev() {
            html += closes[k % 8];
        }
        html += "</body></html>";
        let mut run = Run::new("a deep document");
        let master = master_css();
        let mut steps = vec![HitStep::Layout(375.0)];
        let aim = Aim::new(375.0, 667.0, &master, html.as_bytes(), Switches::default());
        let deep = aim.ids.get(&b"deep"[..]).map_or((10.0, 10.0), |&(_, b)| Aim::centre(&b));
        for (x, y) in [deep, (5.0, 5.0), (20.0, 20.0), (60.0, 30.0), (100.0, 100.0)] {
            steps.push(HitStep::At(At::root(1, x, y)));
            steps.push(HitStep::At(At::root(4, x, y)));
            steps.push(HitStep::Over([x, y, x, y]));
            steps.push(HitStep::Down([x, y, x, y]));
            steps.push(HitStep::Up([x, y, x, y]));
        }
        steps.push(HitStep::Scroll([0.0, 10.0, 10.0, 10.0, 10.0, 10.0]));
        steps.push(HitStep::Leave);
        run.case(375.0, 667.0, &master, html.as_bytes(), &steps, "124 levels");
        let tally = run.finish();
        // The deepest box takes its point: what is over it is the last element of the walk.
        assert!(tally.count("hits on el_div") > 0);
        tally.count("points over an element")
    });
    let hits = thread.expect("a thread").join().expect("the hit tests ran");
    assert!(hits > 0);
}

/// An `<svg>` 124 groups deep, its text and `<use>`s of the groups at the bottom, is pointed at through `element_at`
/// and the document's walk on the same small stacks.
#[test]
fn a_deep_svg_is_pointed_at_on_a_small_stack() {
    let stack = if cfg!(debug_assertions) { 1 << 20 } else { 512 << 10 };
    let thread = std::thread::Builder::new().stack_size(stack).spawn(|| {
        let mut html = String::from("<!DOCTYPE html><html><head></head><body style=\"margin:0\"><svg id=s width=\"200\" height=\"100\">");
        for k in 0..124 {
            html += &format!("<g transform=\"translate(1 0)\" opacity=\".99\" id=\"d{k}\">");
        }
        html += "<rect id=r x=\"2\" y=\"2\" width=\"20\" height=\"20\"/><text x=\"1\" y=\"40\">deep<tspan dx=\"1\">t</tspan></text>";
        html += &"</g>".repeat(124);
        html += "<use href=\"#d0\" y=\"50\"/><use href=\"#d3\" x=\"5\"/></svg></body></html>";
        let mut run = Run::new("a deep svg");
        let master = master_css();
        let aim = Aim::new(375.0, 667.0, &master, html.as_bytes(), Switches::default());
        let svg = aim.svgs.first().map_or(0, |s| s.0);
        let mut steps = vec![HitStep::Layout(375.0)];
        for (x, y) in [(130.0, 10.0), (134.0, 37.0), (130.0, 62.0), (180.0, 90.0), (3.0, 3.0)] {
            steps.push(HitStep::Svg { k: svg, at: None, x, y });
            steps.push(HitStep::At(At::root(1, x, y)));
        }
        steps.push(HitStep::Over([130.0, 10.0, 130.0, 10.0]));
        steps.push(HitStep::Leave);
        run.case(375.0, 667.0, &master, html.as_bytes(), &steps, "124 groups");
        run.finish().count("svg points over a shape")
    });
    let hits = thread.expect("a thread").join().expect("the hit tests ran");
    assert!(hits >= 3, "{hits} svg hits");
}
