//! The Rust side of `oracle/svg_oracle` (stage L14, el_svg.cpp's geometry), with the generators and the corpus its
//! cases come from.
//!
//! svg_oracle reads records `u8 op, u32 length, bytes` and answers each with `u8 status, u32 length, bytes`, as
//! lhdump does. A path is `u32 ops` and each op's `u8` (`vector_path::op_t`), `u32 points` and each `f32`; a matrix
//! six `f32` (a, b, c, d, tx, ty); a length at `at` is `f32 total, u8 written, f32 x, f32 y` of
//! `svg_path_length(path, at, &x, &y)` (x and y the bits `0x7fc12345` unless written). A builder is its path, then
//! `cx cy sx sy qx qy kx ky` and `u8 last`, then the length at -1 and at each of the record's `at`s.
//!
//! - `n` text: `numbers()` (u32 count, f32 each); `next_number` from the start, each step `u8 ok, f32 value, u32
//!   offset` until one fails (at most 64); `next_flag` likewise (`u8 ok, i32 flag, u32 offset`, at most 16); then
//!   the text as a `<rect>`'s `x` attribute: `attr_length` at the bases and defaults (100, -1), (0, 7), (-37.5, 0),
//!   (1e30, 1), (3.3, 0.5), `gradient_coordinate` with 0.5, and `u8 svg_reference_id`.
//! - `p` u32 ats and their f32s, path data: the builder after `path_builder::parse`.
//! - `b` u32 ats and their f32s, u32 steps, each `u8 op` and its operands (0 move x y, 1 line x y, 2 cubic six, 3
//!   quad four, 4 close, 5 arc rx ry rotation i32 large i32 sweep x y, 6 ellipse four, 7 rect six, 8 parse str):
//!   the builder after them.
//! - `t` u8 null, text: `parse_svg_transform` of it (of a null text with `null`).
//! - `m` two matrices: the first `then` the second, then `u8 is_identity` of each.
//! - `D` i32 width, html: a document of it (`createFromString`, litehtml's master sheet, a 375 x 667 screen,
//!   16 px fonts), laid out at the width; status 0 without a root. Then u8 mode and the DOM as lhdump's `h`
//!   writes it (`read_dom`); per node a str of its inputs (u8 el_svg, u8 own `transform`, u8 own
//!   `transform-origin`, u8 fill box, the origin's x and y lengths, u32 transform functions each i32 kind, x and y
//!   lengths and six f32, the placement's four f32), a length being `u8 predefined, i32 predef, i32 units, f32
//!   value, u8 calc`; per node a str of its answers (`element_answers`); the `<use>` calls (`uses_out`), the host
//!   chains (`hosts_out`) and u32 gradient cases, each u32 shape, u32 gradient, u8 `svg_gradient_paint`'s answer,
//!   i32 type and seven f32 (x1 y1 x2 y2 r fx fy).

use std::collections::BTreeMap;
use std::path::PathBuf;

use twidget_litehtml::css_effects::{PathOp, TransformFunction, TransformKind, TransformMatrix, VectorPath};
use twidget_litehtml::css_length::CssLength;
use twidget_litehtml::css_selector::{CssSelector, DocumentMode};
use twidget_litehtml::css_values::CssUnits;
use twidget_litehtml::el_svg::*;
use twidget_litehtml::html::lowcase;
use twidget_litehtml::html_tag::Matcher;
use twidget_litehtml::types::Position;
use twidget_litehtml::{Names, Px, StringId};

use super::{In, Lcg, Out, TestTree, pick, read_dom};

/// The bits svg_oracle leaves in a point `svg_path_length` did not write.
pub const UNWRITTEN: u32 = 0x7fc1_2345;

/// A float of an answer, as svg_oracle writes one: every NaN as one (`0x7fc00000`), since which NaN an operation
/// gives where two meet is the compiler's choice (rust/PLAN.md, Findings).
pub trait Answer {
    fn a32(&mut self, v: f32) -> &mut Out;
}

impl Answer for Out {
    fn a32(&mut self, v: f32) -> &mut Out {
        self.f32(if v.is_nan() { f32::from_bits(0x7fc0_0000) } else { v })
    }
}

/// What a C++ `std::string` made of a `const char*` holds.
pub fn c_str(s: &[u8]) -> &[u8] {
    &s[..s.iter().position(|c| *c == 0).unwrap_or(s.len())]
}

// ---- Elements ------------------------------------------------------------------------------------------------

/// What svg_oracle writes of a node besides the DOM.
#[derive(Debug, Default)]
pub struct SvgNode {
    pub is_svg: bool,
    pub own_transform: bool,
    pub own_origin: bool,
    pub fill_box: bool,
    pub origin_x: CssLength,
    pub origin_y: CssLength,
    pub transform: Vec<TransformFunction>,
    pub placement: Position,
    /// A length of its transform or origin is a calc expression, which the dump does not carry.
    pub calc: bool,
}

/// The fork's laid-out document as svg_oracle writes it, as [`SvgElements`].
#[derive(Debug)]
pub struct SvgTree<'a> {
    pub names: &'a Names,
    pub mode: DocumentMode,
    pub tree: TestTree,
    pub nodes: Vec<SvgNode>,
}

impl SvgElements for SvgTree<'_> {
    type Node = usize;
    fn parent(&self, el: usize) -> Option<usize> {
        self.tree.nodes[el].parent
    }
    fn tag_name(&self, el: usize) -> Vec<u8> {
        self.names.s(self.tree.nodes[el].tag).to_vec()
    }
    fn get_attr(&self, el: usize, name: &[u8]) -> Option<&[u8]> {
        let node = &self.tree.nodes[el];
        if !node.is_tag {
            return None;
        }
        node.attrs.get(name).map(|v| &v[..])
    }
    fn is_html_tag(&self, el: usize) -> bool {
        self.tree.nodes[el].is_tag
    }
    fn is_svg(&self, el: usize) -> bool {
        self.nodes[el].is_svg
    }
    fn root(&self) -> Option<usize> {
        (!self.tree.nodes.is_empty()).then_some(0)
    }
    fn select_one(&self, root: usize, selector: &[u8]) -> Option<usize> {
        let sel = CssSelector::parse(self.names, selector, self.mode)?;
        Matcher::new(&self.tree, self.names).select_one(root, &sel)
    }
    fn has_own_property(&self, el: usize, name: StringId) -> bool {
        let node = &self.nodes[el];
        if name == twidget_litehtml::string_id::_transform_ {
            node.own_transform
        } else if name == twidget_litehtml::string_id::_transform_origin_ {
            node.own_origin
        } else {
            panic!("svg_oracle writes no {name:?}")
        }
    }
    fn transform(&self, el: usize) -> &[TransformFunction] {
        &self.nodes[el].transform
    }
    fn transform_origin_x(&self, el: usize) -> &CssLength {
        &self.nodes[el].origin_x
    }
    fn transform_origin_y(&self, el: usize) -> &CssLength {
        &self.nodes[el].origin_y
    }
    fn transform_fill_box(&self, el: usize) -> bool {
        self.nodes[el].fill_box
    }
    fn placement(&self, el: usize) -> Position {
        self.nodes[el].placement
    }
}

/// One element with attributes and nothing else: what the `n` record writes its text into.
#[derive(Debug, Default)]
pub struct OneElement {
    pub attrs: BTreeMap<Vec<u8>, Vec<u8>>,
    pub none: Vec<TransformFunction>,
    pub length: CssLength,
}

impl SvgElements for OneElement {
    type Node = usize;
    fn parent(&self, _: usize) -> Option<usize> {
        None
    }
    fn tag_name(&self, _: usize) -> Vec<u8> {
        b"rect".to_vec()
    }
    fn get_attr(&self, _: usize, name: &[u8]) -> Option<&[u8]> {
        self.attrs.get(name).map(|v| &v[..])
    }
    fn is_html_tag(&self, _: usize) -> bool {
        true
    }
    fn is_svg(&self, _: usize) -> bool {
        false
    }
    fn root(&self) -> Option<usize> {
        Some(0)
    }
    fn select_one(&self, _: usize, _: &[u8]) -> Option<usize> {
        None
    }
    fn has_own_property(&self, _: usize, _: StringId) -> bool {
        false
    }
    fn transform(&self, _: usize) -> &[TransformFunction] {
        &self.none
    }
    fn transform_origin_x(&self, _: usize) -> &CssLength {
        &self.length
    }
    fn transform_origin_y(&self, _: usize) -> &CssLength {
        &self.length
    }
    fn transform_fill_box(&self, _: usize) -> bool {
        false
    }
    fn placement(&self, _: usize) -> Position {
        Position::default()
    }
}

// ---- Answers -------------------------------------------------------------------------------------------------

pub fn matrix_out(o: &mut Out, m: &TransformMatrix) {
    for v in [m.a, m.b, m.c, m.d, m.tx, m.ty] {
        o.a32(v);
    }
}

pub fn path_out(o: &mut Out, path: &VectorPath) {
    o.u32(path.ops.len() as u32);
    for op in &path.ops {
        o.u8(*op as u8);
    }
    o.u32(path.points.len() as u32);
    for v in &path.points {
        o.a32(*v);
    }
}

pub fn length_at_out(o: &mut Out, path: &VectorPath, at: f32) {
    let (total, point) = svg_path_length(path, at);
    o.a32(total);
    match point {
        Some((x, y)) => o.u8(1).a32(x).a32(y),
        None => o.u8(0).a32(f32::from_bits(UNWRITTEN)).a32(f32::from_bits(UNWRITTEN)),
    };
}

pub fn built_out(o: &mut Out, b: &PathBuilder, ats: &[f32]) {
    path_out(o, &b.path);
    for v in [b.cx, b.cy, b.sx, b.sy, b.qx, b.qy, b.kx, b.ky] {
        o.a32(v);
    }
    o.u8(b.last);
    length_at_out(o, &b.path, -1.0);
    for &at in ats {
        length_at_out(o, &b.path, at);
    }
}

fn ats_out(o: &mut Out, ats: &[f32]) {
    o.u32(ats.len() as u32);
    for &at in ats {
        o.f32(at);
    }
}

/// The `n` record and its answer.
pub fn numbers_case(text: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let mut o = Out::default();
    let list = numbers(Some(text));
    o.u32(list.len() as u32);
    for v in &list {
        o.a32(*v);
    }
    let mut p = 0;
    for _ in 0..64 {
        let v = next_number(text, &mut p);
        o.u8(u8::from(v.is_some())).a32(v.unwrap_or(0.0)).u32(p as u32);
        if v.is_none() {
            break;
        }
    }
    p = 0;
    for _ in 0..16 {
        let flag = next_flag(text, &mut p);
        o.u8(u8::from(flag.is_some())).i32(flag.unwrap_or(-1)).u32(p as u32);
        if flag.is_none() {
            break;
        }
    }
    let mut el = OneElement::default();
    el.attrs.insert(b"x".to_vec(), c_str(text).to_vec());
    for (base, def) in [(100.0, -1.0), (0.0, 7.0), (-37.5, 0.0), (1e30, 1.0), (3.3, 0.5)] {
        o.a32(attr_length(&el, 0, b"x", base, def));
    }
    o.a32(gradient_coordinate(&el, 0, b"x", 0.5));
    o.u8(u8::from(svg_reference_id(c_str(text))));
    (text.to_vec(), o.take())
}

/// The `p` record and its answer, with how the parse ended.
pub fn path_case(ats: &[f32], text: &[u8]) -> (Vec<u8>, Vec<u8>, PathEnd) {
    let mut input = Out::default();
    ats_out(&mut input, ats);
    input.s.extend_from_slice(text);
    let mut b = PathBuilder::new();
    let end = b.parse(text);
    let mut o = Out::default();
    built_out(&mut o, &b, ats);
    (input.take(), o.take(), end)
}

/// A builder call of the `b` record.
#[derive(Clone, Debug)]
pub enum Step {
    Move(f32, f32),
    Line(f32, f32),
    Cubic([f32; 6]),
    Quad([f32; 4]),
    Close,
    Arc(f32, f32, f32, i32, i32, f32, f32),
    Ellipse([f32; 4]),
    Rect([f32; 6]),
    Parse(Vec<u8>),
}

/// The `b` record and its answer, with how each arc went.
pub fn builder_case(ats: &[f32], steps: &[Step]) -> (Vec<u8>, Vec<u8>, Vec<ArcEnd>) {
    let mut input = Out::default();
    ats_out(&mut input, ats);
    input.u32(steps.len() as u32);
    let mut b = PathBuilder::new();
    let mut arcs = Vec::new();
    for step in steps {
        match step {
            Step::Move(x, y) => {
                input.u8(0).f32(*x).f32(*y);
                b.move_to(*x, *y);
            }
            Step::Line(x, y) => {
                input.u8(1).f32(*x).f32(*y);
                b.line(*x, *y);
            }
            Step::Cubic(v) => {
                input.u8(2);
                v.iter().for_each(|f| {
                    input.f32(*f);
                });
                b.cubic(v[0], v[1], v[2], v[3], v[4], v[5]);
            }
            Step::Quad(v) => {
                input.u8(3);
                v.iter().for_each(|f| {
                    input.f32(*f);
                });
                b.quad(v[0], v[1], v[2], v[3]);
            }
            Step::Close => {
                input.u8(4);
                b.close();
            }
            Step::Arc(rx, ry, rotation, large, sweep, x, y) => {
                input.u8(5).f32(*rx).f32(*ry).f32(*rotation).i32(*large).i32(*sweep).f32(*x).f32(*y);
                arcs.push(b.arc(*rx, *ry, *rotation, *large, *sweep, *x, *y));
            }
            Step::Ellipse(v) => {
                input.u8(6);
                v.iter().for_each(|f| {
                    input.f32(*f);
                });
                b.ellipse(v[0], v[1], v[2], v[3]);
            }
            Step::Rect(v) => {
                input.u8(7);
                v.iter().for_each(|f| {
                    input.f32(*f);
                });
                b.rect(v[0], v[1], v[2], v[3], v[4], v[5]);
            }
            Step::Parse(text) => {
                input.u8(8).str(text);
                b.parse(c_str(text));
            }
        }
    }
    let mut o = Out::default();
    built_out(&mut o, &b, ats);
    (input.take(), o.take(), arcs)
}

/// The `t` record and its answer.
pub fn transform_case(text: Option<&[u8]>) -> (Vec<u8>, Vec<u8>) {
    let mut input = Out::default();
    input.u8(u8::from(text.is_none()));
    input.s.extend_from_slice(text.unwrap_or(b""));
    let mut o = Out::default();
    matrix_out(&mut o, &parse_svg_transform(text.map(c_str)));
    (input.take(), o.take())
}

/// The `m` record and its answer.
pub fn matrices_case(a: &TransformMatrix, b: &TransformMatrix) -> (Vec<u8>, Vec<u8>) {
    let mut input = Out::default();
    matrix_out(&mut input, a);
    matrix_out(&mut input, b);
    let mut o = Out::default();
    matrix_out(&mut o, &a.then(b));
    o.u8(u8::from(a.is_identity())).u8(u8::from(b.is_identity()));
    (input.take(), o.take())
}

// ---- Documents -----------------------------------------------------------------------------------------------

fn read_length(i: &mut In, calc: &mut bool) -> CssLength {
    let predefined = i.u8() != 0;
    let predef = i.i32();
    let units = i.i32();
    let value = i.f32();
    if i.u8() != 0 {
        *calc = true;
    }
    if predefined { CssLength::predef_value(predef) } else { CssLength::new(value, CssUnits::from_index(units).expect("a unit")) }
}

fn read_node(bytes: &[u8]) -> SvgNode {
    let mut i = In::new(bytes);
    let mut node =
        SvgNode { is_svg: i.u8() != 0, own_transform: i.u8() != 0, own_origin: i.u8() != 0, fill_box: i.u8() != 0, ..SvgNode::default() };
    let mut calc = false;
    node.origin_x = read_length(&mut i, &mut calc);
    node.origin_y = read_length(&mut i, &mut calc);
    for _ in 0..i.u32() {
        let kind = match i.i32() {
            0 => TransformKind::translate,
            1 => TransformKind::scale,
            2 => TransformKind::rotate,
            3 => TransformKind::skew,
            4 => TransformKind::matrix,
            5 => TransformKind::rotate_x,
            _ => TransformKind::rotate_y,
        };
        let x = read_length(&mut i, &mut calc);
        let y = read_length(&mut i, &mut calc);
        let mut v = [0.0; 6];
        for f in &mut v {
            *f = i.f32();
        }
        node.transform.push(TransformFunction { kind, x, y, v });
    }
    node.placement = Position::new(Px(i.f32()), Px(i.f32()), Px(i.f32()), Px(i.f32()));
    node.calc = calc;
    assert_eq!(i.at, bytes.len(), "a node's inputs read whole");
    node
}

/// What `element_answers` writes of an element.
pub fn element_answers(t: &SvgTree, el: usize, tally: &mut Tally) -> Vec<u8> {
    let mut o = Out::default();
    let name = lowcase(&t.tag_name(el));
    match svg_geometry(t, el) {
        Some(path) => {
            tally.add(format!("geometry {}", String::from_utf8_lossy(&name)));
            o.u8(1);
            path_out(&mut o, &path);
            let (total, _) = svg_path_length(&path, -1.0);
            length_at_out(&mut o, &path, -1.0);
            for at in [0.0, total * 0.37, total * 0.5, total, total + 1.0, 1e30] {
                length_at_out(&mut o, &path, at);
            }
        }
        None => {
            o.u8(0);
        }
    }
    let ctm = svg_ctm(t, el);
    if ctm.is_some() {
        tally.add("ctm");
    }
    o.u8(u8::from(ctm.is_some()));
    matrix_out(&mut o, &ctm.unwrap_or_default());
    if t.is_html_tag(el) {
        let node = &t.nodes[el];
        if node.own_transform {
            tally.add(if node.fill_box { "css transform, fill box" } else { "css transform" });
            for f in &node.transform {
                tally.add(format!("css transform {:?}", f.kind));
            }
            if node.own_origin {
                tally.add("css transform-origin");
            }
        }
        matrix_out(&mut o, &svg_css_transform(t, el, &Position::new(Px(3.0), Px(-7.0), Px(120.0), Px(80.0))));
        let attr = t.get_attr(el, b"transform");
        if attr.is_some() {
            tally.add("transform attribute");
        }
        matrix_out(&mut o, &parse_svg_transform(attr));
    }
    if t.is_svg(el) {
        let b = view_box(t, el);
        tally.add(if b.is_some() { "svg with a view box" } else { "svg without a view box" });
        if let Some(aspect) = t.get_attr(el, b"preserveaspectratio") {
            tally.add(format!("preserveAspectRatio {}", String::from_utf8_lossy(aspect).trim()));
        }
        o.u8(u8::from(b.is_some()));
        for v in b.unwrap_or([0.0; 4]) {
            o.a32(v);
        }
        for max_width in [0.0, 375.0, 123.5] {
            let sz = get_content_size(t, el, Px(max_width));
            o.a32(sz.width.value()).a32(sz.height.value());
        }
        let places =
            [t.placement(el), Position::new(Px(10.0), Px(20.0), Px(0.0), Px(0.0)), Position::new(Px(-5.0), Px(2.5), Px(333.0), Px(77.0))];
        for pos in &places {
            let (view, group) = view_group(t, el, pos);
            for v in view {
                o.a32(v);
            }
            matrix_out(&mut o, &group.matrix);
            for v in [group.bounds.x, group.bounds.y, group.bounds.width, group.bounds.height] {
                o.a32(v.value());
            }
        }
    }
    o.take()
}

/// `use_view` of svg_oracle: the nearest `<svg>`'s view at its placement, 0 0 300 150 without one.
pub fn use_view(t: &SvgTree, el: usize) -> [f32; 4] {
    let mut p = t.parent(el);
    while let Some(e) = p {
        if t.is_svg(e) {
            return view_group(t, e, &t.placement(e)).0;
        }
        p = t.parent(e);
    }
    [0.0, 0.0, 300.0, 150.0]
}

/// `write_uses` of svg_oracle.
pub fn uses_out(t: &SvgTree, tally: &mut Tally) -> Vec<u8> {
    const DEPTHS: [i32; 7] = [0, 0, 1, 8, 9, 3, 2];
    const USE_DEPTHS: [i32; 7] = [0, 1, 0, 2, 0, 1, 3];
    let mut calls = Out::default();
    let mut count = 0u32;
    for budget in [use_budget, 40] {
        let mut uses = SvgUses::new(budget);
        let mut k = 0;
        for i in 0..t.tree.nodes.len() {
            if k >= 1000 {
                break;
            }
            if lowcase(&t.tag_name(i)) != b"use" {
                continue;
            }
            let view = use_view(t, i);
            uses.use_depth = USE_DEPTHS[k % 7];
            let depth = DEPTHS[k % 7];
            k += 1;
            calls.u32(i as u32).i32(depth).i32(uses.use_depth).i32(uses.use_left);
            for v in view {
                calls.a32(v);
            }
            match uses.use_placement(t, i, depth, &view) {
                Ok(placement) => {
                    tally.add(if placement.symbol { "use of a symbol" } else { "use" });
                    calls.u8(1);
                    matrix_out(&mut calls, &placement.matrix);
                }
                Err(refusal) => {
                    tally.add(format!("use refused: {refusal:?}"));
                    calls.u8(0);
                    matrix_out(&mut calls, &TransformMatrix::default());
                }
            }
            calls.i32(uses.use_left);
            count += 1;
        }
    }
    let mut o = Out::default();
    o.u32(count);
    o.s.extend_from_slice(&calls.s);
    o.take()
}

/// `write_hosts` of svg_oracle.
pub fn hosts_out(t: &SvgTree, tally: &mut Tally) -> Vec<u8> {
    let mut hosts = UseHosts::new();
    let mut pairs = Vec::new();
    let n = t.tree.nodes.len();
    for i in 0..n {
        if pairs.len() >= 12 {
            break;
        }
        if lowcase(&t.tag_name(i)) != b"use" {
            continue;
        }
        let Some(href) = t.get_attr(i, b"href").or_else(|| t.get_attr(i, b"xlink:href")).map(c_str) else { continue };
        if href.first() != Some(&b'#') || !svg_reference_id(&href[1..]) {
            continue;
        }
        if let Some(target) = t.select_one(0, &[b"#", &href[1..]].concat()) {
            pairs.push((target, i));
        }
    }
    if n > 0 {
        for k in 0..6 {
            pairs.push(((7 * k + 3) % n, (13 * k + 1) % n));
        }
    }
    let mut o = Out::default();
    o.u32(pairs.len() as u32);
    for &(target, r#use) in &pairs {
        hosts.push(target, r#use);
        o.u32(target as u32).u32(r#use as u32);
    }
    let chains = n.min(400);
    o.u32(chains as u32);
    for i in 0..chains {
        let mut hosts_above = hosts.len();
        let mut chain = Vec::new();
        let mut cur = Some(i);
        while let Some(e) = cur {
            if chain.len() >= 4096 {
                break;
            }
            chain.push(e as u32);
            let before = hosts_above;
            cur = hosts.svg_parent(t, e, &mut hosts_above);
            if hosts_above != before {
                tally.add("svg_parent through a host");
            }
        }
        o.u32(chain.len() as u32);
        for c in chain {
            o.u32(c);
        }
    }
    for _ in 0..pairs.len() {
        hosts.pop();
    }
    assert!(hosts.is_empty());
    o.take()
}

/// What one `D` answer held and how the port compared with it.
#[derive(Debug, Default)]
pub struct DocumentResult {
    pub nodes: usize,
    pub answers_alike: usize,
    pub calc_skipped: usize,
    pub uses: usize,
    pub chains: usize,
    pub gradients_compared: usize,
    pub gradients_refused: usize,
    pub differences: Vec<String>,
}

/// Compares a `D` answer with the port, element by element.
pub fn document_compare(names: &Names, answer: &[u8], tally: &mut Tally) -> DocumentResult {
    let mut result = DocumentResult::default();
    let (mode, tree, at) = read_dom(names, answer);
    let mut i = In::new(answer);
    i.at = at;
    let n = tree.nodes.len();
    let nodes: Vec<SvgNode> = (0..n).map(|_| read_node(&i.str())).collect();
    let t = SvgTree { names, mode, tree, nodes };
    result.nodes = n;
    // An element whose transform, or an ancestor's, holds a calc length is left out: the dump does not carry it.
    let calc_above: Vec<bool> = (0..n)
        .map(|el| {
            let mut p = Some(el);
            while let Some(e) = p {
                if t.nodes[e].calc {
                    return true;
                }
                p = t.parent(e);
            }
            false
        })
        .collect();
    for el in 0..n {
        let theirs = i.str();
        if calc_above[el] {
            result.calc_skipped += 1;
            continue;
        }
        let ours = element_answers(&t, el, tally);
        if theirs == ours {
            result.answers_alike += 1;
        } else {
            result.differences.push(format!(
                "element {el} <{}>: {}",
                String::from_utf8_lossy(&t.tag_name(el)),
                first_difference(&theirs, &ours)
            ));
        }
    }
    let uses = uses_out(&t, tally);
    let theirs = &answer[i.at..(i.at + uses.len()).min(answer.len())];
    result.uses = (uses.len() - 4) / 61;
    if theirs != &uses[..] {
        result.differences.push(format!("the <use> calls: {}", first_difference(theirs, &uses)));
    }
    i.at += uses.len();
    let hosts = hosts_out(&t, tally);
    let theirs = &answer[i.at.min(answer.len())..(i.at + hosts.len()).min(answer.len())];
    result.chains = n.min(400);
    if theirs != &hosts[..] {
        result.differences.push(format!("the host chains: {}", first_difference(theirs, &hosts)));
    }
    i.at += hosts.len();
    for _ in 0..i.u32() {
        let shape = i.u32() as usize;
        let gradient = i.u32() as usize;
        let ok = i.u8() != 0;
        let ty = i.i32();
        let fields: Vec<u32> = (0..7).map(|_| i.u32()).collect();
        if !ok {
            result.gradients_refused += 1;
            continue;
        }
        let path = svg_geometry(&t, shape).expect("a shape with an outline");
        match svg_gradient_geometry(&t, gradient, &path) {
            Some(g) => {
                let ours: Vec<u32> =
                    [g.x1, g.y1, g.x2, g.y2, g.r, g.fx, g.fy].iter().map(|v| if v.is_nan() { 0x7fc0_0000 } else { v.to_bits() }).collect();
                if g.ty != ty || ours != fields {
                    result.differences.push(format!("gradient {gradient} over {shape}: C++ {ty} {fields:x?}, Rust {} {ours:x?}", g.ty));
                } else {
                    tally.add(if ty == 1 { "linear gradient" } else { "radial gradient" });
                    if t.get_attr(gradient, b"gradientunits").is_some_and(|u| c_str(u) == b"userSpaceOnUse") {
                        tally.add("gradient in user space");
                    }
                    result.gradients_compared += 1;
                }
            }
            None => result.differences.push(format!("gradient {gradient} over {shape}: the C++ painted one the port does not take")),
        }
    }
    assert_eq!(i.at, answer.len(), "the answer read whole");
    result
}

pub fn first_difference(theirs: &[u8], ours: &[u8]) -> String {
    let at = theirs.iter().zip(ours).position(|(a, b)| a != b).unwrap_or(theirs.len().min(ours.len()));
    let near = |b: &[u8]| super::hex(&b[at.min(b.len())..(at + 24).min(b.len())]);
    format!("{} / {} bytes, first difference at {at}: C++ …{} / Rust …{}", theirs.len(), ours.len(), near(theirs), near(ours))
}

// ---- What the cases reached ----------------------------------------------------------------------------------

/// Counts of what the cases reached, by name.
#[derive(Debug, Default)]
pub struct Tally {
    pub counts: BTreeMap<String, usize>,
}

impl Tally {
    pub fn add(&mut self, key: impl Into<String>) {
        *self.counts.entry(key.into()).or_default() += 1;
    }

    pub fn report(&self) -> String {
        self.counts.iter().map(|(k, n)| format!("{k} {n}")).collect::<Vec<_>>().join(", ")
    }

    /// The command letters a path's text holds before where its parse stopped, and how it stopped.
    pub fn path(&mut self, text: &[u8], end: PathEnd) {
        let text = c_str(text);
        for &c in text {
            if b"MmLlHhVvCcSsQqTtAaZz".contains(&c) {
                self.add(format!("command {}", c as char));
            }
        }
        self.add(format!("path end {end:?}"));
    }

    pub fn arc(&mut self, end: ArcEnd) {
        self.add(format!("arc {end:?}"));
    }

    pub fn ops(&mut self, path: &VectorPath) {
        for op in &path.ops {
            self.add(format!("op {op:?}"));
        }
    }
}

// ---- Generated cases -----------------------------------------------------------------------------------------

/// A number as path data and transforms write it: plain, signed, decimals, exponents, compact forms.
pub fn path_number(random: &mut Lcg) -> String {
    match random.below(12) {
        0 => pick(
            random,
            &[
                "0",
                "-0",
                ".5",
                "-.5",
                "1e3",
                "1E-2",
                "1e",
                "+1",
                "1.",
                "1.5.5",
                "-1-2",
                "0.0001",
                "1e38",
                "1e39",
                "-1e39",
                "1e-45",
                "1e-50",
                "4e38",
                "340282350000000000000000000000000000000",
            ],
        )
        .to_string(),
        1 => format!("{}", random.below(2001) as i64 - 1000),
        2 => format!("{:.3}", (random.next() as f64 / 4294967296.0 - 0.5) * 400.0),
        3 => format!("{}e{}", random.below(100), random.below(80) as i64 - 40),
        4 => format!("-{}.{}", random.below(100), random.below(1000)),
        5 => format!(".{}", random.below(1000)),
        _ => format!("{}", random.below(200)),
    }
}

/// A separator: whitespace of any kind, a comma, or nothing.
fn separator(random: &mut Lcg) -> &'static str {
    pick(random, &[" ", " ", ",", ", ", "", "\t", "\n", " ,", "\r\n", "\x0c", "\x0b", "  "])
}

/// Path data of every command, both cases, implicit repeats, compact flags and numbers, and the errors.
pub fn any_path_data(random: &mut Lcg) -> Vec<u8> {
    let mut s = String::new();
    if random.chance(12) {
        s.push_str(pick(random, &["L1 1", "1 2", "z", "a", " ", "", "x", "m", "M", "\u{e9}M1 1"]));
    }
    let commands = 1 + random.below(14);
    for c in 0..commands {
        let letter = if c == 0 && !random.chance(10) {
            if random.chance(2) { 'M' } else { 'm' }
        } else {
            *pick(
                random,
                &[
                    &'M', &'m', &'L', &'l', &'H', &'h', &'V', &'v', &'C', &'c', &'S', &'s', &'Q', &'q', &'T', &'t', &'A', &'a', &'A', &'a',
                    &'Z', &'z', &'e', &'B',
                ],
            )
        };
        if !random.chance(8) {
            s.push(letter);
            s.push_str(separator(random));
        }
        let count = match letter.to_ascii_uppercase() {
            'M' | 'L' | 'T' => 2,
            'H' | 'V' => 1,
            'C' => 6,
            'S' | 'Q' => 4,
            'A' => 7,
            _ => 0,
        };
        let repeats = if random.chance(4) { 2 + random.below(3) } else { 1 };
        for _ in 0..repeats {
            let n = if random.chance(10) { random.below(count + 2) } else { count };
            for k in 0..n {
                if letter.eq_ignore_ascii_case(&'a') && (k == 3 || k == 4) && n == 7 {
                    s.push_str(pick(random, &["0", "1", "1", "0", "2", "01", "10", "-1"]));
                } else if letter.eq_ignore_ascii_case(&'a') && k < 2 && random.chance(4) {
                    s.push_str(pick(random, &["0", "-5", "0.001", "1e-30", "1e30"]));
                } else {
                    s.push_str(&path_number(random));
                }
                s.push_str(separator(random));
            }
        }
        if count == 0 && random.chance(5) {
            s.push_str(&path_number(random));
        }
    }
    let mut bytes = s.into_bytes();
    if random.chance(30) {
        let at = random.below(bytes.len() + 1);
        bytes.insert(at, 0);
    }
    bytes
}

/// A `transform` attribute's text: every function, any case, too few and too many numbers, units, junk.
pub fn any_transform_text(random: &mut Lcg) -> Vec<u8> {
    let mut s = String::new();
    for _ in 0..1 + random.below(4) {
        s.push_str(separator(random));
        let name = pick(
            random,
            &["matrix", "translate", "scale", "rotate", "skewX", "skewY", "SKEWY", "Rotate", "TRANSLATE", "foo", "", "translate3d", "skew"],
        );
        s.push_str(name);
        if random.chance(10) {
            s.push_str(pick(random, &[" ", "x", "-1"]));
        }
        if !random.chance(20) {
            s.push('(');
        }
        let n = match random.below(6) {
            0 => 0,
            1 => 1,
            2 => 2,
            3 => 3,
            4 => 6,
            _ => random.below(9),
        };
        for k in 0..n {
            if k > 0 {
                s.push_str(separator(random));
            }
            s.push_str(&path_number(random));
            if random.chance(6) {
                s.push_str(pick(random, &["px", "deg", "%", "e", "rad"]));
            }
        }
        if !random.chance(20) {
            s.push(')');
        }
    }
    s.into_bytes()
}

fn any_float_text(random: &mut Lcg) -> String {
    if random.chance(6) { super::any_f32(random).to_string() } else { path_number(random) }
}

/// A length attribute's text: numbers with units and percentages, empty, junk.
fn length_text(random: &mut Lcg) -> String {
    match random.below(10) {
        0 => pick(random, &["", " ", "auto", "%", "-", "50%", "100%", "1e39", "-1e39", "0", "-0", "1e-45", "x5", "5px", "5em", ".5%"])
            .to_string(),
        1 => format!("{}%", path_number(random)),
        2 => format!("{}{}", path_number(random), pick(random, &["px", "em", "pt", " ", ",3"])),
        _ => format!("{}", random.below(200) as i64 - 20),
    }
}

fn attr(out: &mut String, name: &str, value: &str) {
    out.push(' ');
    out.push_str(name);
    out.push_str("=\"");
    out.push_str(&value.replace('"', "&quot;"));
    out.push('"');
}

/// A CSS transform, origin and box for an SVG element's `style`.
fn css_transform(random: &mut Lcg) -> String {
    let mut s = String::new();
    if random.chance(2) {
        s.push_str("transform:");
        for _ in 0..1 + random.below(3) {
            s.push(' ');
            s.push_str(&match random.below(9) {
                0 => format!("translate({}px, {}%)", random.below(50), random.below(100)),
                1 => format!("translateX({}%)", random.below(100)),
                2 => format!("scale({}, {})", path_number(random), path_number(random)),
                3 => format!("rotate({}deg)", random.below(720) as i64 - 360),
                4 => format!("skew({}deg, {}deg)", random.below(80), random.below(80)),
                5 => format!(
                    "matrix({}, {}, {}, {}, {}, {})",
                    path_number(random),
                    path_number(random),
                    path_number(random),
                    path_number(random),
                    path_number(random),
                    path_number(random)
                ),
                6 => format!("rotateX({}deg)", random.below(180)),
                7 => format!("rotateY({}turn)", random.below(3)),
                _ => format!("translateY({}em)", random.below(10)),
            });
        }
        s.push(';');
    }
    if random.chance(2) {
        s.push_str(&format!(
            " transform-origin: {};",
            pick(random, &["center", "0 0", "50% 50%", "10px 20%", "left top", "right bottom", "25% 75%", "3em 2px"])
        ));
    }
    if random.chance(2) {
        s.push_str(&format!(" transform-box: {};", pick(random, &["fill-box", "view-box", "stroke-box", "border-box"])));
    }
    if random.chance(8) {
        s.push_str(" translate: 5px 7px; rotate: 30deg; scale: 2;");
    }
    s
}

fn shape(random: &mut Lcg, ids: &[String], out: &mut String, depth: u32) {
    let tag = pick(
        random,
        &[
            "circle",
            "ellipse",
            "rect",
            "line",
            "polyline",
            "polygon",
            "path",
            "path",
            "g",
            "use",
            "use",
            "text",
            "symbol",
            "linearGradient",
            "radialGradient",
            "defs",
            "svg",
            "a",
            "image",
        ],
    );
    out.push('<');
    out.push_str(tag);
    if random.chance(3) || matches!(tag, "symbol" | "linearGradient" | "radialGradient" | "g") {
        attr(out, "id", &ids[random.below(ids.len())]);
    }
    let lengths: &[&str] = match tag {
        "circle" => &["cx", "cy", "r"],
        "ellipse" => &["cx", "cy", "rx", "ry"],
        "rect" => &["x", "y", "width", "height", "rx", "ry"],
        "line" => &["x1", "y1", "x2", "y2"],
        "use" => &["x", "y", "width", "height"],
        "linearGradient" => &["x1", "y1", "x2", "y2"],
        "radialGradient" => &["cx", "cy", "r", "fx", "fy"],
        "svg" => &["width", "height", "x", "y"],
        _ => &[],
    };
    for name in lengths {
        if !random.chance(5) {
            attr(out, name, &length_text(random));
        }
    }
    match tag {
        "polyline" | "polygon" => {
            let mut points = String::new();
            for _ in 0..random.below(9) {
                points.push_str(&path_number(random));
                points.push_str(separator(random));
            }
            attr(out, "points", &points);
        }
        "path" => attr(out, "d", &String::from_utf8_lossy(&any_path_data(random)).replace('\0', "")),
        "use" => {
            let target = format!("#{}", ids[random.below(ids.len())]);
            match random.below(6) {
                0 => attr(out, "xlink:href", &target),
                1 => attr(out, "href", pick(random, &["", "#", "x", "#a b", "#\u{e9}t\u{e9}", "#-_x9"])),
                2 => {
                    attr(out, "href", "");
                    attr(out, "xlink:href", &target);
                }
                _ => attr(out, "href", &target),
            }
        }
        "symbol" | "svg" => {
            if random.chance(3) {
                attr(
                    out,
                    "viewBox",
                    &format!("{} {} {} {}", path_number(random), path_number(random), length_text(random), path_number(random)),
                );
            } else if random.chance(2) {
                attr(out, "viewBox", &format!("0 0 {} {}", 1 + random.below(100), 1 + random.below(100)));
            }
        }
        "linearGradient" | "radialGradient" if random.chance(2) => {
            attr(out, "gradientUnits", pick(random, &["userSpaceOnUse", "objectBoundingBox", "userspaceonuse"]));
        }
        _ => {}
    }
    if random.chance(4) {
        attr(out, "transform", &String::from_utf8_lossy(&any_transform_text(random)));
    }
    if random.chance(4) {
        attr(out, "style", &css_transform(random));
    }
    if random.chance(8) {
        attr(out, "class", pick(random, &["spin", "grow", "flip"]));
    }
    if random.chance(10) {
        attr(out, "fill", pick(random, &["url(#g1)", "red", "none", "url(#missing)"]));
    }
    out.push('>');
    match tag {
        "g" | "symbol" | "defs" | "svg" | "a" if depth < 5 => {
            for _ in 0..random.below(5) {
                shape(random, ids, out, depth + 1);
            }
        }
        "linearGradient" | "radialGradient" => {
            for _ in 0..random.below(4) {
                out.push_str(&format!(
                    "<stop offset=\"{}\" stop-color=\"{}\"></stop>",
                    length_text(random),
                    pick(random, &["red", "#00f", "currentColor", "x"])
                ));
            }
        }
        "text" => out.push_str("Hi"),
        _ => {}
    }
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

/// A document of SVGs: sizes and view boxes and aspect ratios, groups with transforms (attributes and CSS),
/// every shape with lengths of every form, `<use>`s of symbols and of shapes (missing, cyclic, deep), gradients;
/// and shapes outside any `<svg>`.
pub fn any_svg_document(random: &mut Lcg) -> Vec<u8> {
    let ids: Vec<String> = ["s1", "s2", "g1", "c", "x-y", "u_1", "\u{e9}t\u{e9}", "a", "1a", "-9"].iter().map(|s| s.to_string()).collect();
    let mut out = String::from("<!doctype html><html><head><style>");
    if random.chance(2) {
        out.push_str(&format!(".spin {{ {} }} ", css_transform(random)));
        out.push_str(&format!(".grow {{ {} }} ", css_transform(random)));
    }
    out.push_str("</style></head><body>");
    for _ in 0..1 + random.below(3) {
        out.push_str("<svg");
        if !random.chance(4) {
            attr(&mut out, "width", &length_text(random));
        }
        if !random.chance(4) {
            attr(&mut out, "height", &length_text(random));
        }
        match random.below(4) {
            0 => {}
            1 => attr(
                &mut out,
                "viewBox",
                &format!("{} {} {} {}", any_float_text(random), any_float_text(random), any_float_text(random), any_float_text(random)),
            ),
            _ => attr(
                &mut out,
                "viewBox",
                &format!(
                    "{} {} {} {}",
                    random.below(20) as i64 - 10,
                    random.below(20) as i64 - 10,
                    1 + random.below(300),
                    1 + random.below(300)
                ),
            ),
        }
        if random.chance(3) {
            attr(
                &mut out,
                "preserveAspectRatio",
                pick(random, &["none", "xMinYMin meet", "xMidYMid slice", "xMaxYMax", "xMinYMax slice", "XMAXYMIN", "xMidYMin", "foo", ""]),
            );
        }
        if random.chance(4) {
            attr(
                &mut out,
                "style",
                pick(random, &["width: 50%", "display: block; width: 120px", "height: 3em", "transform: rotate(10deg)"]),
            );
        }
        out.push('>');
        for _ in 0..1 + random.below(8) {
            shape(random, &ids, &mut out, 0);
        }
        out.push_str("</svg>");
        if random.chance(4) {
            out.push_str("<p>text</p>");
        }
    }
    if random.chance(5) {
        shape(random, &ids, &mut out, 3);
    }
    out.push_str("</body></html>");
    out.into_bytes()
}

/// A float for the builder: the edges (zero, signs, NaN, infinities, huge, tiny) and ordinary values.
pub fn builder_float(random: &mut Lcg) -> f32 {
    match random.below(4) {
        0 => super::any_f32(random),
        _ => (random.below(4001) as f32 - 2000.0) / 8.0,
    }
}

/// A builder script of every call, arcs most of all, with edge values.
pub fn any_builder_steps(random: &mut Lcg) -> Vec<Step> {
    let mut steps = Vec::new();
    for _ in 0..1 + random.below(10) {
        let r = &mut *random;
        let step = match r.below(14) {
            0 => Step::Move(builder_float(r), builder_float(r)),
            1 => Step::Line(builder_float(r), builder_float(r)),
            2 => Step::Cubic(std::array::from_fn(|_| builder_float(r))),
            3 => Step::Quad(std::array::from_fn(|_| builder_float(r))),
            4 => Step::Close,
            5 => Step::Ellipse(std::array::from_fn(|_| builder_float(r))),
            6 => Step::Rect(std::array::from_fn(|_| builder_float(r))),
            7 => Step::Parse(any_path_data(r)),
            _ => {
                let flags = [0, 1, 0, 1, 2, -1, i32::MIN];
                let (rx, ry, rotation, x, y) = (builder_float(r), builder_float(r), builder_float(r), builder_float(r), builder_float(r));
                Step::Arc(rx, ry, rotation, flags[r.below(7)], flags[r.below(7)], x, y)
            }
        };
        steps.push(step);
    }
    steps
}

/// The lengths a case asks the point at: zero, a few along, past the end, negative, NaN, infinite.
pub fn any_ats(random: &mut Lcg) -> Vec<f32> {
    (0..random.below(5))
        .map(|_| match random.below(6) {
            0 => *pick(random, &[&0.0f32, &-0.0, &-1.0, &f32::NAN, &f32::INFINITY, &1e30, &f32::MIN_POSITIVE]),
            _ => random.below(4000) as f32 / 8.0,
        })
        .collect()
}

// ---- The corpus ----------------------------------------------------------------------------------------------

fn text_files(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            text_files(&path, out);
        } else if path.extension().is_some_and(|e| ["html", "js", "mjs", "css", "json"].iter().any(|x| e == *x)) {
            out.push(path);
        }
    }
}

/// widget-sandbox's corpus, conformance set and stress kit, every text file by path.
pub fn corpus_texts() -> Vec<(String, Vec<u8>)> {
    let root = super::widget_sandbox();
    let mut files = Vec::new();
    for dir in ["corpus", "packages/conformance", "packages/stress"] {
        text_files(&root.join(dir), &mut files);
    }
    files.into_iter().filter_map(|p| Some((p.strip_prefix(&root).unwrap_or(&p).display().to_string(), std::fs::read(&p).ok()?))).collect()
}

fn find_ci(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    (from..=hay.len().checked_sub(needle.len())?).find(|&i| hay[i..i + needle.len()].eq_ignore_ascii_case(needle))
}

/// Every `<svg>…</svg>` the corpus writes, in its pages or its scripts' strings, as written.
pub fn corpus_svg_snippets(texts: &[(String, Vec<u8>)]) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for (name, text) in texts {
        let mut at = 0;
        let mut k = 0;
        while let Some(start) = find_ci(text, b"<svg", at) {
            let Some(end) = find_ci(text, b"</svg>", start) else { break };
            out.push((format!("{name} svg {k}"), text[start..end + 6].to_vec()));
            k += 1;
            at = start + 4;
        }
    }
    out
}

/// Every value the corpus gives an attribute `name` (`name="…"`, `name='…'`, and the strings scripts pass for it).
pub fn corpus_attribute_values(texts: &[(String, Vec<u8>)], name: &str) -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    let patterns: Vec<(Vec<u8>, u8)> = vec![
        (format!(" {name}=\"").into_bytes(), b'"'),
        (format!(" {name}='").into_bytes(), b'\''),
        (format!("'{name}', '").into_bytes(), b'\''),
        (format!("\"{name}\", \"").into_bytes(), b'"'),
        (format!("'{name}','").into_bytes(), b'\''),
    ];
    for (_, text) in texts {
        for (open, close) in &patterns {
            let mut at = 0;
            while let Some(start) = find_ci(text, open, at) {
                let from = start + open.len();
                let Some(len) = text[from..].iter().position(|c| c == close) else { break };
                let value = text[from..from + len].to_vec();
                if !out.contains(&value) {
                    out.push(value);
                }
                at = from + len;
            }
        }
    }
    out
}
