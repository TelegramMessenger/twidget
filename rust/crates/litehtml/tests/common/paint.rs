//! The Rust side of lhdump's `Q` record (stage L13, painting): a `Y` record whose steps may also paint (`p`), with
//! every paint call the container gets logged among its other calls, and whose dump steps dump what painting keeps.
//!
//! A paint step: f32 x, y, u8 flags (1 a clip, 2 cull, 4 trace, 8 controls active, 16 bare shadows, 32 `draw_cached`
//! answering as a cache, 64 then each `<svg>`'s `paint_shapes` at its placement), the clip (4 f32),
//! f32 snap scale, u32 hdc, the elements skipped, drawn without background images and without scroll content (u32
//! count, each u32 k: the k-th element of the cascade's walk), i32 the element with bare effects and the unclipped
//! scroller (-1 none), u8 r, g, b, a the controls' accent. Answered with u8 1 and, when tracing, u32 items drawn and
//! each i32 (the render tree's numbering).
//!
//! A dump: u32 the paint generation, u8 culling; u32 items, each u32 its ink's generation, its ink and drawn ink (4
//! f32, u8 empty, u8 unbounded) and its visual transform (6 f32); u32 DOM elements, each u8 class, a control's look as
//! its last paint left it (u8 active, u8 r, g, b, a accent), u64 its paint signature, what an `<svg>` kept (u64 last
//! paint key, i32 stable draws, u64 content key, u32 its generation, u8 cacheable, u8 groups eligible, u32 the
//! `<use>` budget's generation, i32 its allowance) and an SVG `<text>`'s box (u8 found, then 4 f32); then u32 the
//! generation of the document's `<use>` budget and i32 what is left of it.
//!
//! The paint calls (u8 kind, text, arguments, u64 hdc; every float its bits, every NaN as one since which NaN comes out
//! where two meet is the compiler's choice, as svg_oracle writes them; a colour u8 r, g, b, a, current, a box 4 f32,
//! radii 8 f32): `t` text (u64 font, colour, box), `k` list marker (image; u8 base url given, base url, i32 type,
//! colour, box, i32 index, u64 font), `g` image (url; layer, base url), `f` solid fill (layer, colour), `l`, `r`, `c`
//! linear, radial and conic gradients (layer, stops, then end points, centre and radii, centre, angle and reach), `d`
//! borders (each side's f32 width, i32 style, colour, the radii, the box, u8 root), `C` clip (box, radii), `D` clip's
//! end, `U` and `R` clips suspended and resumed, `h` box shadow (box, radii, f32 x, y, blur, spread, colour, u8
//! inset), `P` clip path (a path), `G` group (f32 opacity, matrix, u8 backface hidden, bounds, u8 exact, i32 owner),
//! `X` filtered (a filter paint) then the paint into hdc * 2 + 1 then `x`, `K` backdrop (a filter paint), `p` path,
//! `E` group's end, `A` cached (box, u64 key; answered false, or while caching true, a key not drawn before painted
//! into hdc * 2 + 1 then `a`), `O` isolated (box) then the paint into hdc * 2 + 1 then `o`, `Q` controls active, `q`
//! the accent. A layer is its border box, radii, clip box, origin box, i32 attachment, i32 repeat, u8 root. The port's
//! seam (a later stage's paint) is `Z` with the stage's name: an image stands for the C++'s one `g`.

use std::collections::{BTreeMap, BTreeSet};

use super::cascade::*;
use super::svg::Answer;
use super::*;
use twidget_litehtml::background::{BackgroundLayer, ConicGradient, GradientBase, LinearGradient, RadialGradient};
use twidget_litehtml::borders::{BorderRadiuses, Borders};
use twidget_litehtml::css_effects::{FilterPaint, PaintGroup, ShadowPaint, SvgGradient, VectorPath};
use twidget_litehtml::document::{Document, StyleCx, Switches};
use twidget_litehtml::document_container::ListMarker;
use twidget_litehtml::el_svg::{DocSvg, SvgElements};
use twidget_litehtml::element::{ElementId, ElementKind};
use twidget_litehtml::html::lowcase;
use twidget_litehtml::render_item::RenderId;
use twidget_litehtml::types::Position;
use twidget_litehtml::web_color::WebColor;

/// A paint step's settings.
#[derive(Clone, Debug)]
pub struct PaintStep {
    pub x: f32,
    pub y: f32,
    pub clip: Option<[f32; 4]>,
    pub cull: bool,
    pub trace: bool,
    pub active: bool,
    pub bare_shadows: bool,
    /// `draw_cached` answers as a cache.
    pub cache: bool,
    /// Each `<svg>`'s `paint_shapes` at its placement after the paint.
    pub shapes: bool,
    pub snap: f32,
    pub hdc: u32,
    pub skip: Vec<u32>,
    pub skip_backgrounds: Vec<u32>,
    pub skip_scroll: Vec<u32>,
    pub bare: i32,
    pub unclipped: i32,
    pub accent: [u8; 4],
}

impl PaintStep {
    /// A paint of the viewport as the renderer paints: at 0, 0, clipped to the viewport, not culled.
    pub fn viewport(width: f32, height: f32) -> PaintStep {
        PaintStep {
            x: 0.0,
            y: 0.0,
            clip: Some([0.0, 0.0, width, height]),
            cull: false,
            trace: false,
            active: false,
            bare_shadows: false,
            cache: false,
            shapes: false,
            snap: 1.0,
            hdc: 7,
            skip: Vec::new(),
            skip_backgrounds: Vec::new(),
            skip_scroll: Vec::new(),
            bare: -1,
            unclipped: -1,
            accent: [0, 0, 0, 0],
        }
    }

    /// The step's bytes after its `p`.
    pub fn write(&self, o: &mut Out) {
        let flags = u8::from(self.clip.is_some())
            | u8::from(self.cull) << 1
            | u8::from(self.trace) << 2
            | u8::from(self.active) << 3
            | u8::from(self.bare_shadows) << 4
            | u8::from(self.cache) << 5
            | u8::from(self.shapes) << 6;
        o.f32(self.x).f32(self.y).u8(flags);
        if let Some(c) = self.clip {
            o.f32(c[0]).f32(c[1]).f32(c[2]).f32(c[3]);
        }
        o.f32(self.snap).u32(self.hdc);
        for list in [&self.skip, &self.skip_backgrounds, &self.skip_scroll] {
            o.u32(list.len() as u32);
            for &k in list {
                o.u32(k);
            }
        }
        o.i32(self.bare).i32(self.unclipped);
        for v in self.accent {
            o.u8(v);
        }
    }
}

// ---- The container's paint calls, as lhdump_paint.h logs them ------------------------------------------------

pub fn pos_out(o: &mut Out, p: &Position) {
    o.a32(p.x.value()).a32(p.y.value()).a32(p.width.value()).a32(p.height.value());
}

pub fn radius_out(o: &mut Out, r: &BorderRadiuses) {
    for v in
        [r.top_left_x, r.top_left_y, r.top_right_x, r.top_right_y, r.bottom_right_x, r.bottom_right_y, r.bottom_left_x, r.bottom_left_y]
    {
        o.a32(v.value());
    }
}

pub fn layer_out(o: &mut Out, l: &BackgroundLayer) {
    pos_out(o, &l.border_box);
    radius_out(o, &l.border_radius);
    pos_out(o, &l.clip_box);
    pos_out(o, &l.origin_box);
    o.i32(l.attachment).i32(l.repeat).u8(u8::from(l.is_root));
}

pub fn points_out(o: &mut Out, g: &GradientBase) {
    o.u32(g.color_points.len() as u32);
    for p in &g.color_points {
        o.a32(p.offset).color(&p.color).u8(u8::from(p.hint.is_some())).a32(p.hint.unwrap_or(0.0));
    }
    o.i32(g.color_space.index()).i32(g.hue_interpolation.index());
}

pub fn borders_out(o: &mut Out, b: &Borders) {
    for side in [&b.left, &b.top, &b.right, &b.bottom] {
        o.a32(side.width.value()).i32(side.style).color(&side.color);
    }
    radius_out(o, &b.radius);
}

fn length_out(o: &mut Out, len: &CssLength) {
    o.u8(u8::from(len.is_predefined())).i32(len.predef()).u8(len.units().index() as u8).a32(len.val());
}

fn svg_gradient_out(o: &mut Out, g: &SvgGradient) {
    o.i32(g.ty);
    for v in [g.x1, g.y1, g.x2, g.y2, g.r, g.fx, g.fy] {
        o.a32(v);
    }
    o.u32(g.stops.len() as u32);
    for (offset, c) in &g.stops {
        o.a32(*offset).color(c);
    }
}

pub fn path_out(o: &mut Out, p: &VectorPath) {
    o.u32(p.ops.len() as u32);
    for &op in &p.ops {
        o.u8(op as u8);
    }
    o.u32(p.points.len() as u32);
    for &v in &p.points {
        o.a32(v);
    }
    o.u8(u8::from(p.fill)).color(&p.fill_color).u8(u8::from(p.evenodd)).u8(u8::from(p.stroke)).color(&p.stroke_color);
    o.a32(p.stroke_width).i32(p.line_cap).i32(p.line_join).a32(p.miter_limit);
    o.u32(p.dashes.len() as u32);
    for &v in &p.dashes {
        o.a32(v);
    }
    o.a32(p.dash_offset);
    svg_gradient_out(o, &p.fill_gradient);
    svg_gradient_out(o, &p.stroke_gradient);
}

/// What the paint calls of a container log, and the answers it gives them.
pub struct PaintLog {
    pub active: std::cell::Cell<bool>,
    pub accent: std::cell::Cell<WebColor>,
    pub owners: std::cell::RefCell<BTreeMap<ElementId, i32>>,
    /// While caching, `draw_cached` answers as a cache would: a key not drawn before is painted and kept, a kept one
    /// is drawn as it is.
    pub caching: std::cell::Cell<bool>,
    pub cached_keys: std::cell::RefCell<BTreeSet<u64>>,
}

impl Default for PaintLog {
    fn default() -> PaintLog {
        PaintLog {
            active: std::cell::Cell::new(false),
            accent: std::cell::Cell::new(WebColor::rgba(0, 122, 255, 255)),
            owners: std::cell::RefCell::new(BTreeMap::new()),
            caching: std::cell::Cell::new(false),
            cached_keys: std::cell::RefCell::new(BTreeSet::new()),
        }
    }
}

impl PaintLog {
    pub fn owner_of(&self, el: Option<ElementId>) -> i32 {
        match el {
            None => -1,
            Some(el) => self.owners.borrow().get(&el).copied().unwrap_or(-2),
        }
    }

    pub fn filter(&self, f: &FilterPaint) -> Vec<u8> {
        let mut o = Out::default();
        o.u32(f.steps.len() as u32);
        for step in f.steps {
            o.u8(step.kind as u8).a32(step.amount);
            length_out(&mut o, &step.x);
            length_out(&mut o, &step.y);
            length_out(&mut o, &step.radius);
            o.color(&step.color);
        }
        pos_out(&mut o, &f.bounds);
        pos_out(&mut o, &f.r#box);
        radius_out(&mut o, &f.radius);
        o.i32(self.owner_of(f.owner));
        o.take()
    }

    pub fn text(font: u64, color: WebColor, pos: &Position) -> Vec<u8> {
        let mut o = Out::default();
        o.u64(font).color(&color);
        pos_out(&mut o, pos);
        o.take()
    }

    pub fn marker(m: &ListMarker) -> Vec<u8> {
        let mut o = Out::default();
        o.u8(u8::from(m.baseurl.is_some())).str(m.baseurl.as_deref().unwrap_or(b"")).i32(m.marker_type).color(&m.color);
        pos_out(&mut o, &m.pos);
        o.i32(m.index).u64(m.font);
        o.take()
    }

    pub fn image(layer: &BackgroundLayer, base_url: &[u8]) -> Vec<u8> {
        let mut o = Out::default();
        layer_out(&mut o, layer);
        o.str(base_url);
        o.take()
    }

    pub fn fill(layer: &BackgroundLayer, color: &WebColor) -> Vec<u8> {
        let mut o = Out::default();
        layer_out(&mut o, layer);
        o.color(color);
        o.take()
    }

    pub fn linear(layer: &BackgroundLayer, g: &LinearGradient) -> Vec<u8> {
        let mut o = Out::default();
        layer_out(&mut o, layer);
        points_out(&mut o, &g.base);
        o.a32(g.start.x).a32(g.start.y).a32(g.end.x).a32(g.end.y);
        o.take()
    }

    pub fn radial(layer: &BackgroundLayer, g: &RadialGradient) -> Vec<u8> {
        let mut o = Out::default();
        layer_out(&mut o, layer);
        points_out(&mut o, &g.base);
        o.a32(g.position.x).a32(g.position.y).a32(g.radius.x).a32(g.radius.y);
        o.take()
    }

    pub fn conic(layer: &BackgroundLayer, g: &ConicGradient) -> Vec<u8> {
        let mut o = Out::default();
        layer_out(&mut o, layer);
        points_out(&mut o, &g.base);
        o.a32(g.position.x).a32(g.position.y).a32(g.angle).a32(g.radius);
        o.take()
    }

    pub fn borders(b: &Borders, pos: &Position, root: bool) -> Vec<u8> {
        let mut o = Out::default();
        borders_out(&mut o, b);
        pos_out(&mut o, pos);
        o.u8(u8::from(root));
        o.take()
    }

    pub fn clip(pos: &Position, r: &BorderRadiuses) -> Vec<u8> {
        let mut o = Out::default();
        pos_out(&mut o, pos);
        radius_out(&mut o, r);
        o.take()
    }

    pub fn shadow(bx: &Position, r: &BorderRadiuses, s: &ShadowPaint) -> Vec<u8> {
        let mut o = Out::default();
        pos_out(&mut o, bx);
        radius_out(&mut o, r);
        o.a32(s.x.value()).a32(s.y.value()).a32(s.blur.value()).a32(s.spread.value()).color(&s.color).u8(u8::from(s.inset));
        o.take()
    }

    pub fn path(p: &VectorPath) -> Vec<u8> {
        let mut o = Out::default();
        path_out(&mut o, p);
        o.take()
    }

    pub fn group(&self, g: &PaintGroup) -> Vec<u8> {
        let mut o = Out::default();
        o.a32(g.opacity);
        let m = &g.matrix;
        for v in [m.a, m.b, m.c, m.d, m.tx, m.ty] {
            o.a32(v);
        }
        o.u8(u8::from(g.backface_hidden));
        pos_out(&mut o, &g.bounds);
        o.u8(u8::from(g.exact_bounds)).i32(self.owner_of(g.owner));
        o.take()
    }

    pub fn boxed(bx: &Position, key: Option<u64>) -> Vec<u8> {
        let mut o = Out::default();
        pos_out(&mut o, bx);
        if let Some(key) = key {
            o.u64(key);
        }
        o.take()
    }
}

// ---- The record ---------------------------------------------------------------------------------------------

/// The render tree's numbering of items, and the DOM elements of the cascade's walk.
struct PaintIndex {
    items: Vec<RenderId>,
    item_index: BTreeMap<RenderId, i32>,
    nodes: Vec<ElementId>,
}

impl PaintIndex {
    fn new(doc: &Document) -> PaintIndex {
        let mut nodes = Vec::new();
        if let Some(root) = doc.root {
            cascade_walk(doc, root, -1, &mut nodes);
        }
        let mut index = PaintIndex { items: Vec::new(), item_index: BTreeMap::new(), nodes: nodes.iter().map(|n| n.0).collect() };
        if let Some(root) = doc.render.root {
            index.visit(doc, root);
        }
        for &item in &doc.render.tabular {
            index.visit(doc, item);
        }
        index
    }

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

    fn of(&self, doc: &Document, item: RenderId) -> i32 {
        if !doc.render.alive(item) {
            return -1;
        }
        self.item_index.get(&item).copied().unwrap_or(-3)
    }
}

thread_local! {
    static PAINTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// lhdump's `paint_step`: the document painted with the step's settings, its calls logged by the container.
pub fn paint_step(i: &mut In, o: &mut Out, cx: &StyleCx, doc: &mut Document, log: &PaintLog) {
    let x = i.f32();
    let y = i.f32();
    let flags = i.u8();
    let mut clip = Position::default();
    if flags & 1 != 0 {
        clip.x = Px(i.f32());
        clip.y = Px(i.f32());
        clip.width = Px(i.f32());
        clip.height = Px(i.f32());
    }
    let snap = i.f32();
    let hdc = i.u32();
    let index = PaintIndex::new(doc);
    let pick = |k: u32| (!index.nodes.is_empty()).then(|| index.nodes[k as usize % index.nodes.len()]);
    for which in 0..3 {
        let n = i.u32();
        let mut k = 0;
        while k < n && i.at < i.s.len() {
            if let Some(el) = pick(i.u32()) {
                match which {
                    0 => doc.paint.skip_paint.insert(el),
                    1 => doc.paint.skip_background_images.insert(el),
                    _ => doc.paint.skip_scroll_content.insert(el),
                };
            }
            k += 1;
        }
    }
    let bare = i.i32();
    let unclipped = i.i32();
    doc.paint.bare_effects = if bare >= 0 { pick(bare as u32) } else { None };
    doc.paint.unclipped_scroll = if unclipped >= 0 { pick(unclipped as u32) } else { None };
    doc.paint.bare_shadows = flags & 16 != 0;
    let (r, g, b, a) = (i.u8(), i.u8(), i.u8(), i.u8());
    log.accent.set(WebColor::rgba(r, g, b, a));
    log.active.set(flags & 8 != 0);
    log.caching.set(flags & 32 != 0);
    *log.owners.borrow_mut() = index.nodes.iter().enumerate().map(|(k, &el)| (el, k as i32)).collect();
    if flags & 4 != 0 {
        doc.paint.trace = Some(Vec::new());
    }
    doc.paint.snap_scale = snap;
    PAINTS.with(|p| p.set(p.get() + 1));
    doc.draw(cx, u64::from(hdc), Px(x), Px(y), (flags & 1 != 0).then_some(&clip), flags & 2 != 0);
    if flags & 64 != 0 {
        for &el in &index.nodes {
            if doc.kind(el) == ElementKind::el_svg {
                let placement = DocSvg { doc: &*doc, cx }.placement(el);
                doc.el_svg_paint_shapes(cx, el, u64::from(hdc), &placement);
            }
        }
    }
    log.caching.set(false);
    doc.paint.snap_scale = 1.0;
    let trace = doc.paint.trace.take();
    log.owners.borrow_mut().clear();
    doc.paint.skip_paint.clear();
    doc.paint.skip_background_images.clear();
    doc.paint.skip_scroll_content.clear();
    doc.paint.bare_effects = None;
    doc.paint.unclipped_scroll = None;
    doc.paint.bare_shadows = false;
    o.u8(1);
    if let Some(trace) = trace {
        o.u32(trace.len() as u32);
        for ri in trace {
            o.i32(index.of(doc, ri));
        }
    }
}

/// lhdump's `write_paint`: the paint generation, culling, every item's ink cache and visual transform, each control's
/// look.
pub fn paint_out(o: &mut Out, cx: &StyleCx, doc: &Document) {
    let index = PaintIndex::new(doc);
    mark(o, "paint state", -2);
    o.u32(doc.paint.generation).u8(u8::from(doc.paint.cull));
    o.u32(index.items.len() as u32);
    for (k, &ri) in index.items.iter().enumerate() {
        mark(o, "item's ink", k as i32);
        let ink = &doc.render.item(ri).ink;
        o.u32(ink.generation);
        for i in [&ink.ink, &ink.drawn] {
            o.a32(i.l).a32(i.t).a32(i.r).a32(i.b).u8(u8::from(i.empty)).u8(u8::from(i.unbounded));
        }
        let m = doc.get_visual_transform(cx, ri);
        for v in [m.a, m.b, m.c, m.d, m.tx, m.ty] {
            o.a32(v);
        }
    }
    o.u32(index.nodes.len() as u32);
    for (k, &el) in index.nodes.iter().enumerate() {
        let kind = doc.kind(el);
        o.u8(kind as u8);
        if matches!(
            kind,
            ElementKind::el_input | ElementKind::el_select | ElementKind::el_button | ElementKind::el_textarea | ElementKind::el_meter
        ) {
            mark(o, "control's look", k as i32);
            let look = &doc.el(el).look;
            o.u8(u8::from(look.active)).u8(look.accent.red).u8(look.accent.green).u8(look.accent.blue).u8(look.accent.alpha);
        }
        mark(o, "paint signature", k as i32);
        o.u64(doc.paint_signature(el));
        if kind == ElementKind::el_svg {
            mark(o, "what an svg kept", k as i32);
            let st = doc.svg_paint_state(el);
            o.u64(st.last_paint_key).i32(st.stable_draws).u64(st.content_key).u32(st.content_generation);
            o.u8(u8::from(st.content_cacheable)).u8(u8::from(st.group_cache_eligible)).u32(st.use_generation).i32(st.use_allowance);
        }
        if kind.is_html_tag() && lowcase(&doc.get_tag_name(cx.names, el)) == b"text" {
            mark(o, "svg text box", k as i32);
            let found = doc.svg_text_box(cx, el);
            o.u8(u8::from(found.is_some()));
            if let Some(b) = found {
                pos_out(o, &b);
            }
        }
    }
    mark(o, "the use budget", -2);
    o.u32(doc.paint.svg_use_generation).i32(doc.paint.svg_use_budget);
}

/// What the paints reached, for the report.
#[derive(Default)]
pub struct PaintTally {
    pub documents: usize,
    pub paints: usize,
    pub calls: usize,
    pub seams: BTreeMap<&'static str, usize>,
    pub counts: BTreeMap<String, usize>,
}

impl PaintTally {
    pub fn count(&mut self, what: &str, n: usize) {
        if n > 0 {
            *self.counts.entry(what.to_string()).or_default() += n;
        }
    }

    pub fn merge(&mut self, other: PaintTally) {
        self.documents += other.documents;
        self.paints += other.paints;
        self.calls += other.calls;
        for (k, n) in other.seams {
            *self.seams.entry(k).or_default() += n;
        }
        for (k, n) in other.counts {
            *self.counts.entry(k).or_default() += n;
        }
    }

    pub fn report(&self) -> String {
        let counts: Vec<String> = self.counts.iter().map(|(k, n)| format!("{k} {n}")).collect();
        let seams: Vec<String> = self.seams.iter().map(|(k, n)| format!("{k} {n}")).collect();
        format!(
            "{} documents, {} paints, {} container calls; later stages' paint met (their C++ calls skipped): {}; {}",
            self.documents,
            self.paints,
            self.calls,
            if seams.is_empty() { "none".to_string() } else { seams.join(", ") },
            counts.join(", ")
        )
    }

    /// The paint calls of an answer, counted by kind and by what their arguments show.
    pub fn note_calls(&mut self, answer: &[u8]) {
        let (_, phases) = split_answer(answer);
        for phase in &phases {
            // An <svg>'s paint: its clip (a `C` right before its `A` or `O`) to the matching `D`, which the port's seam
            // stood for before stage L14.
            let mut k = 0;
            while k + 1 < phase.len() {
                if phase[k].0 == b'C' && matches!(phase[k + 1].0, b'A' | b'O') {
                    let mut depth = 0;
                    let start = k;
                    while k < phase.len() {
                        match phase[k].0 {
                            b'C' | b'P' => depth += 1,
                            b'D' => depth -= 1,
                            _ => {}
                        }
                        k += 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    self.count("SVG paints", 1);
                    self.count("calls of SVG paints", k - start);
                    continue;
                }
                k += 1;
            }
            for (kind, a, b, _) in phase {
                let name = match kind {
                    b't' => "texts drawn",
                    b'k' => "list markers drawn",
                    b'g' => "images drawn",
                    b'f' => "solid fills",
                    b'l' => "linear gradients",
                    b'r' => "radial gradients",
                    b'c' => "conic gradients",
                    b'd' => "borders drawn",
                    b'C' => "clips set",
                    b'U' => "clips suspended",
                    b'h' => "box shadows",
                    b'P' => "clip paths",
                    b'G' => "groups",
                    b'X' => "filtered paints",
                    b'K' => "backdrops",
                    b'p' => "paths drawn",
                    b'A' => "cached paints asked",
                    b'a' => "cached paints drawn",
                    b'O' => "isolated paints",
                    b'Q' => "controls asked active",
                    b'q' => "accents asked",
                    b'Z' => {
                        *self.seams.entry(if a == b"image" { "image (L15)" } else { "unknown" }).or_default() += 1;
                        continue;
                    }
                    _ => continue,
                };
                self.calls += 1;
                self.count(name, 1);
                let f = |at: usize| b.get(at..at + 4).map_or(0.0, |s| f32::from_le_bytes([s[0], s[1], s[2], s[3]]));
                let i = |at: usize| b.get(at..at + 4).map_or(0, |s| i32::from_le_bytes([s[0], s[1], s[2], s[3]]));
                match kind {
                    b'G' => {
                        self.count("groups with opacity", usize::from(f(0) < 1.0));
                        self.count(
                            "transformed groups",
                            usize::from(f(4) != 1.0 || f(8) != 0.0 || f(12) != 0.0 || f(16) != 1.0 || f(20) != 0.0 || f(24) != 0.0),
                        );
                        self.count("groups with exact bounds", usize::from(b.get(45) == Some(&1)));
                    }
                    b'd' => {
                        for side in 0..4 {
                            let style = i(side * 13 + 4);
                            self.count(&format!("border sides of style {style}"), 1);
                        }
                        self.count("rounded borders", usize::from((0..8).any(|k| f(52 + k * 4) > 0.0)));
                    }
                    b'h' => self.count("inset box shadows", usize::from(b.last() == Some(&1))),
                    b'k' => {
                        let at = 5 + i(1).max(0) as usize;
                        self.count(&format!("list markers of type {}", i(at)), 1);
                        self.count("list marker images", usize::from(!a.is_empty()));
                    }
                    b'l' | b'r' | b'c' => {
                        // A layer is 89 bytes: its repeat at 84, then the stops' count.
                        let stops = i(89);
                        self.count("gradient stops", stops.max(0) as usize);
                        self.count("gradients of more than 8 stops", usize::from(stops > 8));
                        self.count(&format!("gradient layers of repeat {}", i(84)), 1);
                    }
                    b'f' => {
                        self.count("root background fills", usize::from(b.get(88) == Some(&1)));
                    }
                    b't' => self.count("text bytes drawn", a.len()),
                    b'p' => {
                        // A path: u32 ops and each u8, u32 points and each f32, then u8 fill, colour (5), u8 even-odd, u8 stroke.
                        let ops = i(0).max(0) as usize;
                        let points = i(4 + ops).max(0) as usize;
                        let at = 8 + ops + points * 4;
                        self.count("paths filled", usize::from(b.get(at) == Some(&1)));
                        self.count("paths even-odd", usize::from(b.get(at + 6) == Some(&1)));
                        self.count("paths stroked", usize::from(b.get(at + 7) == Some(&1)));
                        // Then colour (5), f32 width, i32 cap, i32 join, f32 miter, u32 dashes.
                        let after = at + 13;
                        self.count(&format!("stroke caps {}", i(after + 4)), usize::from(b.get(at + 7) == Some(&1)));
                        self.count(&format!("stroke joins {}", i(after + 8)), usize::from(b.get(at + 7) == Some(&1)));
                        let dashes = i(after + 16).max(0) as usize;
                        self.count("dashed paths", usize::from(dashes > 0));
                        // Then the dashes, f32 offset, and the fill and stroke gradients (i32 type first).
                        let gradient = after + 20 + dashes * 4 + 4;
                        let fill_type = i(gradient);
                        self.count(&format!("fill gradients of type {fill_type}"), usize::from(fill_type != 0));
                        let fill_stops = i(gradient + 32).max(0) as usize;
                        let stroke_type = i(gradient + 36 + fill_stops * 9);
                        self.count(&format!("stroke gradients of type {stroke_type}"), usize::from(stroke_type != 0));
                        self.count("path points", points / 2);
                        let nan = (0..points).any(|k| f(8 + ops + k * 4).is_nan());
                        self.count("paths with NaN points", usize::from(nan));
                    }
                    b'A' => self.count("cached paint keys", 1),
                    _ => {}
                }
            }
        }
    }
}

/// What lhdump answers to a `Q` record.
pub fn paint_answer(lh: &Lh, switches: Switches, input: &[u8], tally: &mut PaintTally) -> (u8, Vec<u8>) {
    let paints = PAINTS.with(|p| p.get());
    let answer = record_answer(lh, switches, input, &mut |o: &mut Out, cx: &StyleCx, doc: &Document| {
        paint_out(o, cx, doc);
    });
    tally.documents += 1;
    tally.paints += PAINTS.with(|p| p.get()) - paints;
    tally.note_calls(&answer.1);
    answer
}

/// A `Q` record.
pub fn paint_record(width: f32, height: f32, dpr: f32, master: &[u8], user: &[u8], html: &[u8], steps: &[Step]) -> Vec<u8> {
    cascade_record(width, height, dpr, master, user, html, steps)
}

/// Where the paint calls of the two sides part, `None` when they are alike: each phase call by call, a seam of the
/// port's (`Z`) standing for the C++'s calls of that later stage (an image's one `g`), which are counted in `skipped`.
pub fn compare_paint_calls(cpp: &[u8], rust: &[u8], skipped: &mut usize) -> Option<String> {
    let (_, c) = split_answer(cpp);
    let (_, r) = split_answer(rust);
    if c.len() != r.len() {
        return Some(format!("{} phases of calls in the C++, {} in the port", c.len(), r.len()));
    }
    let shown = |calls: &[Call]| -> Vec<String> {
        calls
            .iter()
            .take(4)
            .map(|(k, a, b, n)| format!("{} {:?} [{}] {n}", *k as char, String::from_utf8_lossy(a), hex(&b[..b.len().min(64)])))
            .collect()
    };
    for (p, (cp, rp)) in c.iter().zip(&r).enumerate() {
        let (mut i, mut j) = (0, 0);
        while i < cp.len() && j < rp.len() {
            let rc = &rp[j];
            if rc.0 == b'Z' {
                if rc.1 != b"image" || cp[i].0 != b'g' {
                    return Some(format!(
                        "phase {p}: the port's {} seam at {j} meets C++ {:?}",
                        String::from_utf8_lossy(&rc.1),
                        shown(&cp[i..])
                    ));
                }
                i += 1;
                *skipped += 1;
                j += 1;
                continue;
            }
            if cp[i] != *rc {
                let (cc, rr) = (&cp[i], rc);
                if cc.0 == rr.0 && cc.1 == rr.1 && cc.3 == rr.3 {
                    let at = cc.2.iter().zip(&rr.2).position(|(a, b)| a != b).unwrap_or(cc.2.len().min(rr.2.len()));
                    let near = |b: &[u8]| hex(&b[at.saturating_sub(8).min(b.len())..(at + 40).min(b.len())]);
                    return Some(format!(
                        "phase {p}, call C++ {i} / Rust {j} ({}): arguments part at byte {at} of {} / {}: C++ {} / Rust {}",
                        cc.0 as char,
                        cc.2.len(),
                        rr.2.len(),
                        near(&cc.2),
                        near(&rr.2)
                    ));
                }
                return Some(format!(
                    "calls of phase {p} part at C++ {i} / Rust {j} ({} / {}): C++ {:?} / Rust {:?}",
                    cp.len(),
                    rp.len(),
                    shown(&cp[i..]),
                    shown(&rp[j..])
                ));
            }
            i += 1;
            j += 1;
        }
        if i != cp.len() || j != rp.len() {
            return Some(format!(
                "calls of phase {p} end apart at C++ {i} / Rust {j} ({} / {}): C++ {:?} / Rust {:?}",
                cp.len(),
                rp.len(),
                shown(&cp[i.min(cp.len())..]),
                shown(&rp[j.min(rp.len())..])
            ));
        }
    }
    None
}
