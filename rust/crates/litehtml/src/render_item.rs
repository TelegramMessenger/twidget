//! `render_item.h` and `render_item.cpp`: the render items, built from the cascaded DOM (stage L6:
//! `element::create_render_item`, `el_image`'s and `el_svg`'s, `el_comment`'s none), `init()` (the base class's
//! here; `render_item_block`'s, `render_item_flex`'s and `render_item_table`'s in their modules), `split_inlines`,
//! `clone`, and each element's list of its render items (`element::m_renders`, `add_render`, `get_render_item`);
//! and their layout (stages L7 and L8, in `layout.rs`).
//!
//! The C++'s `shared_ptr` tree is an arena, [`RenderTree`], of [`RenderItem`]s by [`RenderId`]. What owns an item in
//! the C++ owns it here, counted: its parent's children, the document's root and its tabular list, a table's grid.
//! An item no owner keeps is gone, as the C++ frees it, which shows: an element's render items are weak
//! references (`get_render_item` answers nothing when the first has gone) and so is an item's parent. An id says
//! which use of a slot it names, so a reference to a gone item stays gone when the slot is used again.
//! An item made and not yet given to an owner is alive, as a C++ local holds it. The anonymous elements the tree
//! makes (`html_tag`'s anonymous wrapper constructor) live as long as an item of theirs or an anonymous element
//! whose parent they are, and their slots in the document's elements are used again after.

use crate::Px;
use crate::css_values::*;
use crate::document::{Document, StyleCx};
use crate::element::{ElementId, ElementKind};
use crate::line_box::LineBox;
use crate::string_id::*;
use crate::table::TableGrid;
use crate::types::{ContainingBlockContext, Margins, Position, RenderedWidth, Size};

macro_rules! numbers {
    ($($ty:ident :: $name:ident),* $(,)?) => {
        $(#[allow(dead_code)] pub(crate) const $name: i32 = $ty::$name.index();)*
    };
}

numbers!(
    StyleDisplay::display_none,
    StyleDisplay::display_block,
    StyleDisplay::display_inline,
    StyleDisplay::display_inline_block,
    StyleDisplay::display_inline_table,
    StyleDisplay::display_list_item,
    StyleDisplay::display_table,
    StyleDisplay::display_table_caption,
    StyleDisplay::display_table_cell,
    StyleDisplay::display_table_column,
    StyleDisplay::display_table_column_group,
    StyleDisplay::display_table_footer_group,
    StyleDisplay::display_table_header_group,
    StyleDisplay::display_table_row,
    StyleDisplay::display_table_row_group,
    StyleDisplay::display_inline_text,
    StyleDisplay::display_flex,
    StyleDisplay::display_inline_flex,
    StyleDisplay::display_grid,
    StyleDisplay::display_inline_grid,
    ElementFloat::float_none,
    ElementFloat::float_left,
    ElementFloat::float_right,
    ElementClear::clear_none,
    ElementClear::clear_left,
    ElementClear::clear_right,
    ElementClear::clear_both,
    ListStyleType::list_style_type_none,
    ListStyleType::list_style_type_armenian,
    ListStylePosition::list_style_position_inside,
    BorderCollapse::border_collapse_separate,
    ElementPosition::element_position_static,
    ElementPosition::element_position_relative,
    ElementPosition::element_position_absolute,
    ElementPosition::element_position_fixed,
    ElementPosition::element_position_sticky,
    Overflow::overflow_visible,
    Overflow::overflow_hidden,
    Overflow::overflow_scroll,
    Overflow::overflow_auto,
    Overflow::overflow_clip,
    VerticalAlign::va_baseline,
    VerticalAlign::va_sub,
    VerticalAlign::va_super,
    VerticalAlign::va_top,
    VerticalAlign::va_text_top,
    VerticalAlign::va_middle,
    VerticalAlign::va_bottom,
    VerticalAlign::va_text_bottom,
    TextAlign::text_align_left,
    TextAlign::text_align_right,
    TextAlign::text_align_center,
    TextAlign::text_align_justify,
    TextAlign::text_align_start,
    TextAlign::text_align_end,
    WhiteSpace::white_space_normal,
    WhiteSpace::white_space_nowrap,
    WhiteSpace::white_space_pre,
    WhiteSpace::white_space_pre_line,
    WhiteSpace::white_space_pre_wrap,
    OverflowWrap::overflow_wrap_normal,
    OverflowWrap::overflow_wrap_anywhere,
    Direction::direction_rtl,
);

/// A render item: its slot in [`RenderTree`] and which use of the slot it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RenderId {
    pub index: u32,
    pub generation: u32,
}

/// The render item classes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum RenderKind {
    /// `render_item`, which nothing makes but its own `clone()`.
    render_item,
    render_item_block,
    render_item_block_context,
    render_item_inline_context,
    render_item_inline,
    render_text,
    render_item_image,
    render_item_table,
    render_item_table_part,
    render_item_table_row,
    render_item_flex,
    render_item_grid,
}

/// `render_item_table`'s own: `m_grid` (none until `init()`), `m_border_spacing_x` and `m_border_spacing_y`.
#[derive(Clone, Debug, Default)]
pub struct TableData {
    pub grid: Option<Box<TableGrid>>,
    pub border_spacing_x: Px,
    pub border_spacing_y: Px,
}

/// `render_item::layout_entry`: a layout's input and what it gave.
#[derive(Clone, Copy, Debug, Default)]
pub struct LayoutEntry {
    pub cb: ContainingBlockContext,
    pub ret: RenderedWidth,
    pub second_pass: bool,
    pub valid: bool,
    pub outer_height: Px,
}

/// The layout cache of a render item and its marks: `m_layout_dirty`, `m_relayout_pending`, `m_out_of_flow`,
/// `m_layout_epoch`, the last layout (`m_last_layout` with the place and boxes it left), the two measures
/// (`m_measured`, `m_measured_next`) and `m_resolved_layout_safe`.
#[derive(Clone, Copy, Debug)]
pub struct LayoutState {
    pub layout_dirty: bool,
    pub relayout_pending: bool,
    pub out_of_flow: bool,
    pub layout_epoch: u64,
    pub last_layout: LayoutEntry,
    pub last_pos: Position,
    pub last_x: Px,
    pub last_y: Px,
    pub last_margins: Margins,
    pub last_padding: Margins,
    pub last_borders: Margins,
    pub measured: [LayoutEntry; 2],
    pub measured_next: i32,
    pub resolved_layout_safe: i32,
}

impl Default for LayoutState {
    fn default() -> LayoutState {
        LayoutState {
            layout_dirty: true,
            relayout_pending: false,
            out_of_flow: true,
            layout_epoch: 0,
            last_layout: LayoutEntry::default(),
            last_pos: Position::default(),
            last_x: Px(0.0),
            last_y: Px(0.0),
            last_margins: Margins::default(),
            last_padding: Margins::default(),
            last_borders: Margins::default(),
            measured: [LayoutEntry::default(); 2],
            measured_next: 0,
            resolved_layout_safe: -1,
        }
    }
}

/// `scroll_view`: the padding box a box with `overflow: scroll`, `auto` or `hidden` shows and what it could scroll
/// over, with the scroll position.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScrollView {
    pub viewport: Position,
    pub scroll_size: Size,
    pub left: Px,
    pub top: Px,
}

impl ScrollView {
    /// `get_max_h_scroll()`.
    pub fn get_max_h_scroll(&self) -> Px {
        Px(0.0).max(self.scroll_size.width - self.viewport.width)
    }

    /// `get_max_v_scroll()`.
    pub fn get_max_v_scroll(&self) -> Px {
        Px(0.0).max(self.scroll_size.height - self.viewport.height)
    }

    /// `set(viewport, scroll_size)`: the scroll position kept inside the new range.
    pub fn set(&mut self, viewport: Position, scroll_size: Size) {
        self.viewport = viewport;
        self.scroll_size = scroll_size;
        self.left = clamp_px(self.left, Px(0.0), self.get_max_h_scroll());
        self.top = clamp_px(self.top, Px(0.0), self.get_max_v_scroll());
    }
}

/// `std::clamp(v, lo, hi)` with `pixel_t`'s comparisons.
pub fn clamp_px(v: Px, lo: Px, hi: Px) -> Px {
    if v < lo {
        lo
    } else if hi < v {
        hi
    } else {
        v
    }
}

/// `render_text`'s own: the pieces a wrapped text was cut into (`fragments`, which own them), a piece's text
/// (`fragment_text`) and whether the item is one (`is_fragment`), and how many bytes `text-overflow: ellipsis`
/// leaves drawn (`cut`, -1 all).
#[derive(Clone, Debug)]
pub struct TextItem {
    pub fragments: Vec<RenderId>,
    pub fragment_text: Vec<u8>,
    pub is_fragment: bool,
    pub cut: i32,
}

impl Default for TextItem {
    fn default() -> TextItem {
        TextItem { fragments: Vec::new(), fragment_text: Vec::new(), is_fragment: false, cut: -1 }
    }
}

/// `render_item_inline_context`'s own: `m_trailing`, the texts it cut into pieces (`m_fragmented_texts`), its line
/// boxes, `m_rendered_width`, `m_va_applied`, the ellipses it placed and whether its texts may be cut (`m_cut`).
#[derive(Clone, Debug, Default)]
pub struct InlineState {
    pub trailing: Px,
    pub fragmented_texts: Vec<RenderId>,
    pub line_boxes: Vec<LineBox>,
    pub rendered_width: RenderedWidth,
    pub va_applied: Px,
    pub ellipses: Vec<Position>,
    pub cut: bool,
}

/// A render item of any class: what `render_item` holds, a table's grid, and the layout state of each class (a
/// block context's `m_va_applied`, an inline's boxes, a text's pieces, an inline context's line boxes). The ink is
/// painting's (L13).
#[derive(Clone, Debug)]
pub struct RenderItem {
    pub kind: RenderKind,
    /// `m_element`.
    pub element: ElementId,
    /// `m_parent`, a weak reference.
    pub parent: Option<RenderId>,
    /// `m_children`, which own their items.
    pub children: Vec<RenderId>,
    /// `m_margins`, `m_padding` and `m_borders`: the computed style's in pixels, as the constructor sets them.
    pub margins: Margins,
    pub padding: Margins,
    pub borders: Margins,
    /// `m_pos`.
    pub pos: Position,
    /// `m_skip`.
    pub skip: bool,
    /// `collapse_skip` and `defer_borders`.
    pub collapse_skip: u8,
    pub defer_borders: bool,
    /// A `render_item_table`'s own.
    pub table: Option<Box<TableData>>,
    /// `m_positioned`, which own their items.
    pub positioned: Vec<RenderId>,
    /// `m_scroll_view`.
    pub scroll_view: Option<ScrollView>,
    /// The layout cache and its marks.
    pub layout: LayoutState,
    /// `render_item_block_context::m_va_applied`.
    pub va_applied: Px,
    /// `render_item_inline::m_boxes` (and a `render_text`'s).
    pub inline_boxes: Vec<Position>,
    /// A `render_text`'s own.
    pub text: Option<Box<TextItem>>,
    /// A `render_item_inline_context`'s own.
    pub inline: Option<Box<InlineState>>,
    /// The owners that hold the item.
    strong: u32,
    generation: u32,
    live: bool,
}

/// The document's render tree: the items, the root (`m_root_render`), the tabular items `fix_tables_layout` looks at
/// (`m_tabular_elements`, which own theirs), the table cells taken of the document's budget (`m_table_cells`,
/// `m_table_limited`) and `m_render_tree_serial`.
#[derive(Clone, Debug, Default)]
pub struct RenderTree {
    items: Vec<RenderItem>,
    free: Vec<u32>,
    pub root: Option<RenderId>,
    pub tabular: Vec<RenderId>,
    pub table_cells: i32,
    pub table_limited: bool,
    pub serial: u64,
}

impl RenderTree {
    /// The item, when it is alive.
    pub fn get(&self, id: RenderId) -> Option<&RenderItem> {
        self.items.get(id.index as usize).filter(|item| item.live && item.generation == id.generation)
    }

    /// Whether the item is alive: what `weak_ptr::lock` asks.
    pub fn alive(&self, id: RenderId) -> bool {
        self.get(id).is_some()
    }

    /// An item the tree keeps alive.
    pub fn item(&self, id: RenderId) -> &RenderItem {
        self.get(id).expect("a live render item")
    }

    pub(crate) fn item_mut(&mut self, id: RenderId) -> &mut RenderItem {
        self.items.get_mut(id.index as usize).filter(|item| item.live && item.generation == id.generation).expect("a live render item")
    }

    /// A `render_item_table`'s own.
    pub(crate) fn item_table_mut(&mut self, id: RenderId) -> &mut TableData {
        self.item_mut(id).table.get_or_insert_with(Default::default)
    }

    /// `parent()`: the parent while it is alive.
    pub fn parent(&self, id: RenderId) -> Option<RenderId> {
        self.item(id).parent.filter(|&p| self.alive(p))
    }

    /// `parent(par)`.
    pub fn set_parent(&mut self, id: RenderId, parent: Option<RenderId>) {
        self.item_mut(id).parent = parent;
    }

    /// `children()`.
    pub fn children(&self, id: RenderId) -> &[RenderId] {
        &self.item(id).children
    }

    /// How many items are alive, and how many slots the arena has.
    pub fn counts(&self) -> (usize, usize) {
        (self.items.iter().filter(|item| item.live).count(), self.items.len())
    }

    fn alloc(&mut self, mut item: RenderItem) -> RenderId {
        item.live = true;
        item.strong = 0;
        if let Some(index) = self.free.pop() {
            let slot = &mut self.items[index as usize];
            item.generation = slot.generation;
            *slot = item;
            RenderId { index, generation: slot.generation }
        } else {
            item.generation = 0;
            self.items.push(item);
            RenderId { index: (self.items.len() - 1) as u32, generation: 0 }
        }
    }

    /// An owner takes the item.
    pub(crate) fn hold(&mut self, id: RenderId) {
        let item = self.item_mut(id);
        item.strong = item.strong.saturating_add(1);
    }
}

impl RenderItem {
    /// `right()`: the margin box's right edge.
    pub fn right(&self) -> Px {
        self.left() + self.width()
    }

    /// `left()`.
    pub fn left(&self) -> Px {
        self.pos.left() - self.margins.left - self.padding.left - self.borders.left
    }

    /// `top()`.
    pub fn top(&self) -> Px {
        self.pos.top() - self.margins.top - self.padding.top - self.borders.top
    }

    /// `bottom()`.
    pub fn bottom(&self) -> Px {
        self.top() + self.height()
    }

    /// `height()`: the margin box's.
    pub fn height(&self) -> Px {
        self.pos.height + self.margins.height() + self.padding.height() + self.borders.height()
    }

    /// `width()`.
    pub fn width(&self) -> Px {
        self.pos.width + self.margins.width() + self.padding.width() + self.borders.width()
    }

    /// `content_offset_top()`: margin, padding and border above the content.
    pub fn content_offset_top(&self) -> Px {
        self.margins.top + self.padding.top + self.borders.top
    }

    /// `content_offset_bottom()`.
    pub fn content_offset_bottom(&self) -> Px {
        self.margins.bottom + self.padding.bottom + self.borders.bottom
    }

    /// `content_offset_left()`.
    pub fn content_offset_left(&self) -> Px {
        self.margins.left + self.padding.left + self.borders.left
    }

    /// `content_offset_right()`.
    pub fn content_offset_right(&self) -> Px {
        self.margins.right + self.padding.right + self.borders.right
    }

    /// `content_offset_width()`.
    pub fn content_offset_width(&self) -> Px {
        self.content_offset_left() + self.content_offset_right()
    }

    /// `content_offset_height()`.
    pub fn content_offset_height(&self) -> Px {
        self.content_offset_top() + self.content_offset_bottom()
    }
}

impl Document {
    /// The constructor of a render item of the class: the element's margins, padding and border widths in pixels
    /// (`render_item::render_item`; a table's spacing starts at 0). Kept out of the tree walks' frames.
    #[inline(never)]
    pub(crate) fn new_render_item(&mut self, cx: &StyleCx, kind: RenderKind, element: ElementId) -> RenderId {
        let units = self.units(cx);
        let css = &self.el(element).css;
        let fm = css.m_font_metrics;
        let px = |len: &crate::css_length::CssLength| units.to_pixels(len, &fm, Px(0.0));
        let m = &css.m_css_margins;
        let p = &css.m_css_padding;
        let b = &css.m_css_borders;
        let margins = Margins { left: px(&m.left), right: px(&m.right), top: px(&m.top), bottom: px(&m.bottom) };
        let padding = Margins { left: px(&p.left), right: px(&p.right), top: px(&p.top), bottom: px(&p.bottom) };
        let borders = Margins { left: px(&b.left.width), right: px(&b.right.width), top: px(&b.top.width), bottom: px(&b.bottom.width) };
        let table = (kind == RenderKind::render_item_table).then(|| Box::new(TableData::default()));
        if self.el(element).anonymous_style {
            self.el_mut(element).box_holds += 1;
        }
        self.render.alloc(RenderItem {
            kind,
            element,
            parent: None,
            children: Vec::new(),
            margins,
            padding,
            borders,
            pos: Position::default(),
            skip: false,
            collapse_skip: 0,
            defer_borders: false,
            table,
            positioned: Vec::new(),
            scroll_view: None,
            layout: LayoutState::default(),
            va_applied: Px(0.0),
            inline_boxes: Vec::new(),
            text: (kind == RenderKind::render_text).then(Box::default),
            inline: (kind == RenderKind::render_item_inline_context).then(Box::default),
            strong: 0,
            generation: 0,
            live: false,
        })
    }

    /// An owner lets the item go: with no owner left it is freed, and so are its children and its grid's items that
    /// nothing else owns, and its element when that is an anonymous one nothing keeps.
    pub(crate) fn release_render(&mut self, id: RenderId) {
        let mut pending = vec![id];
        while let Some(id) = pending.pop() {
            let Some(item) = self.render.items.get_mut(id.index as usize) else { continue };
            if !item.live || item.generation != id.generation {
                continue;
            }
            item.strong = item.strong.saturating_sub(1);
            if item.strong == 0 {
                self.free_render(id, &mut pending);
            }
        }
    }

    /// A local's end: an item made and given to no owner is freed.
    pub(crate) fn discard_render(&mut self, id: RenderId) {
        if self.render.get(id).is_some_and(|item| item.strong == 0) {
            let mut pending = Vec::new();
            self.free_render(id, &mut pending);
            while let Some(next) = pending.pop() {
                self.release_render(next);
            }
        }
    }

    /// The item's end; what it owned goes to `pending`, each to be let go once.
    fn free_render(&mut self, id: RenderId, pending: &mut Vec<RenderId>) {
        let item = &mut self.render.items[id.index as usize];
        item.live = false;
        item.generation = item.generation.wrapping_add(1);
        let element = item.element;
        pending.append(&mut item.children);
        pending.append(&mut item.positioned);
        if let Some(text) = item.text.as_mut() {
            pending.append(&mut text.fragments);
        }
        if let Some(table) = item.table.take()
            && let Some(grid) = table.grid
        {
            grid.owned(pending);
        }
        self.render.free.push(id.index);
        self.release_anonymous(element);
    }

    /// An anonymous element loses what kept it: with nothing left it is freed, and its slot used again.
    pub(crate) fn release_anonymous(&mut self, element: ElementId) {
        let mut next = Some(element);
        while let Some(id) = next.take() {
            let el = self.el_mut(id);
            if !el.anonymous_style || el.box_holds == 0 {
                return;
            }
            el.box_holds -= 1;
            if el.box_holds == 0 {
                let parent = el.parent;
                *el = crate::element::Element::new(ElementKind::html_tag);
                self.free_elements.push(id);
                next = parent;
            }
        }
    }

    /// `m_root_render = item`.
    pub(crate) fn set_root_render(&mut self, item: Option<RenderId>) {
        if let Some(item) = item {
            self.render.hold(item);
        }
        if let Some(old) = std::mem::replace(&mut self.render.root, item) {
            self.release_render(old);
        }
    }

    /// The item's `k`-th child replaced by `parts`, which the caller has made owned.
    pub(crate) fn render_children_splice(&mut self, item: RenderId, k: usize, parts: &[RenderId]) {
        self.render.item_mut(item).children.splice(k..=k, parts.iter().copied());
    }

    /// `child` put among the item's children before the `k`-th, which the caller has made owned.
    pub(crate) fn render_children_insert(&mut self, item: RenderId, k: usize, child: RenderId) {
        self.render.item_mut(item).children.insert(k, child);
    }

    /// The children `from..to` taken out of the list, for the caller to let go.
    pub(crate) fn render_children_drain(&mut self, item: RenderId, from: usize, to: usize) -> Vec<RenderId> {
        self.render.item_mut(item).children.drain(from..to).collect()
    }

    /// `add_child(ri)`: the child last, owned, with this parent.
    pub(crate) fn add_render_child(&mut self, parent: RenderId, child: RenderId) {
        self.render.hold(child);
        self.render.item_mut(parent).children.push(child);
        self.render.set_parent(child, Some(parent));
    }

    /// `m_children = children`: the new ones owned before the old ones are let go.
    pub(crate) fn set_render_children(&mut self, parent: RenderId, children: Vec<RenderId>) {
        for &child in &children {
            self.render.hold(child);
        }
        let old = std::mem::replace(&mut self.render.item_mut(parent).children, children);
        for child in old {
            self.release_render(child);
        }
    }

    /// `document::add_tabular(el)`.
    pub(crate) fn add_tabular(&mut self, item: RenderId) {
        self.render.hold(item);
        self.render.tabular.push(item);
    }

    /// `element::add_render(ri)`: the element's render items without the gone ones, and this one last.
    pub fn add_render(&mut self, element: ElementId, item: RenderId) {
        let render = &self.render;
        let el = &mut self.elements[element.0 as usize];
        el.renders.retain(|&r| render.alive(r));
        el.renders.push(item);
    }

    /// `element::get_render_item()`: the element's first render item, unless it has gone.
    pub fn get_render_item(&self, element: ElementId) -> Option<RenderId> {
        self.el(element).renders.first().copied().filter(|&r| self.render.alive(r))
    }

    /// `element::run_on_renderers`'s walk: the element's render items that are alive, in order.
    pub fn live_renders(&self, element: ElementId) -> impl Iterator<Item = RenderId> + '_ {
        self.el(element).renders.iter().copied().filter(|&r| self.render.alive(r))
    }

    /// `src_el()->css().get_display()`.
    pub(crate) fn render_display(&self, item: RenderId) -> i32 {
        self.el(self.render.item(item).element).css.m_display
    }

    // ---- What render items ask of their elements (element.h) ----------------------------------------------

    /// `is_inline()`.
    pub fn is_inline(&self, id: ElementId) -> bool {
        let d = self.el(id).css.m_display;
        d == display_inline
            || d == display_inline_table
            || d == display_inline_block
            || d == display_inline_text
            || d == display_inline_flex
            || d == display_inline_grid
    }

    /// `is_block_box()`.
    pub fn is_block_box(&self, id: ElementId) -> bool {
        let d = self.el(id).css.m_display;
        d == display_block || d == display_flex || d == display_grid || d == display_table || d == display_list_item
    }

    /// `is_float()`.
    pub fn is_float(&self, id: ElementId) -> bool {
        self.el(id).css.m_float != float_none
    }

    /// `is_table_skip()`: a space, a comment or nothing displayed.
    pub fn is_table_skip(&self, id: ElementId) -> bool {
        self.is_space(id) || self.is_comment(id) || self.el(id).css.m_display == display_none
    }

    // ---- Building ----------------------------------------------------------------------------------------

    /// `create_render_item(parent_ri)`, as the element's class has it: a comment makes none; an image and an
    /// `<svg>` a `render_item_image` whatever their display, without children; any other element the class its
    /// display asks for (none for `none` and the displays without one), the tabular ones in the document's list,
    /// then its children's (a closed `<details>` only its first `<summary>`'s).
    pub fn create_render_item(&mut self, cx: &StyleCx, el: ElementId, parent_ri: Option<RenderId>) -> Option<RenderId> {
        match self.kind(el) {
            ElementKind::el_comment => None,
            ElementKind::el_image | ElementKind::el_svg => {
                let ret = self.new_render_item(cx, RenderKind::render_item_image, el);
                self.render.set_parent(ret, parent_ri);
                Some(ret)
            }
            _ => self.element_create_render_item(cx, el, parent_ri),
        }
    }

    fn element_create_render_item(&mut self, cx: &StyleCx, el: ElementId, parent_ri: Option<RenderId>) -> Option<RenderId> {
        let d = self.el(el).css.m_display;
        let kind = if d == display_table_column
            || d == display_table_column_group
            || d == display_table_footer_group
            || d == display_table_header_group
            || d == display_table_row_group
        {
            RenderKind::render_item_table_part
        } else if d == display_table_row {
            RenderKind::render_item_table_row
        } else if d == display_block
            || d == display_table_cell
            || d == display_table_caption
            || d == display_list_item
            || d == display_inline_block
        {
            RenderKind::render_item_block
        } else if d == display_table || d == display_inline_table {
            RenderKind::render_item_table
        } else if d == display_inline {
            RenderKind::render_item_inline
        } else if d == display_inline_text {
            RenderKind::render_text
        } else if d == display_flex || d == display_inline_flex {
            RenderKind::render_item_flex
        } else if d == display_grid || d == display_inline_grid {
            RenderKind::render_item_grid
        } else {
            return None;
        };
        let ret = self.new_render_item(cx, kind, el);
        if d == display_table
            || d == display_inline_table
            || d == display_table_caption
            || d == display_table_cell
            || d == display_table_column
            || d == display_table_column_group
            || d == display_table_footer_group
            || d == display_table_header_group
            || d == display_table_row
            || d == display_table_row_group
        {
            self.add_tabular(ret);
        }

        self.render.set_parent(ret, parent_ri);
        let closed_details = self.tag(el) == _details_ && self.get_attr(el, b"open").is_none();
        let mut summary_found = false;
        let mut k = 0;
        while k < self.el(el).children.len() {
            let child = self.el(el).children[k];
            k += 1;
            if closed_details {
                if self.tag(child) != _summary_ || summary_found {
                    continue;
                }
                summary_found = true;
            }
            if let Some(ri) = self.create_render_item(cx, child, Some(ret)) {
                self.add_render_child(ret, ri);
            }
        }
        Some(ret)
    }

    /// `clone()`: a new item of the same class for the same element.
    pub(crate) fn clone_render_item(&mut self, cx: &StyleCx, item: RenderId) -> RenderId {
        let (kind, element) = {
            let it = self.render.item(item);
            (it.kind, it.element)
        };
        self.new_render_item(cx, kind, element)
    }

    /// `init()`, as the item's class has it: what the item becomes (a block becomes its formatting context).
    pub fn render_init(&mut self, cx: &StyleCx, item: RenderId) -> RenderId {
        match self.render.item(item).kind {
            RenderKind::render_item_block | RenderKind::render_item_block_context | RenderKind::render_item_inline_context => {
                self.render_block_init(cx, item)
            }
            RenderKind::render_item_flex | RenderKind::render_item_grid => self.render_flex_init(cx, item),
            RenderKind::render_item_table => self.render_table_init(cx, item),
            _ => self.render_item_init(cx, item),
        }
    }

    /// `render_item::init()`: the element knows its item, then each child becomes what its `init()` makes it.
    fn render_item_init(&mut self, cx: &StyleCx, item: RenderId) -> RenderId {
        let element = self.render.item(item).element;
        self.add_render(element, item);
        for k in 0..self.render.children(item).len() {
            self.init_render_child(cx, item, k);
        }
        item
    }

    /// `el = el->init()` on the item's `k`-th child: what it becomes takes its place.
    pub(crate) fn init_render_child(&mut self, cx: &StyleCx, item: RenderId, k: usize) {
        let child = self.render.children(item)[k];
        let made = self.render_init(cx, child);
        self.replace_render_child(item, k, made);
    }

    /// The item's `k`-th child slot takes `made`.
    pub(crate) fn replace_render_child(&mut self, item: RenderId, k: usize, made: RenderId) {
        let old = self.render.children(item)[k];
        if made != old {
            self.render.hold(made);
            self.render.item_mut(item).children[k] = made;
            self.release_render(old);
        }
    }

    /// `split_inlines()`: around the first block box (not floated) among the children, or deeper in a child that
    /// has children: a clone of the item with the children before it, the block box, and a clone with those after.
    /// A child that splits itself puts its two halves at the ends of the clones.
    pub(crate) fn split_inlines(&mut self, cx: &StyleCx, item: RenderId) -> Option<(RenderId, RenderId, RenderId)> {
        let children = self.render.children(item).to_vec();
        for &child in &children {
            let child_el = self.render.item(child).element;
            if self.is_block_box(child_el) && !self.is_float(child_el) {
                let first = self.clone_render_item(cx, item);
                let last = self.clone_render_item(cx, item);
                self.render.set_parent(child, Some(first));
                self.render.set_parent(last, Some(first));
                self.split_children(&children, child, first, last);
                return Some((first, child, last));
            }
            if !self.render.children(child).is_empty()
                && let Some((child_first, block, child_last)) = self.split_inlines(cx, child)
            {
                let first = self.clone_render_item(cx, item);
                let last = self.clone_render_item(cx, item);
                self.render.set_parent(last, Some(first));
                self.split_children(&children, child, first, last);
                self.add_render_child(first, child_first);
                self.add_render_child(last, child_last);
                return Some((first, block, last));
            }
        }
        None
    }

    /// The children before `at` to `first`, those after it to `last`.
    fn split_children(&mut self, children: &[RenderId], at: RenderId, first: RenderId, last: RenderId) {
        let mut found = false;
        for &ch in children {
            if ch == at {
                found = true;
                continue;
            }
            self.add_render_child(if found { last } else { first }, ch);
        }
    }

    /// The tree as `finalize_from_external_root` makes it after the cascade: the root's items (the table cells
    /// taken again from 0), the tabular items fixed up (`fix_tables_layout`), then the root's `init()`.
    pub fn create_render_tree(&mut self, cx: &StyleCx) {
        let Some(root) = self.root else { return };
        self.render.table_cells = 0;
        let made = self.create_render_item(cx, root, None);
        self.set_root_render(made);
        self.render.serial += 1;
        self.layout.relayout_roots.clear();
        self.fix_tables_layout(cx);
        if let Some(root_render) = self.render.root {
            let made = self.render_init(cx, root_render);
            self.set_root_render(Some(made));
        }
    }

    /// The tree as `media_changed` makes it again after a restyle: the root's items and their `init()`, the old
    /// tabular items kept in the list and the new ones added, no `fix_tables_layout`.
    pub fn recreate_render_tree_on_media_change(&mut self, cx: &StyleCx) {
        let Some(root) = self.root else { return };
        self.render.table_cells = 0;
        let made = self.create_render_item(cx, root, None);
        self.set_root_render(made);
        self.render.serial += 1;
        self.layout.relayout_roots.clear();
        if let Some(root_render) = self.render.root {
            let made = self.render_init(cx, root_render);
            self.set_root_render(Some(made));
        }
    }

    /// The tree as `restyle_after_patch` makes it again when it does not keep it: the tabular list emptied, the
    /// root's items, `fix_tables_layout`, `init()`.
    pub fn recreate_render_tree_after_patch(&mut self, cx: &StyleCx) {
        for item in std::mem::take(&mut self.render.tabular) {
            self.release_render(item);
        }
        self.create_render_tree(cx);
    }
}
