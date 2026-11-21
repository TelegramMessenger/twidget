//! Hit testing and the mouse state (stage L16): `render_item.cpp`'s hit half (`get_element_by_point` with its passes
//! in z-order, `get_child_by_point`, `is_point_inside` of each class: the border box less a sticky shift inside a clip
//! path, an inline's boxes, a table row's cells), scrolling (`render_item.h`'s and `element.cpp`'s `h_scroll`,
//! `v_scroll`, the scrollable tests, `scroll_to`, `scroll_box` over `scroll_view.h`, which is [`ScrollView`]'s),
//! `element::get_placement`, and `document.cpp`'s pointer: `on_mouse_over`, `on_lbutton_down`, `on_lbutton_up`,
//! `on_button_cancel`, `on_mouse_leave` and `on_scroll` with the hovered and active elements (kept with what patching
//! remaps, `DocPatch`), the container's cursor and mouse events and the redraw boxes of the restyle that follows. The
//! elements' own parts are in their modules: `html_tag.rs` (`:hover` and `:active` up the chain, clicks),
//! `el_anchor.rs` (an anchor's click), `el_svg.rs` (an `<svg>`'s `element_at` through its painter's hit mode).
//!
//! The walk only reads the document, so a hit test takes `&self`; what a check asks, and the container's
//! `point_in_clip_path`, are its only calls out.
//!
//! [`ScrollView`]: crate::render_item::ScrollView

use std::collections::BTreeSet;

use crate::Px;
use crate::css_values::{ElementFloat, ElementPosition, StyleDisplay};
use crate::document::{Document, StyleCx};
use crate::document_container::MouseEvent;
use crate::element::ElementId;
use crate::paint::{DrawFlag, group_transform};
use crate::render_item::{RenderId, RenderKind};
use crate::types::Position;

/// What `get_element_by_point`'s `check` asks of a render item a hit would land on: whether it may take it (the
/// renderer's `hittable`, `on_scroll`'s scrollable tests).
pub type HitCheck<'a> = &'a dyn Fn(&Document, RenderId) -> bool;

/// `scroll_values`: how far `on_scroll` moved a scroller and its placement.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScrollValues {
    pub dx: Px,
    pub dy: Px,
    pub scroll_box: Position,
}

/// A point as the walk carries it: where it is in the item's parent's content box, and where it is in the client
/// (the viewport), which a fixed box is placed against.
#[derive(Clone, Copy, Debug)]
struct Point {
    x: Px,
    y: Px,
    client_x: Px,
    client_y: Px,
}

/// What `get_child_by_point` asks of a child first: nothing, its stacking context (`get_element_by_point`) at a
/// point, or its children in the same pass.
#[derive(Clone, Copy, Debug)]
enum First {
    Nothing,
    Element(Point),
    Children,
}

/// What `get_child_by_point` does with a child in a pass (`Hit::plan`): the point in its parent's content box mapped
/// back through its transform, what is asked first, where its own box is tried when that gives nothing, and whether
/// what its children take is asked after.
#[derive(Clone, Copy, Debug)]
struct Plan {
    hit: Point,
    first: First,
    own: Option<(Px, Px)>,
    into: bool,
}

/// One hit test: the document, the container and the check, so that the walk's frames carry only the item, the point
/// and the pass.
struct Hit<'a> {
    doc: &'a Document,
    cx: &'a StyleCx<'a>,
    check: Option<HitCheck<'a>>,
}

impl Hit<'_> {
    /// `check(ri)`, true without one.
    fn passes(&self, ri: RenderId) -> bool {
        self.check.is_none_or(|check| check(self.doc, ri))
    }

    /// The container's `point_in_clip_path` of the item's clip path over its border box.
    #[inline(never)]
    fn in_clip_path(&self, id: RenderId, x: Px, y: Px) -> bool {
        let it = self.doc.render.item(id);
        let mut bx = it.pos;
        bx.grow(&it.padding);
        bx.grow(&it.borders);
        self.cx.container.point_in_clip_path(&self.doc.ri_css(id).m_clip_path.resolve(&bx), x, y)
    }

    /// `get_element_by_point(x, y, client_x, client_y, check)`: nothing for an item that is not visible, or for a
    /// point (less the item's sticky shift) outside its clip path; else its children's hit in the order painting
    /// covers them (`passes_of`); else the item itself when the point is inside it (a fixed box's client point) and
    /// the check lets it. Each pass's choices are made in functions of their own, which keeps each level's frames
    /// small.
    fn element_by_point(&self, id: RenderId, p: Point) -> Option<ElementId> {
        let (p, sticky) = self.enter(id, p)?;
        for (flag, zindex) in self.passes_of(id) {
            if let Some(ret) = self.child_by_point(id, p, flag, zindex) {
                return Some(ret);
            }
        }
        self.own_hit(id, p, sticky)
    }

    /// The start of `get_element_by_point`: none for an item that is not visible or a point (less the item's sticky
    /// shift) outside its clip path; else the point less the shift, and the shift.
    #[inline(never)]
    fn enter(&self, id: RenderId, mut p: Point) -> Option<(Point, Position)> {
        let doc = self.doc;
        if !doc.is_visible(id) {
            return None;
        }
        let sticky = doc.sticky_offset(id);
        p.x -= sticky.x;
        p.y -= sticky.y;
        if !doc.ri_css(id).m_clip_path.empty() && !self.in_clip_path(id, p.x, p.y) {
            return None;
        }
        Some((p, sticky))
    }

    /// The passes of `get_element_by_point`, in order: over the z-indexes of the item's positioned list (each once, as
    /// `std::map<int, bool>`'s keys) the positioned ones of positive `z-index` (highest first), of `z-index` 0, then
    /// inlines, floats, blocks, then the positioned ones of negative `z-index` (highest first).
    #[inline(never)]
    fn passes_of(&self, id: RenderId) -> Vec<(DrawFlag, i32)> {
        let doc = self.doc;
        let z: BTreeSet<i32> = doc.render.item(id).positioned.iter().map(|&i| doc.ri_css(i).get_z_index()).collect();
        let mut passes: Vec<(DrawFlag, i32)> = z.iter().rev().filter(|&&z| z > 0).map(|&z| (DrawFlag::draw_positioned, z)).collect();
        if z.contains(&0) {
            passes.push((DrawFlag::draw_positioned, 0));
        }
        passes.extend([(DrawFlag::draw_inlines, 0), (DrawFlag::draw_floats, 0), (DrawFlag::draw_block, 0)]);
        passes.extend(z.iter().rev().filter(|&&z| z < 0).map(|&z| (DrawFlag::draw_positioned, z)));
        passes
    }

    /// The end of `get_element_by_point`: the item itself, when the point (a fixed box's client point, another's own
    /// with the sticky shift added back) is inside it and the check lets it.
    #[inline(never)]
    fn own_hit(&self, id: RenderId, p: Point, sticky: Position) -> Option<ElementId> {
        let doc = self.doc;
        let inside = if doc.ri_css(id).m_el_position == ElementPosition::element_position_fixed.index() {
            doc.is_point_inside(self.cx, id, p.client_x, p.client_y)
        } else {
            doc.is_point_inside(self.cx, id, p.x + sticky.x, p.y + sticky.y)
        };
        (inside && self.passes(id)).then(|| doc.render.item(id).element)
    }

    /// `get_child_by_point(x, y, client_x, client_y, flag, zindex, check)`: nothing for a point outside the item's clip
    /// path, or outside its content box when it clips its contents; else the first of its children, from the last,
    /// that takes the point in the pass, the point moved into the item's content box and by its scroll position.
    fn child_by_point(&self, id: RenderId, p: Point, flag: DrawFlag, zindex: i32) -> Option<ElementId> {
        let doc = self.doc;
        let css = doc.ri_css(id);
        if !css.m_clip_path.empty() && !self.in_clip_path(id, p.x, p.y) {
            return None;
        }
        let pos = doc.render.item(id).pos;
        if css.clips_contents() && !pos.is_point_inside(p.x, p.y) {
            return None;
        }
        let el_pos = Point { x: p.x - pos.x + doc.get_scroll_left(id), y: p.y - pos.y + doc.get_scroll_top(id), ..p };
        for &el in doc.render.children(id).iter().rev() {
            if let Some(ret) = self.child_hit(el, el_pos, flag, zindex) {
                return Some(ret);
            }
        }
        None
    }

    /// One child of `get_child_by_point` at the point in its parent's content box, as `plan` has it: what is asked
    /// first, then its own box when that gave nothing, then what its children take in the same pass, which, with a
    /// check, one of the child's render items must pass.
    #[inline(never)]
    fn child_hit(&self, el: RenderId, hit: Point, flag: DrawFlag, zindex: i32) -> Option<ElementId> {
        let plan = self.plan(el, hit, flag, zindex)?;
        let mut ret = match plan.first {
            First::Nothing => None,
            First::Element(p) => self.element_by_point(el, p),
            First::Children => self.child_by_point(el, plan.hit, flag, zindex),
        };
        if ret.is_none()
            && let Some((x, y)) = plan.own
        {
            ret = self.own_box(el, x, y);
        }
        if plan.into
            && let Some(child) = self.child_by_point(el, plan.hit, flag, zindex)
        {
            ret = self.accepted(child).or(ret);
        }
        ret
    }

    /// What `get_child_by_point` does with a child in the pass: nothing when it is not visible or is text, or its
    /// transform does not invert; else, the point mapped back through its transform (a fixed one's client point too),
    /// its own hit in the pass that draws it (a positioned box of the z-index through its stacking context, a fixed
    /// one at its client point; a block through its children; a float through its stacking context; an inline box
    /// through its stacking context and an inline by its boxes); and, unless it was asked whole or is positioned,
    /// what its children take in the same pass (in the positioned pass whatever it holds; in the others not a float's
    /// or an inline box's, but an inline table's).
    #[inline(never)]
    fn plan(&self, el: RenderId, mut hit: Point, flag: DrawFlag, zindex: i32) -> Option<Plan> {
        let doc = self.doc;
        let element = doc.render.item(el).element;
        let el_css = &doc.el(element).css;
        if !doc.is_visible(el) || el_css.m_display == StyleDisplay::display_inline_text.index() {
            return None;
        }
        let fixed = el_css.m_el_position == ElementPosition::element_position_fixed.index();
        if !el_css.m_transform.is_empty() && !Self::untransform(doc, el, fixed, &mut hit) {
            return None;
        }
        let positioned = doc.is_positioned(element);
        let floats = el_css.m_float != ElementFloat::float_none.index();
        let inline = doc.is_inline(element);
        let display = el_css.m_display;
        let mut plan = Plan { hit, first: First::Nothing, own: None, into: false };
        let mut process = true;
        match flag {
            DrawFlag::draw_positioned => {
                if positioned && el_css.get_z_index() == zindex {
                    let (x, y) = if fixed { (hit.client_x, hit.client_y) } else { (hit.x, hit.y) };
                    plan.first = First::Element(Point { x, y, ..hit });
                    plan.own = Some((x, y));
                    process = false;
                }
            }
            DrawFlag::draw_block => {
                if !inline && !floats && !positioned {
                    plan.first = First::Children;
                    plan.own = Some((hit.x, hit.y));
                    process = false;
                }
            }
            DrawFlag::draw_floats => {
                if floats && !positioned {
                    plan.first = First::Element(hit);
                    plan.own = Some((hit.x, hit.y));
                    process = false;
                }
            }
            DrawFlag::draw_inlines => {
                if inline && !floats && !positioned {
                    if display == StyleDisplay::display_inline_block.index()
                        || display == StyleDisplay::display_inline_table.index()
                        || display == StyleDisplay::display_inline_flex.index()
                        || display == StyleDisplay::display_inline_grid.index()
                    {
                        plan.first = First::Element(hit);
                        process = false;
                    }
                    plan.own = Some((hit.x, hit.y));
                }
            }
            DrawFlag::draw_root => {}
        }
        plan.into = process
            && !positioned
            && (flag == DrawFlag::draw_positioned
                || (!floats
                    && display != StyleDisplay::display_inline_block.index()
                    && display != StyleDisplay::display_inline_flex.index()
                    && display != StyleDisplay::display_inline_grid.index()));
        Some(plan)
    }

    /// The child itself, when the point is inside it and the check lets it.
    #[inline(never)]
    fn own_box(&self, el: RenderId, x: Px, y: Px) -> Option<ElementId> {
        (self.doc.is_point_inside(self.cx, el, x, y) && self.passes(el)).then(|| self.doc.render.item(el).element)
    }

    /// What a child's children took, when it counts: always without a check, else when one of its render items passes
    /// the check (`run_on_renderers`, in their order, until one does).
    #[inline(never)]
    fn accepted(&self, child: ElementId) -> Option<ElementId> {
        (self.check.is_none() || self.doc.live_renders(child).any(|ri| self.passes(ri))).then_some(child)
    }

    /// The point (and a fixed box's client point) mapped back through the child's transform about its border box;
    /// false when the matrix's determinant is under 1e-12, and the child is passed over.
    #[inline(never)]
    fn untransform(doc: &Document, el: RenderId, fixed: bool, hit: &mut Point) -> bool {
        let it = doc.render.item(el);
        let mut bx = it.pos;
        bx.grow(&it.padding);
        bx.grow(&it.borders);
        let matrix = group_transform(doc.ri_css(el), &bx);
        let determinant = matrix.a * matrix.d - matrix.b * matrix.c;
        if determinant.abs() < 1e-12 {
            return false;
        }
        let inverse = |px: &mut Px, py: &mut Px| {
            let dx = px.value() - matrix.tx;
            let dy = py.value() - matrix.ty;
            *px = Px((matrix.d * dx - matrix.c * dy) / determinant);
            *py = Px((matrix.a * dy - matrix.b * dx) / determinant);
        };
        inverse(&mut hit.x, &mut hit.y);
        if fixed {
            inverse(&mut hit.client_x, &mut hit.client_y);
        }
        true
    }
}

impl Document {
    // ---- Hit testing ---------------------------------------------------------------------------------------------

    /// `render_item::get_element_by_point(x, y, client_x, client_y, check)`: the element at the point, `x`, `y` in the
    /// item's parent's content box and `client_x`, `client_y` in the viewport (where a fixed box is placed), as
    /// painting covers it: the topmost of the item's descendants that takes the point and passes `check`, else the
    /// item itself; none when nothing does.
    #[allow(clippy::too_many_arguments)]
    pub fn get_element_by_point(
        &self,
        cx: &StyleCx,
        id: RenderId,
        x: Px,
        y: Px,
        client_x: Px,
        client_y: Px,
        check: Option<HitCheck>,
    ) -> Option<ElementId> {
        Hit { doc: self, cx, check }.element_by_point(id, Point { x, y, client_x, client_y })
    }

    /// `render_item::get_child_by_point(x, y, client_x, client_y, flag, zindex, check)`: the element one of the item's
    /// children (from the last) takes in the pass `flag` (of the z-index, for the positioned pass).
    #[allow(clippy::too_many_arguments)]
    pub fn get_child_by_point(
        &self,
        cx: &StyleCx,
        id: RenderId,
        x: Px,
        y: Px,
        client_x: Px,
        client_y: Px,
        flag: DrawFlag,
        zindex: i32,
        check: Option<HitCheck>,
    ) -> Option<ElementId> {
        Hit { doc: self, cx, check }.child_by_point(id, Point { x, y, client_x, client_y }, flag, zindex)
    }

    /// `is_point_inside(x, y)` of the item's class: an inline's (and a text's) boxes; a table row's cells, each from
    /// the cell's margin edge across and over the row's padding and borders down; any other's border box, the point
    /// less its sticky shift, inside its clip path when it has one (the container's `point_in_clip_path`).
    pub fn is_point_inside(&self, cx: &StyleCx, id: RenderId, x: Px, y: Px) -> bool {
        let it = self.render.item(id);
        match it.kind {
            RenderKind::render_item_inline | RenderKind::render_text => it.inline_boxes.iter().any(|b| b.is_point_inside(x, y)),
            RenderKind::render_item_table_row => {
                for &el in &it.children {
                    if self.ri_css(el).m_display == StyleDisplay::display_table_cell.index() {
                        let c = self.render.item(el);
                        let mut pos = Position::default();
                        pos.x = c.left() + c.margins.left;
                        pos.y = c.top() - it.padding.top - it.borders.top;
                        pos.width = c.right() - pos.x - c.margins.right - c.margins.left;
                        pos.height = c.height() + it.padding.top + it.padding.bottom + it.borders.top + it.borders.bottom;
                        if pos.is_point_inside(x, y) {
                            return true;
                        }
                    }
                }
                false
            }
            _ => {
                let mut pos = it.pos;
                let sticky = self.sticky_offset(id);
                let x = x - sticky.x;
                let y = y - sticky.y;
                pos.grow(&it.padding);
                pos.grow(&it.borders);
                let css = self.ri_css(id);
                if !css.m_clip_path.empty() && !cx.container.point_in_clip_path(&css.m_clip_path.resolve(&pos), x, y) {
                    return false;
                }
                pos.is_point_inside(x, y)
            }
        }
    }

    // ---- Scrolling -----------------------------------------------------------------------------------------------

    /// `render_item::h_scroll(dx)`: the item's view moved by `dx` within its range (none without a view); how far.
    pub fn h_scroll(&mut self, id: RenderId, dx: Px) -> Px {
        self.render.item_mut(id).scroll_view.as_mut().map_or(Px(0.0), |view| view.h_scroll(dx))
    }

    /// `render_item::v_scroll(dy)`.
    pub fn v_scroll(&mut self, id: RenderId, dy: Px) -> Px {
        self.render.item_mut(id).scroll_view.as_mut().map_or(Px(0.0), |view| view.v_scroll(dy))
    }

    /// `render_item::is_h_scrollable(dx)`: the item has a view that `dx` would move.
    pub fn is_h_scrollable(&self, id: RenderId, dx: Px) -> bool {
        self.render.item(id).scroll_view.as_ref().is_some_and(|view| view.is_h_scrollable(dx))
    }

    /// `render_item::is_v_scrollable(dy)`.
    pub fn is_v_scrollable(&self, id: RenderId, dy: Px) -> bool {
        self.render.item(id).scroll_view.as_ref().is_some_and(|view| view.is_v_scrollable(dy))
    }

    /// `render_item::scroll_to(left, top)`: the item's view at the position, within its range.
    pub fn scroll_to(&mut self, id: RenderId, left: Px, top: Px) {
        if let Some(view) = self.render.item_mut(id).scroll_view.as_mut() {
            view.scroll_to(left, top);
        }
    }

    /// `render_item::scroll_box(box)`: the box moved by the item's scroll position.
    pub fn scroll_box(&self, id: RenderId, bx: &mut Position) {
        if let Some(view) = self.render.item(id).scroll_view.as_ref() {
            bx.x -= view.left;
            bx.y -= view.top;
        }
    }

    /// `element::h_scroll(dx)`: the element's first render item scrolled (nothing when it is gone or there is none).
    pub fn element_h_scroll(&mut self, el: ElementId, dx: Px) -> Px {
        match self.get_render_item(el) {
            Some(ri) => self.h_scroll(ri, dx),
            None => Px(0.0),
        }
    }

    /// `element::v_scroll(dy)`.
    pub fn element_v_scroll(&mut self, el: ElementId, dy: Px) -> Px {
        match self.get_render_item(el) {
            Some(ri) => self.v_scroll(ri, dy),
            None => Px(0.0),
        }
    }

    /// `element::get_placement()`: the first live render item's placement, moved right and down to any later one's
    /// that starts further right or down; an empty box without one.
    pub fn element_placement(&self, cx: &StyleCx, el: ElementId) -> Position {
        let mut pos = Position::default();
        let mut is_first = true;
        for ri in self.live_renders(el) {
            let ri_pos = self.get_placement(cx, ri);
            if is_first {
                is_first = false;
                pos = ri_pos;
            } else {
                if pos.x < ri_pos.x {
                    pos.x = ri_pos.x;
                }
                if pos.y < ri_pos.y {
                    pos.y = ri_pos.y;
                }
            }
        }
        pos
    }

    // ---- The document's pointer ----------------------------------------------------------------------------------

    /// `get_over_element()`: the element the pointer is over.
    pub fn get_over_element(&self) -> Option<ElementId> {
        self.patch.over_element
    }

    /// `m_active_element`: the element the last press landed on, until a cancel (a release leaves it).
    pub fn get_active_element(&self) -> Option<ElementId> {
        self.patch.active_element
    }

    /// `m_over_element = el`: a `shared_ptr` in the C++, which keeps an anonymous element the render tree made alive
    /// after the tree lets it go (a hit can land on one); the element it held is let go.
    pub(crate) fn set_over_element(&mut self, el: Option<ElementId>) {
        self.hold_anonymous(el);
        if let Some(old) = std::mem::replace(&mut self.patch.over_element, el) {
            self.release_anonymous(old);
        }
    }

    /// `m_active_element = el`, as `set_over_element`.
    pub(crate) fn set_active_element(&mut self, el: Option<ElementId>) {
        self.hold_anonymous(el);
        if let Some(old) = std::mem::replace(&mut self.patch.active_element, el) {
            self.release_anonymous(old);
        }
    }

    /// A `shared_ptr` to the element taken: an anonymous one is kept (`release_anonymous` lets it go).
    pub(crate) fn hold_anonymous(&mut self, el: Option<ElementId>) {
        if let Some(el) = el
            && self.el(el).anonymous_style
        {
            self.el_mut(el).box_holds += 1;
        }
    }

    /// The hovered element's `cursor` for `set_cursor`, empty when none is hovered, up to its first NUL.
    fn over_cursor(&self) -> Vec<u8> {
        match self.patch.over_element {
            Some(over) => {
                let cursor = &self.el(over).css.m_cursor;
                cursor[..cursor.iter().position(|&c| c == 0).unwrap_or(cursor.len())].to_vec()
            }
            None => Vec::new(),
        }
    }

    /// `document::on_mouse_over(x, y, client_x, client_y, redraw_box)`: the element at the point becomes the hovered
    /// one (the last leaves `:hover` and `:active`, the container told; the new one and its ancestors gain `:hover`),
    /// the container gets its cursor, and when a state changed, the container is told of the enter and the styles that
    /// changed are restyled, each restyled element's and its children's boxes told to `redraw_box` first; true when
    /// one was. A new paint generation in any case; false without a root or a render tree.
    #[allow(clippy::too_many_arguments)]
    pub fn doc_on_mouse_over(
        &mut self,
        cx: &StyleCx,
        x: Px,
        y: Px,
        client_x: Px,
        client_y: Px,
        redraw_box: &mut dyn FnMut(Position),
    ) -> bool {
        self.invalidate_paint();
        let (Some(root), Some(root_render)) = (self.root, self.render.root) else { return false };
        let over_el = self.get_element_by_point(cx, root_render, x, y, client_x, client_y, None);
        let mut state_was_changed = false;
        if over_el != self.patch.over_element {
            if let Some(over) = self.patch.over_element
                && self.on_mouse_leave(over)
            {
                cx.container.on_mouse_event(Some(over), MouseEvent::mouse_event_leave);
                state_was_changed = true;
            }
            self.set_over_element(over_el);
        }
        if let Some(over) = self.patch.over_element
            && self.on_mouse_over(over)
        {
            state_was_changed = true;
        }
        cx.container.set_cursor(&self.over_cursor());
        if state_was_changed {
            cx.container.on_mouse_event(self.patch.over_element, MouseEvent::mouse_event_enter);
            return self.find_styles_changes_with(cx, root, redraw_box);
        }
        false
    }

    /// `document::on_lbutton_down(x, y, client_x, client_y, redraw_box)`: the element at the point becomes the active
    /// one and, when it is another, the hovered one (the last leaving, the new one gaining `:hover`); it and its
    /// ancestors gain `:active`, the container gets the cursor, and a state changed restyles as `on_mouse_over` does.
    #[allow(clippy::too_many_arguments)]
    pub fn doc_on_lbutton_down(
        &mut self,
        cx: &StyleCx,
        x: Px,
        y: Px,
        client_x: Px,
        client_y: Px,
        redraw_box: &mut dyn FnMut(Position),
    ) -> bool {
        self.invalidate_paint();
        let (Some(root), Some(root_render)) = (self.root, self.render.root) else { return false };
        let over_el = self.get_element_by_point(cx, root_render, x, y, client_x, client_y, None);
        self.set_active_element(over_el);
        let mut state_was_changed = false;
        if over_el != self.patch.over_element {
            if let Some(over) = self.patch.over_element
                && self.on_mouse_leave(over)
            {
                cx.container.on_mouse_event(Some(over), MouseEvent::mouse_event_leave);
                state_was_changed = true;
            }
            self.set_over_element(over_el);
            if let Some(over) = self.patch.over_element
                && self.on_mouse_over(over)
            {
                state_was_changed = true;
            }
        }
        if let Some(over) = self.patch.over_element
            && self.on_lbutton_down(over)
        {
            state_was_changed = true;
        }
        cx.container.set_cursor(&self.over_cursor());
        if state_was_changed {
            cx.container.on_mouse_event(self.patch.over_element, MouseEvent::mouse_event_enter);
            return self.find_styles_changes_with(cx, root, redraw_box);
        }
        false
    }

    /// `document::on_lbutton_up(x, y, client_x, client_y, redraw_box)`: the point is not read; the hovered element and
    /// its ancestors leave `:active`, a click when it is the active one (which stays active), and what changed is
    /// restyled.
    #[allow(clippy::too_many_arguments)]
    pub fn doc_on_lbutton_up(
        &mut self,
        cx: &StyleCx,
        _x: Px,
        _y: Px,
        _client_x: Px,
        _client_y: Px,
        redraw_box: &mut dyn FnMut(Position),
    ) -> bool {
        self.invalidate_paint();
        let (Some(root), Some(_)) = (self.root, self.render.root) else { return false };
        if let Some(over) = self.patch.over_element
            && self.on_lbutton_up(cx, over, self.patch.active_element == Some(over))
        {
            return self.find_styles_changes_with(cx, root, redraw_box);
        }
        false
    }

    /// `document::on_button_cancel(redraw_box)`: no element is active, and the pointer leaves.
    pub fn doc_on_button_cancel(&mut self, cx: &StyleCx, redraw_box: &mut dyn FnMut(Position)) -> bool {
        self.invalidate_paint();
        self.set_active_element(None);
        self.doc_on_mouse_leave(cx, redraw_box)
    }

    /// `document::on_mouse_leave(redraw_box)`: no element is hovered; the last one and its ancestors leave `:hover`
    /// and `:active`, and when that changed them the container is told and what changed is restyled.
    pub fn doc_on_mouse_leave(&mut self, cx: &StyleCx, redraw_box: &mut dyn FnMut(Position)) -> bool {
        self.invalidate_paint();
        let (Some(root), Some(_)) = (self.root, self.render.root) else { return false };
        let Some(el) = self.patch.over_element.take() else { return false };
        // The C++'s local keeps the element until the function returns.
        let mut ret = false;
        if self.on_mouse_leave(el) {
            cx.container.on_mouse_event(Some(el), MouseEvent::mouse_event_leave);
            ret = self.find_styles_changes_with(cx, root, redraw_box);
        }
        self.release_anonymous(el);
        ret
    }

    /// `document::on_scroll(dx, dy, x, y, client_x, client_y)`: the element at the point whose render item `dy` would
    /// scroll is scrolled by it, and the one `dx` would scroll by that (through each element's first render item,
    /// which need not be the one that passed); one value when both are the same element, else the vertical one first.
    /// Nothing when both deltas are 0. A new paint generation in any case. The C++ reads the render tree without
    /// looking for one (undefined without it); without one nothing scrolls here.
    #[allow(clippy::too_many_arguments)]
    pub fn doc_on_scroll(&mut self, cx: &StyleCx, dx: Px, dy: Px, x: Px, y: Px, client_x: Px, client_y: Px) -> Vec<ScrollValues> {
        self.invalidate_paint();
        if dy == Px(0.0) && dx == Px(0.0) {
            return Vec::new();
        }
        let Some(root_render) = self.render.root else { return Vec::new() };
        let mut vscroll_el = None;
        let mut hscroll_el = None;
        if dy != Px(0.0) {
            let check = |doc: &Document, ri: RenderId| doc.is_v_scrollable(ri, dy);
            vscroll_el = self.get_element_by_point(cx, root_render, x, y, client_x, client_y, Some(&check));
        }
        if dx != Px(0.0) {
            let check = |doc: &Document, ri: RenderId| doc.is_h_scrollable(ri, dx);
            hscroll_el = self.get_element_by_point(cx, root_render, x, y, client_x, client_y, Some(&check));
        }
        match (vscroll_el, hscroll_el) {
            (None, None) => Vec::new(),
            (Some(v), Some(h)) if v == h => {
                let dx = self.element_h_scroll(h, dx);
                let dy = self.element_v_scroll(v, dy);
                vec![ScrollValues { dx, dy, scroll_box: self.element_placement(cx, h) }]
            }
            (v, h) => {
                let mut ret = Vec::with_capacity(2);
                if let Some(v) = v {
                    let dy = self.element_v_scroll(v, dy);
                    ret.push(ScrollValues { dx: Px(0.0), dy, scroll_box: self.element_placement(cx, v) });
                }
                if let Some(h) = h {
                    let dx = self.element_h_scroll(h, dx);
                    ret.push(ScrollValues { dx, dy: Px(0.0), scroll_box: self.element_placement(cx, h) });
                }
                ret
            }
        }
    }
}
