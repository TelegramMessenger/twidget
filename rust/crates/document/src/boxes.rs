//! renderer.cpp's free helpers that read the render tree for the queries, the layers and the input: an element's
//! border box (an SVG shape's from its geometry, an inline's from its line boxes), its line boxes, where a box shows
//! through the transforms around it, the padding box and what of a box its clipping ancestors let show, offsetLeft and
//! offsetTop with the offsetParent, how far content reaches for scrollWidth and scrollHeight, and an outline's bounds.

use twidget_litehtml::Px;
use twidget_litehtml::css_effects::{TransformMatrix, VectorPath};
use twidget_litehtml::css_values::*;
use twidget_litehtml::document::{Document, StyleCx};
use twidget_litehtml::el_svg::{DocSvg, svg_ctm, svg_geometry, svg_path_length};
use twidget_litehtml::element::ElementId;
use twidget_litehtml::render_item::RenderId;
use twidget_litehtml::string_id::{_body_, _html_, _table_, _td_, _th_};
use twidget_litehtml::types::{Position, contain_layout, contain_paint};
use twidget_wire::Rect;

use crate::container::rect_of;
use crate::cxx::{cmax, cmin};

/// `inline_boxes_of`: the line boxes of an inline element, in document coordinates; none for any other element.
pub fn inline_boxes_of(doc: &Document, cx: &StyleCx, el: ElementId) -> Vec<Rect> {
    let mut boxes = Vec::new();
    let Some(ri) = doc.get_render_item(el) else { return boxes };
    // They are kept in the frame the item's own position is in: its placement, less that position.
    let placed = doc.get_placement(cx, ri);
    let pos = doc.render.item(ri).pos;
    let ox = (placed.x - pos.x).value();
    let oy = (placed.y - pos.y).value();
    doc.for_inline_boxes(ri, &mut |b: &Position, _, _| {
        if b.width.value() > 0.0 || b.height.value() > 0.0 {
            boxes.push(Rect::new(ox + b.x.value(), oy + b.y.value(), b.width.value(), b.height.value()));
        }
        true
    });
    boxes
}

/// `path_bounds`: a path's bounds, sampled along its outline so that curves bound by their ink, not their control
/// points; None when no sample is finite.
pub fn path_bounds(path: &VectorPath) -> Option<[f32; 4]> {
    const STEPS: i32 = 512;
    let total = svg_path_length(path, -1.0).0;
    let (mut x0, mut y0, mut x1, mut y1) = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    for i in 0..=STEPS {
        let (x, y) = svg_path_length(path, total * i as f32 / STEPS as f32).1.unwrap_or((0.0, 0.0));
        x0 = cmin(x0, x);
        y0 = cmin(y0, y);
        x1 = cmax(x1, x);
        y1 = cmax(y1, y);
    }
    x0.is_finite().then_some([x0, y0, x1, y1])
}

/// `svg_box_of`: an SVG shape or text has no box of its own: its geometry, or its text, through its transforms and the
/// view box.
pub fn svg_box_of(doc: &Document, cx: &StyleCx, el: ElementId) -> Option<Rect> {
    if doc.get_render_item(el).is_some() {
        return None;
    }
    let tree = DocSvg { doc, cx };
    let ctm = svg_ctm(&tree, el)?;
    let (x0, y0, x1, y1);
    if let Some(path) = svg_geometry(&tree, el) {
        let [a, b, c, d] = path_bounds(&path)?;
        (x0, y0, x1, y1) = (a, b, c, d);
    } else if let Some(text) = doc.svg_text_box(cx, el) {
        x0 = text.x.value();
        y0 = text.y.value();
        x1 = x0 + text.width.value();
        y1 = y0 + text.height.value();
    } else {
        return None;
    }
    let (mut bx0, mut by0, mut bx1, mut by1) = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    for (x, y) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] {
        let tx = ctm.a * x + ctm.c * y + ctm.tx;
        let ty = ctm.b * x + ctm.d * y + ctm.ty;
        bx0 = cmin(bx0, tx);
        by0 = cmin(by0, ty);
        bx1 = cmax(bx1, tx);
        by1 = cmax(by1, ty);
    }
    Some(Rect::new(bx0, by0, bx1 - bx0, by1 - by0))
}

/// `border_box_of`: the border box of an element in document coordinates (`get_placement()` is the content box;
/// getBoundingClientRect / offsetWidth are border boxes).
pub fn border_box_of(doc: &Document, cx: &StyleCx, el: ElementId) -> Rect {
    if let Some(svg) = svg_box_of(doc, cx, el) {
        return svg;
    }
    // An inline element shows as its line boxes; its own placement is its containing block's origin.
    let lines = inline_boxes_of(doc, cx, el);
    if !lines.is_empty() {
        let (mut x0, mut y0, mut x1, mut y1) = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        for l in &lines {
            x0 = cmin(x0, l.x);
            y0 = cmin(y0, l.y);
            x1 = cmax(x1, l.x + l.w);
            y1 = cmax(y1, l.y + l.h);
        }
        return Rect::new(x0, y0, x1 - x0, y1 - y0);
    }
    let pos = doc.element_placement(cx, el);
    let mut r = rect_of(&pos);
    if let Some(ri) = doc.get_render_item(el) {
        let it = doc.render.item(ri);
        let (pad, bd) = (&it.padding, &it.borders);
        r.x -= pad.left.value() + bd.left.value();
        r.y -= pad.top.value() + bd.top.value();
        r.w += pad.left.value() + pad.right.value() + bd.left.value() + bd.right.value();
        r.h += pad.top.value() + pad.bottom.value() + bd.top.value() + bd.bottom.value();
    }
    r
}

/// `content_extent`: how far the content under `ri` reaches, for scrollWidth and scrollHeight: in-flow children by
/// their margin boxes, everything else by its border box, and nothing past a descendant that clips.
pub fn content_extent(doc: &Document, cx: &StyleCx, ri: RenderId, right: &mut f32, bottom: &mut f32, children: bool) {
    for &c in doc.render.children(ri) {
        let el = doc.render.item(c).element;
        let css = &doc.el(el).css;
        if css.m_display == StyleDisplay::display_none.index() {
            continue;
        }
        let b = border_box_of(doc, cx, el);
        let mut r = b.x + b.w;
        let mut bt = b.y + b.h;
        let position = css.m_el_position;
        if children
            && (position == ElementPosition::element_position_static.index()
                || position == ElementPosition::element_position_relative.index())
        {
            let m = &doc.render.item(c).margins;
            r += m.right.value();
            bt += m.bottom.value();
        }
        *right = cmax(*right, r);
        *bottom = cmax(*bottom, bt);
        if doc.el(el).css.m_overflow == Overflow::overflow_visible.index() {
            content_extent(doc, cx, c, right, bottom, false);
        }
    }
}

/// `transformed_box`: where `r`, in `el`'s layout coordinates, shows: its transform and its ancestors', as a bounding
/// box.
pub fn transformed_box(doc: &Document, cx: &StyleCx, el: ElementId, r: Rect) -> Rect {
    let mut m = TransformMatrix::default();
    let mut ri = doc.get_render_item(el);
    while let Some(item) = ri {
        m = m.then(&doc.get_visual_transform(cx, item));
        ri = doc.render.parent(item);
    }
    if m.is_identity() {
        return r;
    }
    let xs = [r.x, r.x + r.w, r.x, r.x + r.w];
    let ys = [r.y, r.y, r.y + r.h, r.y + r.h];
    let (mut x0, mut y0, mut x1, mut y1) = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    for i in 0..4 {
        let x = m.a * xs[i] + m.c * ys[i] + m.tx;
        let y = m.b * xs[i] + m.d * ys[i] + m.ty;
        x0 = cmin(x0, x);
        y0 = cmin(y0, y);
        x1 = cmax(x1, x);
        y1 = cmax(y1, y);
    }
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

/// `visual_box_of`: where an element's border box shows.
pub fn visual_box_of(doc: &Document, cx: &StyleCx, el: ElementId) -> Rect {
    transformed_box(doc, cx, el, border_box_of(doc, cx, el))
}

/// `padding_box_of`.
pub fn padding_box_of(doc: &Document, cx: &StyleCx, ri: RenderId) -> Rect {
    let it = doc.render.item(ri);
    let border = border_box_of(doc, cx, it.element);
    let (l, t, r, b) = (it.borders.left.value(), it.borders.top.value(), it.borders.right.value(), it.borders.bottom.value());
    Rect::new(border.x + l, border.y + t, border.w - l - r, border.h - t - b)
}

/// `clipped_border_box`: what can show of an item's border box: inside the padding box of each ancestor that clips it,
/// up to a fixed ancestor, which escapes them.
pub fn clipped_border_box(doc: &Document, cx: &StyleCx, ri: RenderId) -> Rect {
    let el = doc.render.item(ri).element;
    let mut bx = border_box_of(doc, cx, el);
    let fixed = ElementPosition::element_position_fixed.index();
    let mut escaped = doc.el(el).css.m_el_position == fixed;
    let mut p = doc.render.parent(ri);
    while let Some(item) = p {
        if !(bx.w > 0.0 && bx.h > 0.0) {
            break;
        }
        if escaped || doc.render.parent(item).is_none() {
            break;
        }
        let css = doc.ri_css(item);
        escaped = css.m_el_position == fixed;
        if css.clips_contents() {
            let clip = padding_box_of(doc, cx, item);
            let x0 = cmax(bx.x, clip.x);
            let y0 = cmax(bx.y, clip.y);
            let x1 = cmin(bx.x + bx.w, clip.x + clip.w);
            let y1 = cmin(bx.y + bx.h, clip.y + clip.h);
            bx = Rect::new(x0, y0, cmax(0.0, x1 - x0), cmax(0.0, y1 - y0));
        }
        p = doc.render.parent(item);
    }
    bx
}

/// `strtoll(id, nullptr, 10)` of a `data-twid`.
pub fn twid(value: &[u8]) -> i64 {
    crate::cxx::strtoll(crate::container::c_str(value))
}

/// `offsets_of`: offsetLeft/offsetTop and offsetParent: from the border box to the padding edge of the nearest
/// positioned, transformed or contained ancestor (a table cell for a static element), else the body.
pub fn offsets_of(doc: &Document, ri: RenderId) -> (f64, f64, i64) {
    let fixed = ElementPosition::element_position_fixed.index();
    let origin = |item: RenderId| -> (f64, f64) {
        let (mut x, mut y) = (0.0f64, 0.0f64);
        let mut cur = Some(item);
        while let Some(i) = cur {
            let sticky = doc.sticky_offset(i);
            let pos = doc.render.item(i).pos;
            x += f64::from(pos.x.value() + sticky.x.value());
            y += f64::from(pos.y.value() + sticky.y.value());
            if doc.ri_css(i).m_el_position == fixed {
                break;
            }
            cur = doc.render.parent(i);
        }
        (x, y)
    };
    let (mut left, mut top) = origin(ri);
    let it = doc.render.item(ri);
    left -= f64::from(it.padding.left.value() + it.borders.left.value());
    top -= f64::from(it.padding.top.value() + it.borders.top.value());
    let mut parent = 0i64;
    let tag = doc.tag(it.element);
    let static_ = ElementPosition::element_position_static.index();
    if doc.ri_css(ri).m_el_position == fixed || tag == _body_ || tag == _html_ {
        return (left, top, parent);
    }
    let mut p = doc.render.parent(ri);
    while let Some(item) = p {
        let css = doc.ri_css(item);
        let pel = doc.render.item(item).element;
        let ptag = doc.tag(pel);
        let positioned =
            css.m_el_position != static_ || !css.m_transform.is_empty() || (css.m_contain & (contain_layout | contain_paint)) != 0;
        let table = doc.ri_css(ri).m_el_position == static_ && (ptag == _table_ || ptag == _td_ || ptag == _th_);
        if !positioned && !table && ptag != _body_ {
            p = doc.render.parent(item);
            continue;
        }
        if let Some(id) = doc.get_attr(pel, b"data-twid") {
            parent = twid(id);
        }
        if positioned || table {
            let (x, y) = origin(item);
            let pi = doc.render.item(item);
            left -= x - f64::from(pi.padding.left.value());
            top -= y - f64::from(pi.padding.top.value());
        }
        break;
    }
    (left.round(), top.round(), parent)
}

/// The renderer's `hittable` check: `computed_pointer_events` (computed_style.cpp) of the item's element is not
/// `none`.
pub fn pointer_events(doc: &Document, cx: &StyleCx, el: ElementId) -> Vec<u8> {
    let mut cur = Some(el);
    while let Some(e) = cur {
        let v =
            if doc.kind(e).is_html_tag() { twidget_css::trim(&doc.raw_declaration(cx.names, e, b"pointer-events")) } else { Vec::new() };
        if !v.is_empty() && v != b"inherit" && v != b"unset" {
            return if v == b"initial" { b"auto".to_vec() } else { v };
        }
        cur = doc.parent(e);
    }
    b"auto".to_vec()
}

/// A box's corner `p` as a position.
pub fn position_of(r: &Rect) -> Position {
    Position { x: Px(r.x), y: Px(r.y), width: Px(r.w), height: Px(r.h) }
}
