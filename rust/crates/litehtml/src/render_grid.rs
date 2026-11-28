//! `render_grid.h` and `render_grid.cpp` (stage L11): `render_item_grid`, a flex box's `init()` (in
//! `render_flex.rs`) and the block's baselines, with its own `_render_content`. The fork's grid is its own
//! simplification of css-grid-1: columns from `grid-template-columns` (one `auto-fill` or `auto-fit` `repeat()`
//! counted from the definite sizes, the room and the gap), rows from `grid-template-rows`' plain tracks and
//! `grid-auto-rows`; items placed by numbered lines and spans (negative lines from the explicit end), the rest
//! auto-placed row by row (sparse, a cursor that never goes back), under a budget of cells examined; the columns
//! sized from each item's min- and max-content width (spanning items grow the intrinsic tracks they cross, a
//! flexible one only), then grown to their limits, then `fr` shares with tracks frozen at their base, or `auto`
//! tracks stretched; `justify-content` between the columns; the items laid out at their area's width
//! (`justify-self`), the rows sized from their heights and, with a definite height, `fr` and `align-content`;
//! `align-self: stretch` lays an item out again at its area's height. Absolute and fixed children get their
//! in-flow pass as a flex box's do. The C++ keeps nothing of a pass: the tracks and items are locals, kept here only
//! in [`GridPass`]es when `DocLayout::grid_trace` asks for them (lhdump's probe records the same).

use crate::Px;
use crate::css_effects::{GridPlacement, GridTrackList, GridTrackSize, TrackType};
use crate::css_values::*;
use crate::cxx;
use crate::document::{Document, StyleCx};
use crate::formatting_context::FormattingContext;
use crate::render_item::*;
use crate::types::*;

const flex_align_items_auto: i32 = FlexAlignItems::flex_align_items_auto.index();
const flex_align_items_normal: i32 = FlexAlignItems::flex_align_items_normal.index();
const flex_align_items_stretch: i32 = FlexAlignItems::flex_align_items_stretch.index();
const flex_align_items_center: i32 = FlexAlignItems::flex_align_items_center.index();
const flex_align_items_end: i32 = FlexAlignItems::flex_align_items_end.index();
const flex_align_items_self_end: i32 = FlexAlignItems::flex_align_items_self_end.index();
const flex_align_items_flex_end: i32 = FlexAlignItems::flex_align_items_flex_end.index();
const flex_justify_content_normal: i32 = FlexJustifyContent::flex_justify_content_normal.index();
const flex_justify_content_stretch: i32 = FlexJustifyContent::flex_justify_content_stretch.index();
const flex_justify_content_flex_start: i32 = FlexJustifyContent::flex_justify_content_flex_start.index();
const flex_justify_content_center: i32 = FlexJustifyContent::flex_justify_content_center.index();
const flex_justify_content_end: i32 = FlexJustifyContent::flex_justify_content_end.index();
const flex_justify_content_flex_end: i32 = FlexJustifyContent::flex_justify_content_flex_end.index();
const flex_justify_content_right: i32 = FlexJustifyContent::flex_justify_content_right.index();
const flex_justify_content_space_between: i32 = FlexJustifyContent::flex_justify_content_space_between.index();
const flex_justify_content_space_around: i32 = FlexJustifyContent::flex_justify_content_space_around.index();
const flex_justify_content_space_evenly: i32 = FlexJustifyContent::flex_justify_content_space_evenly.index();
const flex_align_content_center: i32 = FlexAlignContent::flex_align_content_center.index();
const flex_align_content_end: i32 = FlexAlignContent::flex_align_content_end.index();
const flex_align_content_flex_end: i32 = FlexAlignContent::flex_align_content_flex_end.index();
const flex_align_content_space_between: i32 = FlexAlignContent::flex_align_content_space_between.index();
const flex_align_content_space_around: i32 = FlexAlignContent::flex_align_content_space_around.index();
const flex_align_content_stretch: i32 = FlexAlignContent::flex_align_content_stretch.index();

/// `grid_track`: a column or row, its size and what sizing found (`infinite` is set and never read).
#[derive(Clone, Debug)]
pub struct GridTrack {
    pub size: GridTrackSize,
    pub base: Px,
    pub limit: Px,
    pub infinite: bool,
    pub repeated: bool,
    pub occupied: bool,
    pub collapsed: bool,
    pub content: Px,
    pub min_content: Px,
}

impl Default for GridTrack {
    fn default() -> GridTrack {
        GridTrack {
            size: GridTrackSize::default(),
            base: Px(0.0),
            limit: Px(0.0),
            infinite: true,
            repeated: false,
            occupied: false,
            collapsed: false,
            content: Px(0.0),
            min_content: Px(0.0),
        }
    }
}

/// `grid_cell`: an item, its area (0-based start and span; -1 until placed) and its measures.
#[derive(Clone, Debug)]
pub struct GridCell {
    pub el: RenderId,
    pub col: i32,
    pub row: i32,
    pub col_span: i32,
    pub row_span: i32,
    pub min_content: Px,
    pub max_content: Px,
    /// `min-width: auto`.
    pub auto_min: bool,
    pub width: Px,
    pub height: Px,
}

impl GridCell {
    fn new(el: RenderId) -> GridCell {
        GridCell {
            el,
            col: -1,
            row: -1,
            col_span: 1,
            row_span: 1,
            min_content: Px(0.0),
            max_content: Px(0.0),
            auto_min: true,
            width: Px(0.0),
            height: Px(0.0),
        }
    }
}

/// One grid's `_render_content`: the locals the C++ lays the grid out with, as the pass leaves them (the heap's, so
/// that the frames of nested grids stay small).
#[derive(Clone, Debug)]
pub struct GridPass {
    pub grid: RenderId,
    pub available: Px,
    pub column_gap: Px,
    pub definite_height: bool,
    pub container_height: Px,
    pub row_gap: Px,
    pub explicit_columns: i32,
    pub explicit_rows: i32,
    pub columns: Vec<GridTrack>,
    pub rows: Vec<GridTrack>,
    pub cells: Vec<GridCell>,
    pub column_x: Vec<Px>,
    pub row_y: Vec<Px>,
    pub row_start: Px,
    pub row_between: Px,
    pub ret: RenderedWidth,
}

/// `resolve_gap(gap, base)`: a % of the base when there is one (else 0), any other length's number as it is.
fn resolve_gap(gap: &crate::css_length::CssLength, base: Px) -> Px {
    if gap.units() == CssUnits::css_units_percentage {
        return if base > Px(0.0) { gap.calc_percent(base) } else { Px(0.0) };
    }
    Px(gap.val())
}

/// `definite_size(size, available)`: the maximum's length, else the minimum's, else 0.
fn definite_size(size: &GridTrackSize, available: Px) -> Px {
    if size.max_type == TrackType::length {
        return size.max_len.calc_percent(available);
    }
    if size.min_type == TrackType::length {
        return size.min_len.calc_percent(available);
    }
    Px(0.0)
}

/// `make_tracks(list, available, gap)`: the list's tracks, its `auto-fill`/`auto-fit` repetition as many times as
/// the definite sizes and gaps fit the room (at least once, at most 1000 tracks of it) at its place.
fn make_tracks(list: &GridTrackList, available: Px, gap: Px) -> Vec<GridTrack> {
    let mut tracks = Vec::new();
    let add = |tracks: &mut Vec<GridTrack>, size: &GridTrackSize, repeated: bool| {
        tracks.push(GridTrack { size: size.clone(), repeated, ..GridTrack::default() });
    };
    let mut count = 0;
    if list.repeat_at >= 0 && !list.repeat.is_empty() {
        let mut fixed = Px(0.0);
        for size in &list.tracks {
            fixed += definite_size(size, available);
        }
        let mut step = Px(0.0);
        for size in &list.repeat {
            step += definite_size(size, available) + gap;
        }
        let fixed_count = list.tracks.len() as i32;
        let room = available - fixed - (if fixed_count > 0 { gap * Px::from_int(fixed_count) } else { Px(0.0) }) + gap;
        if step > Px(0.0) && room > Px(0.0) {
            count = cxx::f32_to_int(cxx::min_f32((room / step).value().floor(), 1000.0));
        }
        count = 1.max(count.min(1000 / list.repeat.len() as i32));
    }
    for i in 0..=list.tracks.len() {
        if i as i32 == list.repeat_at {
            for _ in 0..count {
                for size in &list.repeat {
                    add(&mut tracks, size, true);
                }
            }
        }
        if i < list.tracks.len() {
            add(&mut tracks, &list.tracks[i], false);
        }
    }
    tracks
}

/// `resolve_line(line, explicit_count)`: a line number as a 0-based track, a negative one from the explicit end
/// (not before 0); -1 for `auto`.
fn resolve_line(line: i32, explicit_count: i32) -> i32 {
    if line > 0 {
        return line - 1;
    }
    if line < 0 {
        return 0.max(explicit_count + 1 + line);
    }
    -1
}

/// `resolve_placement(placement, explicit_count, start, span)`: both lines (in either order), a start and a span, an
/// end and a span before it, or a span alone (start -1).
fn resolve_placement(placement: &GridPlacement, explicit_count: i32, start: &mut i32, span: &mut i32) {
    let mut s = resolve_line(placement.start, explicit_count);
    let mut e = resolve_line(placement.end, explicit_count);
    *span = 1.max(placement.span);
    if s >= 0 && e >= 0 {
        if e < s {
            std::mem::swap(&mut s, &mut e);
        }
        *span = 1.max(e - s);
        *start = s;
    } else if s >= 0 {
        *start = s;
    } else if e >= 0 {
        *start = 0.max(e - *span);
    } else {
        *start = -1;
    }
}

/// `occupancy`: the cells taken, row by row, and the cells placement may still examine or mark (200000): past it
/// items stack in rows of their own.
struct Occupancy {
    rows: Vec<Vec<bool>>,
    cols: i32,
    budget: i64,
}

impl Occupancy {
    fn new(cols: i32) -> Occupancy {
        Occupancy { rows: Vec::new(), cols, budget: 200000 }
    }

    fn rows(&self) -> i32 {
        self.rows.len() as i32
    }

    fn spent(&self) -> bool {
        self.budget <= 0
    }

    /// `fits(row, col, row_span, col_span)`: inside the columns and free in the rows there are, each row looked at
    /// charged its span.
    fn fits(&mut self, row: i32, col: i32, row_span: i32, col_span: i32) -> bool {
        if col < 0 || col + col_span > self.cols {
            return false;
        }
        let mut r = row;
        while r < row + row_span && r < self.rows() {
            self.budget -= i64::from(col_span);
            for c in col..col + col_span {
                if self.rows[r as usize][c as usize] {
                    return false;
                }
            }
            r += 1;
        }
        true
    }

    /// `mark(row, col, row_span, col_span)`: the area charged, and taken unless that spent the budget.
    fn mark(&mut self, row: i32, col: i32, row_span: i32, col_span: i32) {
        self.budget -= i64::from(row_span) * i64::from(col_span);
        if self.spent() {
            return;
        }
        while self.rows() < row + row_span {
            self.rows.push(vec![false; self.cols as usize]);
        }
        for r in row..row + row_span {
            for c in col..col + col_span {
                self.rows[r as usize][c as usize] = true;
            }
        }
    }
}

/// `place_cells(cells, cols)`: the items with both a row and a column marked; those with a row only put in its first
/// columns that fit (else column 0); the rest after a cursor that goes along the rows and never back (an item with
/// a column only moves it a row down when the column is behind it). Once the budget is spent, items take rows of
/// their own below the others.
fn place_cells(cells: &mut [GridCell], cols: i32) {
    let mut grid = Occupancy::new(cols);
    let mut bottom = 0;
    for cell in cells.iter() {
        if cell.row >= 0 {
            bottom = bottom.max(cell.row + cell.row_span);
        }
        if cell.row >= 0 && cell.col >= 0 {
            grid.mark(cell.row, cell.col, cell.row_span, cell.col_span);
        }
    }
    for cell in cells.iter_mut() {
        if cell.row >= 0 && cell.col < 0 {
            let mut col = 0;
            while !grid.spent() && col + cell.col_span <= cols && !grid.fits(cell.row, col, cell.row_span, cell.col_span) {
                col += 1;
            }
            cell.col = if col + cell.col_span <= cols { col } else { 0 };
            grid.mark(cell.row, cell.col, cell.row_span, cell.col_span);
        }
    }
    let mut cursor_row = 0;
    let mut cursor_col = 0;
    for cell in cells.iter_mut() {
        if cell.row >= 0 {
            continue;
        }
        if grid.spent() {
            cell.row = bottom;
            bottom += 1;
            cell.row_span = 1;
            cell.col = cell.col.max(0);
            continue;
        }
        if cell.col >= 0 {
            if cell.col < cursor_col {
                cursor_row += 1;
            }
            while !grid.spent() && !grid.fits(cursor_row, cell.col, cell.row_span, cell.col_span) {
                cursor_row += 1;
            }
            cell.row = cursor_row;
            cursor_col = cell.col + cell.col_span;
        } else {
            loop {
                if cursor_col + cell.col_span > cols {
                    cursor_col = 0;
                    cursor_row += 1;
                }
                if grid.spent() || grid.fits(cursor_row, cursor_col, cell.row_span, cell.col_span) {
                    break;
                }
                cursor_col += 1;
            }
            cell.row = cursor_row;
            cell.col = cursor_col;
            cursor_col += cell.col_span;
        }
        bottom = bottom.max(cell.row + cell.row_span);
        grid.mark(cell.row, cell.col, cell.row_span, cell.col_span);
    }
}

/// `size_columns`' sum of the bases and the gaps between the visible tracks.
fn used_width(tracks: &[GridTrack], gaps: Px) -> Px {
    let mut sum = gaps;
    for track in tracks {
        sum += track.base;
    }
    sum
}

/// `size_columns(tracks, cells, available, gap, stretch_auto)`: each track's base and limit from its lengths, grown
/// by the items of one column to their minimum (or max-content) widths, then by spanning items across the intrinsic
/// tracks they cross (the flexible ones only when they cross one; not at all with an automatic minimum then);
/// collapsed tracks 0; the free room handed to the tracks below their limits (8 rounds); then the `fr` share, tracks
/// whose base is more than their share frozen (16 rounds), or with no flexible track the room left to the `auto`
/// tracks when `stretch_auto`.
fn size_columns(tracks: &mut [GridTrack], cells: &[GridCell], available: Px, gap: Px, stretch_auto: bool) {
    for track in tracks.iter_mut() {
        track.base = Px(0.0);
        track.limit = Px(0.0);
        track.infinite = true;
        if track.size.min_type == TrackType::length {
            track.base = track.size.min_len.calc_percent(available);
        }
        if track.size.max_type == TrackType::length {
            track.limit = track.base.max(track.size.max_len.calc_percent(available));
            track.infinite = false;
        }
    }
    for cell in cells {
        if cell.col_span != 1 {
            continue;
        }
        let track = &mut tracks[cell.col as usize];
        track.content = track.content.max(cell.max_content);
        track.min_content = track.min_content.max(cell.min_content);
        match track.size.min_type {
            TrackType::automatic | TrackType::min_content => track.base = track.base.max(cell.min_content),
            TrackType::max_content => track.base = track.base.max(cell.max_content),
            _ => {}
        }
        if track.size.max_type == TrackType::automatic || track.size.max_type == TrackType::max_content {
            track.limit = track.limit.max(cell.max_content);
        } else if track.size.max_type == TrackType::min_content {
            track.limit = track.limit.max(cell.min_content);
        }
    }
    for cell in cells {
        if cell.col_span == 1 {
            continue;
        }
        let area = cell.col as usize..(cell.col + cell.col_span) as usize;
        // Across a flexible track an item's automatic minimum is zero (css-grid 6.6), and a minimum of its own
        // grows the flexible tracks only (css-grid 12.5).
        let mut spans_flexible = false;
        for track in &tracks[area.clone()] {
            spans_flexible = spans_flexible || (track.size.max_type == TrackType::fraction && !track.collapsed);
        }
        if spans_flexible && cell.auto_min {
            continue;
        }
        let grows = |track: &GridTrack| {
            track.size.min_type != TrackType::length && !track.collapsed && (!spans_flexible || track.size.max_type == TrackType::fraction)
        };
        let mut spanned = gap * Px::from_int(cell.col_span - 1);
        let mut intrinsic = 0;
        for track in &tracks[area.clone()] {
            spanned += track.base;
            if grows(track) {
                intrinsic += 1;
            }
        }
        if intrinsic > 0 && cell.min_content > spanned {
            let share = (cell.min_content - spanned) / Px::from_int(intrinsic);
            for track in &mut tracks[area] {
                if grows(track) {
                    track.base += share;
                }
            }
        }
    }
    for track in tracks.iter_mut() {
        if track.collapsed {
            track.base = Px(0.0);
            track.limit = Px(0.0);
            continue;
        }
        if track.size.max_type != TrackType::length && track.size.max_type != TrackType::fraction {
            track.infinite = false;
        }
        track.limit = track.limit.max(track.base);
    }

    let visible = tracks.iter().filter(|track| !track.collapsed).count() as i32;
    let gaps = if visible > 1 { gap * Px::from_int(visible - 1) } else { Px(0.0) };

    for _guard in 0..8 {
        let free = available - used_width(tracks, gaps);
        if free <= Px(0.5) {
            break;
        }
        let growing = tracks
            .iter()
            .filter(|track| !track.collapsed && track.size.max_type != TrackType::fraction && track.base < track.limit)
            .count() as i32;
        if growing == 0 {
            break;
        }
        let share = free / Px::from_int(growing);
        for track in tracks.iter_mut() {
            if !track.collapsed && track.size.max_type != TrackType::fraction && track.base < track.limit {
                track.base = track.limit.min(track.base + share);
            }
        }
    }

    let flexible: Vec<usize> =
        (0..tracks.len()).filter(|&k| !tracks[k].collapsed && tracks[k].size.max_type == TrackType::fraction).collect();
    if !flexible.is_empty() {
        let mut frozen = vec![false; flexible.len()];
        let mut fr_size = Px(0.0);
        for _guard in 0..16 {
            let mut rest = available - gaps;
            let mut sum_fr: f32 = 0.0;
            for track in tracks.iter() {
                if track.collapsed || track.size.max_type == TrackType::fraction {
                    continue;
                }
                rest -= track.base;
            }
            for (i, &k) in flexible.iter().enumerate() {
                if frozen[i] {
                    rest -= tracks[k].base;
                } else {
                    sum_fr += tracks[k].size.fr;
                }
            }
            fr_size = if sum_fr > 1.0 { rest / Px(sum_fr) } else { rest };
            let mut changed = false;
            for (i, &k) in flexible.iter().enumerate() {
                if !frozen[i] && fr_size * Px(tracks[k].size.fr) < tracks[k].base {
                    frozen[i] = true;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        for (i, &k) in flexible.iter().enumerate() {
            if !frozen[i] {
                tracks[k].base = tracks[k].base.max(fr_size * Px(tracks[k].size.fr));
            }
        }
    } else if stretch_auto {
        let free = available - used_width(tracks, gaps);
        let autos = tracks.iter().filter(|track| !track.collapsed && track.size.max_type == TrackType::automatic).count() as i32;
        if free > Px(0.0) && autos > 0 {
            let share = free / Px::from_int(autos);
            for track in tracks.iter_mut() {
                if !track.collapsed && track.size.max_type == TrackType::automatic {
                    track.base += share;
                }
            }
        }
    }
}

/// `distribute_content(free, justify, count, start, between)`: where `justify-content` puts the first track and what
/// it adds between two, for positive free room.
fn distribute_content(free: Px, justify: i32, count: i32) -> (Px, Px) {
    let mut start = Px(0.0);
    let mut between = Px(0.0);
    if free <= Px(0.0) || count <= 0 {
        return (start, between);
    }
    match justify {
        flex_justify_content_center => start = free / Px(2.0),
        flex_justify_content_end | flex_justify_content_flex_end | flex_justify_content_right => start = free,
        flex_justify_content_space_between => between = if count > 1 { free / Px::from_int(count - 1) } else { Px(0.0) },
        flex_justify_content_space_around => {
            between = free / Px::from_int(count);
            start = between / Px(2.0);
        }
        flex_justify_content_space_evenly => {
            between = free / Px::from_int(count + 1);
            start = between;
        }
        _ => {}
    }
    (start, between)
}

/// `is_stretch(align)`: `normal`, `stretch` or `auto`, whatever the modifiers.
fn is_stretch(align: i32) -> bool {
    let align = align & 0xFF;
    align == flex_align_items_normal || align == flex_align_items_stretch || align == flex_align_items_auto
}

/// `align_offset(align, free)`: the free room before an item `center`, `end`, `flex-end` or `self-end` aligns.
fn align_offset(align: i32, free: Px) -> Px {
    match align & 0xFF {
        flex_align_items_center => free / Px(2.0),
        flex_align_items_end | flex_align_items_flex_end | flex_align_items_self_end => free,
        _ => Px(0.0),
    }
}

impl GridPass {
    /// `area_width(cell)`: from the start of its first column to the end of its last.
    fn area_width(&self, cell: &GridCell) -> Px {
        let last = (cell.col + cell.col_span - 1) as usize;
        self.column_x[last] + self.columns[last].base - self.column_x[cell.col as usize]
    }

    /// `area_h`: from the top of its first row to the bottom of its last.
    fn area_height(&self, cell: &GridCell) -> Px {
        let last = (cell.row + cell.row_span - 1) as usize;
        self.row_y[last] + self.rows[last].base - self.row_y[cell.row as usize]
    }
}

impl Document {
    /// `render_item_grid::_render_content(x, y, second_pass, self_size, fmt_ctx)`: the gaps (a % row gap only
    /// against a definite height), the columns, the items placed, measured and the columns sized, the widths it
    /// reports (each visible column's fixed or content width, the gaps), the columns placed by `justify-content`,
    /// the items laid out at their areas' widths, the rows sized and placed, the items placed and stretched; the box
    /// as high as its rows, at `x` and `y`. An item's % height resolves against the grid's height only when it has
    /// one.
    pub(crate) fn grid_render_content(
        &mut self,
        cx: &StyleCx,
        this: RenderId,
        x: Px,
        y: Px,
        self_size: &ContainingBlockContext,
        fmt_ctx: &mut FormattingContext,
    ) -> RenderedWidth {
        let mut g = self.grid_begin(this, self_size);
        // A % height of an item resolves against this grid's height only when it has one: not an ancestor's.
        let mut item_size = *self_size;
        if self_size.height.ty == CbcValueType::auto {
            item_size.height = TypedPixel::new(Px(0.0), CbcValueType::auto);
        }

        self.grid_cells(cx, this, &mut g, self_size, fmt_ctx);
        self.grid_place(this, &mut g);
        self.grid_measure(cx, &mut g, &item_size, fmt_ctx);
        let ret = self.grid_columns(this, &mut g);
        self.grid_widths(cx, this, &mut g, &item_size, fmt_ctx);
        self.grid_rows(this, &mut g);
        self.grid_align(cx, this, &g, self_size, fmt_ctx);

        let height = g.row_y[g.rows.len()];
        let it = self.render.item_mut(this);
        it.pos.height = height;
        it.pos.move_to(x, y);
        it.pos.x += it.content_offset_left();
        it.pos.y += it.content_offset_top();

        if let Some(trace) = self.layout.grid_trace.as_mut() {
            g.ret = ret;
            trace.push(*g);
        }
        ret
    }

    /// The pass begun: the available width, the gaps, the definite height and the explicit columns and rows.
    #[inline(never)]
    fn grid_begin(&self, this: RenderId, self_size: &ContainingBlockContext) -> Box<GridPass> {
        let css = self.ri_css(this);
        let available = Px(0.0).max(self_size.render_width.px());
        let column_gap = resolve_gap(&css.m_column_gap, available);
        let definite_height = self_size.height.ty == CbcValueType::absolute && self_size.height.value > Px(0.0);
        let container_height = if definite_height { self_size.height.value - self.box_sizing_height(this) } else { Px(0.0) };
        let row_gap = resolve_gap(&css.m_row_gap, container_height);
        let columns = make_tracks(&css.m_grid_template_columns, available, column_gap);
        Box::new(GridPass {
            grid: this,
            available,
            column_gap,
            definite_height,
            container_height,
            row_gap,
            explicit_columns: columns.len() as i32,
            explicit_rows: css.m_grid_template_rows.tracks.len() as i32,
            columns,
            rows: Vec::new(),
            cells: Vec::new(),
            column_x: Vec::new(),
            row_y: Vec::new(),
            row_start: Px(0.0),
            row_between: Px(0.0),
            ret: RenderedWidth::default(),
        })
    }

    /// The children: absolute and fixed ones given their in-flow pass (`render_positioned` takes the shrink-to-fit
    /// size from it), the others items with their lines resolved, stable-sorted by `order`.
    #[inline(never)]
    fn grid_cells(
        &mut self,
        cx: &StyleCx,
        this: RenderId,
        g: &mut GridPass,
        self_size: &ContainingBlockContext,
        fmt_ctx: &mut FormattingContext,
    ) {
        let mut orders = Vec::new();
        let mut k = 0;
        while k < self.render.children(this).len() {
            let el = self.render.children(this)[k];
            k += 1;
            let (position, display) = {
                let el_css = self.ri_css(el);
                (el_css.m_el_position, el_css.m_display)
            };
            if position == element_position_absolute || position == element_position_fixed {
                // Out of flow, but render_positioned() takes the shrink-to-fit size from this pass, as the
                // block context does (render_block_context.cpp); left unrendered the box stays 0 x 0.
                if display != display_none {
                    self.render_out_of_flow(cx, el, Px(0.0), self_size, fmt_ctx);
                }
                continue;
            }
            let el_css = self.ri_css(el);
            let mut cell = GridCell::new(el);
            resolve_placement(&el_css.m_grid_column, g.explicit_columns, &mut cell.col, &mut cell.col_span);
            resolve_placement(&el_css.m_grid_row, g.explicit_rows, &mut cell.row, &mut cell.row_span);
            orders.push(el_css.m_order);
            g.cells.push(cell);
        }
        let mut sorted: Vec<(i32, GridCell)> = orders.into_iter().zip(g.cells.drain(..)).collect();
        sorted.sort_by_key(|(order, _)| *order);
        g.cells.extend(sorted.into_iter().map(|(_, cell)| cell));
    }

    /// The columns grown to the items' areas, the items placed, the rows counted, the columns marked occupied and,
    /// for `auto-fit`, the repeated ones no item occupies collapsed.
    #[inline(never)]
    fn grid_place(&self, this: RenderId, g: &mut GridPass) {
        let mut column_count = g.explicit_columns.max(1);
        for cell in &g.cells {
            column_count = column_count.max((if cell.col >= 0 { cell.col } else { 0 }) + cell.col_span);
        }
        while (g.columns.len() as i32) < column_count {
            g.columns.push(GridTrack::default());
        }
        place_cells(&mut g.cells, column_count);

        let mut row_count = g.explicit_rows;
        for cell in &g.cells {
            row_count = row_count.max(cell.row + cell.row_span);
            for c in cell.col..cell.col + cell.col_span {
                g.columns[c as usize].occupied = true;
            }
        }
        g.rows = vec![GridTrack::default(); row_count as usize];
        if self.ri_css(this).m_grid_template_columns.auto_fit {
            for track in g.columns.iter_mut() {
                track.collapsed = track.repeated && !track.occupied;
            }
        }
    }

    /// Each item laid out to measure it: one with a fixed width at the available width (its width both measures),
    /// any other in content mode (a replaced one with a % width compresses to nothing); a `min-width` replaces the
    /// minimum.
    #[inline(never)]
    fn grid_measure(&mut self, cx: &StyleCx, g: &mut GridPass, item_size: &ContainingBlockContext, fmt_ctx: &mut FormattingContext) {
        for k in 0..g.cells.len() {
            let el = g.cells[k].el;
            let width = &self.ri_css(el).m_css_width;
            let predefined = width.is_predefined();
            if !predefined && width.units() != CssUnits::css_units_percentage && width.units() != CssUnits::css_units_calc {
                self.render(cx, el, Px(0.0), Px(0.0), &item_size.new_width(g.available, size_mode_normal), Some(&mut *fmt_ctx), false);
                let w = self.render.item(el).width();
                g.cells[k].max_content = w;
                g.cells[k].min_content = w;
            } else {
                let measured =
                    self.render(cx, el, Px(0.0), Px(0.0), &item_size.new_width(g.available, size_mode_content), Some(&mut *fmt_ctx), false);
                g.cells[k].max_content = measured.natural_width;
                // A replaced element with a % width compresses to nothing (css-sizing 5.2.2): its % is of the area.
                let element = self.render.item(el).element;
                g.cells[k].min_content = if self.is_replaced(element) && !predefined { Px(0.0) } else { measured.min_width };
            }
            let minimum = &self.ri_css(el).m_css_min_width;
            g.cells[k].auto_min = minimum.is_predefined();
            if !minimum.is_predefined() {
                g.cells[k].min_content = Px(0.0).max(minimum.calc_percent(g.available) - self.box_sizing_width(el))
                    + self.render.item(el).content_offset_width();
            }
        }
    }

    /// The columns sized (`auto` ones stretched under `justify-content` `normal`, `stretch` or `flex-start`), the
    /// widths the grid reports, and the columns placed.
    #[inline(never)]
    fn grid_columns(&self, this: RenderId, g: &mut GridPass) -> RenderedWidth {
        let justify = self.ri_css(this).m_flex_justify_content;
        let stretch_auto =
            justify == flex_justify_content_normal || justify == flex_justify_content_stretch || justify == flex_justify_content_flex_start;
        size_columns(&mut g.columns, &g.cells, g.available, g.column_gap, stretch_auto);

        let mut ret = RenderedWidth::default();
        {
            let mut visible = 0;
            for track in &g.columns {
                if track.collapsed {
                    continue;
                }
                visible += 1;
                let fixed_min =
                    if track.size.min_type == TrackType::length { track.size.min_len.calc_percent(g.available) } else { Px(0.0) };
                let fixed_max =
                    if track.size.max_type == TrackType::length { track.size.max_len.calc_percent(g.available) } else { Px(0.0) };
                if track.size.max_type == TrackType::length {
                    ret.natural_width += fixed_min.max(fixed_max);
                } else {
                    ret.natural_width += fixed_min.max(track.content);
                }
                ret.min_width += fixed_min.max(track.min_content);
            }
            let gaps = if visible > 1 { g.column_gap * Px::from_int(visible - 1) } else { Px(0.0) };
            ret.natural_width += gaps;
            ret.min_width += gaps;
        }

        g.column_x = vec![Px(0.0); g.columns.len() + 1];
        let mut used = Px(0.0);
        let mut visible = 0;
        for track in &g.columns {
            if !track.collapsed {
                used += track.base;
                visible += 1;
            }
        }
        used += if visible > 1 { g.column_gap * Px::from_int(visible - 1) } else { Px(0.0) };
        let (start, between) = distribute_content(g.available - used, justify, visible);
        let mut pos = start;
        let mut first = true;
        for c in 0..g.columns.len() {
            if !g.columns[c].collapsed {
                if !first {
                    pos += g.column_gap + between;
                }
                first = false;
            }
            g.column_x[c] = pos;
            pos += g.columns[c].base;
        }
        let n = g.columns.len();
        g.column_x[n] = pos;
        ret
    }

    /// Each item laid out at its area's width: one with a CSS width at the area's width, any other at an exact
    /// width, the area's when it stretches (`justify-self`, else the grid's `justify-items`), else its max-content
    /// width up to the area's; its outer width and height kept.
    #[inline(never)]
    fn grid_widths(
        &mut self,
        cx: &StyleCx,
        this: RenderId,
        g: &mut GridPass,
        item_size: &ContainingBlockContext,
        fmt_ctx: &mut FormattingContext,
    ) {
        let justify_items = self.ri_css(this).m_justify_items;
        for k in 0..g.cells.len() {
            let el = g.cells[k].el;
            let item_css = self.ri_css(el);
            let justify_self = if item_css.m_justify_self == flex_align_items_auto { justify_items } else { item_css.m_justify_self };
            let width_predefined = item_css.m_css_width.is_predefined();
            let area = g.area_width(&g.cells[k]);
            if !width_predefined {
                self.render(cx, el, Px(0.0), Px(0.0), &item_size.new_width(area, size_mode_normal), Some(&mut *fmt_ctx), false);
            } else {
                let outer = if is_stretch(justify_self) { area } else { area.min(g.cells[k].max_content) };
                let cb = item_size.new_width(outer - self.render_offset_width(el), size_mode_exact_width);
                self.render(cx, el, Px(0.0), Px(0.0), &cb, Some(&mut *fmt_ctx), false);
            }
            let it = self.render.item(el);
            g.cells[k].width = it.width();
            g.cells[k].height = it.height();
        }
    }

    /// The rows: the template's, then `grid-auto-rows` in turn; their lengths (% of a definite height) as bases,
    /// grown to the items of one row, then by spanning items across their content-sized rows (else the last); with
    /// a definite height the `fr` share (rows above it frozen, 16 rounds) and `align-content` (stretch grows the
    /// `auto` rows; no `space-evenly`); the rows placed.
    #[inline(never)]
    fn grid_rows(&self, this: RenderId, g: &mut GridPass) {
        let css = self.ri_css(this);
        let template_rows = &css.m_grid_template_rows;
        let auto_rows = &css.m_grid_auto_rows;
        let row_count = g.rows.len() as i32;
        let explicit_rows = g.explicit_rows;
        let container_height = g.container_height;
        for r in 0..row_count {
            let row = &mut g.rows[r as usize];
            if r < explicit_rows {
                row.size = template_rows.tracks[r as usize].clone();
            } else if !auto_rows.tracks.is_empty() {
                row.size = auto_rows.tracks[(r - explicit_rows) as usize % auto_rows.tracks.len()].clone();
            }
            if row.size.min_type == TrackType::length {
                row.base = row.size.min_len.calc_percent(container_height);
            }
            if row.size.max_type == TrackType::length {
                row.base = row.base.max(row.size.max_len.calc_percent(container_height));
            }
        }
        let content_sized = |track: &GridTrack| track.size.max_type != TrackType::length;
        for cell in &g.cells {
            let row = &mut g.rows[cell.row as usize];
            if cell.row_span == 1 && content_sized(row) {
                row.base = row.base.max(cell.height);
            }
        }
        let row_gap = g.row_gap;
        for cell in &g.cells {
            if cell.row_span == 1 {
                continue;
            }
            let area = cell.row as usize..(cell.row + cell.row_span) as usize;
            let mut spanned = row_gap * Px::from_int(cell.row_span - 1);
            let mut flexible_rows = 0;
            for row in &g.rows[area.clone()] {
                spanned += row.base;
                if content_sized(row) {
                    flexible_rows += 1;
                }
            }
            if cell.height > spanned {
                if flexible_rows == 0 {
                    g.rows[area.end - 1].base += cell.height - spanned;
                } else {
                    let share = (cell.height - spanned) / Px::from_int(flexible_rows);
                    for row in &mut g.rows[area] {
                        if content_sized(row) {
                            row.base += share;
                        }
                    }
                }
            }
        }
        if g.definite_height && row_count > 0 {
            // As the flexible columns: a share per fr, and a row whose content is more than its share keeps its content.
            let rows = &mut g.rows;
            let mut frozen = vec![false; rows.len()];
            let mut fr_size = Px(0.0);
            for _guard in 0..16 {
                let mut rest = container_height - row_gap * Px::from_int(row_count - 1);
                let mut sum_fr: f32 = 0.0;
                for r in 0..rows.len() {
                    if rows[r].size.max_type != TrackType::fraction || frozen[r] {
                        rest -= rows[r].base;
                    } else {
                        sum_fr += rows[r].size.fr;
                    }
                }
                fr_size = if sum_fr > 1.0 { rest / Px(sum_fr) } else { rest };
                let mut changed = false;
                for r in 0..rows.len() {
                    if rows[r].size.max_type == TrackType::fraction && !frozen[r] && fr_size * Px(rows[r].size.fr) < rows[r].base {
                        frozen[r] = true;
                        changed = true;
                    }
                }
                if !changed {
                    break;
                }
            }
            for r in 0..rows.len() {
                if rows[r].size.max_type == TrackType::fraction && !frozen[r] {
                    rows[r].base = rows[r].base.max(fr_size * Px(rows[r].size.fr));
                }
            }
        }

        let mut row_start = Px(0.0);
        let mut row_between = Px(0.0);
        if g.definite_height && row_count > 0 {
            let mut used = row_gap * Px::from_int(row_count - 1);
            for row in &g.rows {
                used += row.base;
            }
            let free = container_height - used;
            let align = css.m_flex_align_content;
            if free > Px(0.0) && align == flex_align_content_stretch {
                let stretchy = g.rows.iter().filter(|row| row.size.max_type == TrackType::automatic).count() as i32;
                for row in g.rows.iter_mut() {
                    if stretchy > 0 && row.size.max_type == TrackType::automatic {
                        row.base += free / Px::from_int(stretchy);
                    }
                }
            } else if free > Px(0.0) {
                match align {
                    flex_align_content_center => row_start = free / Px(2.0),
                    flex_align_content_end | flex_align_content_flex_end => row_start = free,
                    flex_align_content_space_between => {
                        row_between = if row_count > 1 { free / Px::from_int(row_count - 1) } else { Px(0.0) };
                    }
                    flex_align_content_space_around => {
                        row_between = free / Px::from_int(row_count);
                        row_start = row_between / Px(2.0);
                    }
                    _ => {}
                }
            }
        }
        g.row_start = row_start;
        g.row_between = row_between;

        g.row_y = vec![Px(0.0); g.rows.len() + 1];
        let mut pos = row_start;
        for r in 0..g.rows.len() {
            g.row_y[r] = pos;
            pos += g.rows[r].base + row_gap + row_between;
        }
        let n = g.rows.len();
        g.row_y[n] = if g.rows.is_empty() { Px(0.0) } else { pos - row_gap - row_between };
    }

    /// Each item stretched to its area's height when it stretches (`align-self`, else the grid's `align-items`), has
    /// an `auto` height and is shorter (laid out again at an exact width and height in the grid's own containing
    /// block), then placed in its area by `justify-self` and `align-self`.
    #[inline(never)]
    fn grid_align(
        &mut self,
        cx: &StyleCx,
        this: RenderId,
        g: &GridPass,
        self_size: &ContainingBlockContext,
        fmt_ctx: &mut FormattingContext,
    ) {
        let css = self.ri_css(this);
        let (align_items, justify_items) = (css.m_flex_align_items, css.m_justify_items);
        for cell in &g.cells {
            let el = cell.el;
            let item_css = self.ri_css(el);
            let align_self = if item_css.m_flex_align_self == flex_align_items_auto { align_items } else { item_css.m_flex_align_self };
            let justify_self = if item_css.m_justify_self == flex_align_items_auto { justify_items } else { item_css.m_justify_self };
            let height_predefined = item_css.m_css_height.is_predefined();
            let area_w = g.area_width(cell);
            let area_h = g.area_height(cell);
            if is_stretch(align_self) && height_predefined && cell.height < area_h {
                let it = self.render.item(el);
                let cb = self_size.new_width_height(
                    it.pos.width + self.box_sizing_width(el),
                    area_h - it.content_offset_height() + self.box_sizing_height(el),
                    size_mode_exact_width | size_mode_exact_height,
                );
                self.render(cx, el, Px(0.0), Px(0.0), &cb, Some(&mut *fmt_ctx), false);
            }
            let it = self.render.item(el);
            let offset_x = align_offset(justify_self, area_w - it.width());
            let offset_y = if is_stretch(align_self) { Px(0.0) } else { align_offset(align_self, area_h - it.height()) };
            let x = g.column_x[cell.col as usize] + offset_x + it.content_offset_left();
            let y = g.row_y[cell.row as usize] + offset_y + it.content_offset_top();
            let it = self.render.item_mut(el);
            it.pos.x = x;
            it.pos.y = y;
        }
    }
}
