//! `render_table.h` and `render_table.cpp`, with `iterators.cpp`'s walks over a table: `render_item_table::init()`
//! builds the grid from the rows inside the table and its row groups and the cells of each row (a row group met
//! inside a row taken as a cell too, and its own cells after it; each one made what its `init()` makes it), then
//! the captions, and reads the border spacing (stage L6); `_render` lays the table out (stage L12): the collapsed
//! borders' winners, the cells measured for their columns' minimum and maximum widths (or the columns' CSS widths
//! alone in a fixed table), spanning cells' widths handed to their columns, the columns' widths and places, the
//! cells laid out at their columns' width, the rows' heights from their cells (rowspans after), the table's height
//! shared among the rows, the cells placed and vertically aligned in their rows, then the captions above and below.
//! `render_item_table_part` and `render_item_table_row` have the base class's `init()` and `_render`; a row's boxes
//! for the document's size are `layout.rs`'s. Painting (stage L13): a table's captions, rows' backgrounds and cells
//! (their borders deferred when collapsed, then drawn after the table's own), its ink and the captions' offset, a
//! row's boxes.

use crate::Px;
use crate::css_values::{BorderCollapse, BorderStyle, BoxSizing, CaptionSide, CssUnits, TableLayout};
use crate::document::{Document, StyleCx};
use crate::formatting_context::FormattingContext;
use crate::render_item::*;
use crate::table::TableGrid;
use crate::types::*;

const border_collapse_collapse: i32 = BorderCollapse::border_collapse_collapse.index();
const table_layout_fixed: i32 = TableLayout::table_layout_fixed.index();
const caption_side_top: i32 = CaptionSide::caption_side_top.index();
const caption_side_bottom: i32 = CaptionSide::caption_side_bottom.index();
const box_sizing_content_box: i32 = BoxSizing::box_sizing_content_box.index();

/// `border_rank(style)`: CSS 2.1 17.6.2.1's order of the styles, for a tie in width.
fn border_rank(style: i32) -> i32 {
    match BorderStyle::from_index(style) {
        Some(BorderStyle::border_style_double) => 8,
        Some(BorderStyle::border_style_solid) => 7,
        Some(BorderStyle::border_style_dashed) => 6,
        Some(BorderStyle::border_style_dotted) => 5,
        Some(BorderStyle::border_style_ridge) => 4,
        Some(BorderStyle::border_style_outset) => 3,
        Some(BorderStyle::border_style_groove) => 2,
        Some(BorderStyle::border_style_inset) => 1,
        _ => 0,
    }
}

/// `compare_borders(aw, as, bw, bs)`: more than 0 when border a beats b, less when b beats a, 0 on a tie: `hidden`
/// wins (a first), then the wider (`none` as 0 wide), then the style's rank.
fn compare_borders(aw: Px, a_style: i32, bw: Px, b_style: i32) -> f32 {
    let hidden = BorderStyle::border_style_hidden.index();
    let none = BorderStyle::border_style_none.index();
    if a_style == hidden || b_style == hidden {
        return if a_style == hidden { 1.0 } else { -1.0 };
    }
    let aw = if a_style == none { Px(0.0) } else { aw };
    let bw = if b_style == none { Px(0.0) } else { bw };
    if aw != bw {
        return (aw - bw).value();
    }
    (border_rank(a_style) - border_rank(b_style)) as f32
}

impl Document {
    /// `go_inside_table`: a row group of any kind.
    fn go_inside_table(&self, item: RenderId) -> bool {
        let d = self.render_display(item);
        d == display_table_row_group || d == display_table_header_group || d == display_table_footer_group
    }

    /// `render_item_table::init()`.
    pub(crate) fn render_table_init(&mut self, cx: &StyleCx, this: RenderId) -> RenderId {
        // Initialize Grid
        let old = self.render.item_table_mut(this).grid.take();
        if let Some(old) = old {
            let mut owned = Vec::new();
            old.owned(&mut owned);
            for item in owned {
                self.release_render(item);
            }
        }
        let mut grid = Box::new(TableGrid::default());

        self.table_rows(cx, this, &mut grid);

        for k in 0..self.render.children(this).len() {
            let el = self.render.children(this)[k];
            if self.render_display(el) == display_table_caption {
                self.init_render_child(cx, this, k);
                let made = self.render.children(this)[k];
                self.render.hold(made);
                grid.captions.push(made);
            }
        }

        self.grid_finish(cx, &mut grid);

        let element = self.render.item(this).element;
        let (x, y) = if self.el(element).css.m_border_collapse == border_collapse_separate {
            let units = self.units(cx);
            let css = &self.el(element).css;
            let fm = css.m_font_metrics;
            (units.to_pixels(&css.m_css_border_spacing_x, &fm, Px(0.0)), units.to_pixels(&css.m_css_border_spacing_y, &fm, Px(0.0)))
        } else {
            (Px(0.0), Px(0.0))
        };
        let table = self.render.item_table_mut(this);
        table.grid = Some(grid);
        table.border_spacing_x = x;
        table.border_spacing_y = y;

        self.add_render(element, this);

        this
    }

    /// `elements_iterator(false, &go_inside_table, &table_rows_selector)` over `container`: each row, inside the row
    /// groups too, a row of the grid with its cells.
    fn table_rows(&mut self, cx: &StyleCx, container: RenderId, grid: &mut TableGrid) {
        for k in 0..self.render.children(container).len() {
            let el = self.render.children(container)[k];
            if self.go_inside_table(el) {
                self.table_rows(cx, el, grid);
            } else if self.render_display(el) == display_table_row {
                self.grid_begin_row(grid, el);
                self.table_cells(cx, el, grid);
            }
        }
    }

    /// `elements_iterator(true, &go_inside_table, &table_cells_selector)` over a row: each cell, and a row group as it
    /// is met and then its own cells, made what its `init()` makes it and added to the grid.
    fn table_cells(&mut self, cx: &StyleCx, container: RenderId, grid: &mut TableGrid) {
        for k in 0..self.render.children(container).len() {
            let el = self.render.children(container)[k];
            if self.go_inside_table(el) {
                self.init_render_child(cx, container, k);
                let made = self.render.children(container)[k];
                self.grid_add_cell(cx, grid, made);
                self.table_cells(cx, made, grid);
            } else if self.render_display(el) == display_table_cell {
                self.init_render_child(cx, container, k);
                let made = self.render.children(container)[k];
                self.grid_add_cell(cx, grid, made);
            }
        }
    }
}

// ---- Layout (stage L12) ----------------------------------------------------------------------------------------

impl Document {
    /// `render_item_table::resolve_collapsed_borders()`: with `border-collapse: collapse`, the sides each cell leaves
    /// to the table or to its neighbour, which win the conflict (`collapse_skip`: 1 left, 2 top, 4 right, 8 bottom).
    /// The table's border beats a cell's on the table's edge only when it is strictly stronger; between two cells the
    /// one to the left or above wins a tie.
    #[inline(never)]
    fn resolve_collapsed_borders(&mut self, this: RenderId, grid: &TableGrid) {
        let (cols, rows) = (grid.cols_count, grid.rows_count);
        for row in 0..rows {
            for col in 0..cols {
                if let Some(el) = grid.cell(col, row).and_then(|cell| cell.el) {
                    self.render.item_mut(el).collapse_skip = 0;
                }
            }
        }
        for row in 0..rows {
            for col in 0..cols {
                let Some(cell) = grid.cell(col, row) else { continue };
                let Some(el) = cell.el else { continue };
                let table_borders = self.render.item(this).borders;
                let table = &self.ri_css(this).m_css_borders;
                let own = &self.ri_css(el).m_css_borders;
                let mut skip = 0u8;
                // The table's own border beats a cell only when strictly stronger.
                let right = col + 1.max(cell.colspan);
                let below = row + 1.max(cell.rowspan);
                if col == 0 && compare_borders(table_borders.left, table.left.style, cell.borders.left, own.left.style) > 0.0 {
                    skip |= 1;
                }
                if row == 0 && compare_borders(table_borders.top, table.top.style, cell.borders.top, own.top.style) > 0.0 {
                    skip |= 2;
                }
                if right >= cols && compare_borders(table_borders.right, table.right.style, cell.borders.right, own.right.style) > 0.0 {
                    skip |= 4;
                }
                if below >= rows && compare_borders(table_borders.bottom, table.bottom.style, cell.borders.bottom, own.bottom.style) > 0.0 {
                    skip |= 8;
                }
                // Between cells the one to the left or above wins a tie.
                let mut right_skip = None;
                if right < cols
                    && let Some(next) = grid.cell(right, row)
                    && let Some(next_el) = next.el
                    && next_el != el
                {
                    let other = &self.ri_css(next_el).m_css_borders;
                    if compare_borders(next.borders.left, other.left.style, cell.borders.right, own.right.style) > 0.0 {
                        skip |= 4;
                    } else {
                        right_skip = Some((next_el, 1u8));
                    }
                }
                let mut below_skip = None;
                if below < rows
                    && let Some(next) = grid.cell(col, below)
                    && let Some(next_el) = next.el
                    && next_el != el
                {
                    let other = &self.ri_css(next_el).m_css_borders;
                    if compare_borders(next.borders.top, other.top.style, cell.borders.bottom, own.bottom.style) > 0.0 {
                        skip |= 8;
                    } else {
                        below_skip = Some((next_el, 2u8));
                    }
                }
                self.render.item_mut(el).collapse_skip |= skip;
                for (item, side) in [right_skip, below_skip].into_iter().flatten() {
                    self.render.item_mut(item).collapse_skip |= side;
                }
            }
        }
    }

    /// `render_item_table::_render(x, y, containing_block_size, fmt_ctx, second_pass)`: nothing without a grid; the
    /// grid is taken from the table while its cells are laid out and given back after.
    pub(crate) fn render_table_render(
        &mut self,
        cx: &StyleCx,
        this: RenderId,
        x: Px,
        y: Px,
        containing_block_size: &ContainingBlockContext,
        fmt_ctx: &mut FormattingContext,
    ) -> RenderedWidth {
        let Some(mut grid) = self.render.item_table_mut(this).grid.take() else {
            return RenderedWidth { natural_width: Px(0.0), min_width: Px(0.0) };
        };
        let ret = self.render_table_grid(cx, this, &mut grid, x, y, containing_block_size, fmt_ctx);
        self.render.item_table_mut(this).grid = Some(grid);
        ret
    }

    /// `_render`'s body over the grid.
    #[allow(clippy::too_many_arguments)]
    #[inline(never)]
    fn render_table_grid(
        &mut self,
        cx: &StyleCx,
        this: RenderId,
        grid: &mut TableGrid,
        x: Px,
        y: Px,
        containing_block_size: &ContainingBlockContext,
        fmt_ctx: &mut FormattingContext,
    ) -> RenderedWidth {
        let self_size = self.calculate_containing_block_context(cx, this, containing_block_size);

        if self.ri_css(this).m_border_collapse == border_collapse_collapse {
            self.resolve_collapsed_borders(this, grid);
        }

        let (cols, rows) = (grid.cols_count, grid.rows_count);
        let (spacing_x, spacing_y) = {
            let table = self.render.item(this).table.as_ref().expect("a table's own");
            (table.border_spacing_x, table.border_spacing_y)
        };

        // Calculate table spacing
        let mut table_width_spacing;
        if self.ri_css(this).m_border_collapse == border_collapse_separate {
            table_width_spacing = spacing_x * Px::from_int(cols + 1);
        } else {
            table_width_spacing = Px(0.0);

            if cols != 0 {
                let borders = self.render.item(this).borders;
                table_width_spacing -= borders.left.min(grid.columns[0].border_left);
                table_width_spacing -= borders.right.min(grid.columns[cols as usize - 1].border_right);
            }

            for col in 1..cols as usize {
                table_width_spacing -= grid.columns[col].border_left.min(grid.columns[col - 1].border_right);
            }
        }

        // The minimum and maximum widths of each cell, or of each column in a fixed table.
        let fixed_layout = self.ri_css(this).m_table_layout == table_layout_fixed && self_size.width.ty != CbcValueType::auto;
        self.table_cell_widths(cx, grid, fixed_layout, &self_size, table_width_spacing, fmt_ctx);

        // For each column, determine a maximum and minimum column width from the cells that span only that column.
        // The minimum is that required by the cell with the largest minimum cell width (or the column 'width',
        // whichever is larger). The maximum is that required by the cell with the largest maximum cell width (or the
        // column 'width', whichever is larger).
        let mut col = 0;
        while col < cols && !fixed_layout {
            let column = &mut grid.columns[col as usize];
            column.max_width = Px(0.0);
            column.min_width = Px(0.0);
            for row in 0..rows as usize {
                let cell = &grid.cells[row][col as usize];
                if cell.colspan <= 1 {
                    column.max_width = column.max_width.max(cell.max_width);
                    column.min_width = column.min_width.max(cell.min_width);
                }
            }
            col += 1;
        }

        // For each cell that spans more than one column, increase the minimum widths of the columns it spans so that
        // together, they are at least as wide as the cell. Do the same for the maximum widths. If possible, widen all
        // spanned columns by approximately the same amount.
        let mut col = 0;
        while col < cols && !fixed_layout {
            for row in 0..rows as usize {
                let cell = &grid.cells[row][col as usize];
                if cell.colspan > 1 {
                    let (colspan, cell_min, cell_max) = (cell.colspan, cell.min_width, cell.max_width);
                    let mut max_total_width = grid.columns[col as usize].max_width;
                    let mut min_total_width = grid.columns[col as usize].min_width;
                    for col2 in col + 1..col + colspan {
                        max_total_width += grid.columns[col2 as usize].max_width;
                        min_total_width += grid.columns[col2 as usize].min_width;
                    }
                    if min_total_width < cell_min {
                        grid.distribute_min_width(cell_min - min_total_width, col, col + colspan - 1);
                    }
                    if max_total_width < cell_max {
                        grid.distribute_max_width(cell_max - max_total_width, col, col + colspan - 1);
                    }
                }
            }
            col += 1;
        }

        // If the 'table' or 'inline-table' element's 'width' property has a computed value (W) other than 'auto', the
        // used width is the greater of W, CAPMIN, and the minimum width required by all the columns plus cell spacing
        // or borders (MIN). If the used width is greater than MIN, the extra width should be distributed over the
        // columns.
        //
        // If the 'table' or 'inline-table' element has 'width: auto', the used width is the greater of the table's
        // containing block width, CAPMIN, and MIN. However, if either CAPMIN or the maximum width required by the
        // columns plus cell spacing or borders (MAX) is less than that of the containing block, use max(MAX, CAPMIN).
        let mut min_table_width = Px(0.0);
        let mut max_table_width = Px(0.0);
        let mut table_width = if fixed_layout {
            grid.calc_fixed_table_width(self_size.render_width.value - table_width_spacing)
        } else if self_size.width.ty == CbcValueType::absolute {
            grid.calc_table_width(self_size.render_width.value - table_width_spacing, false, &mut min_table_width, &mut max_table_width)
        } else {
            grid.calc_table_width(
                self_size.render_width.value - table_width_spacing,
                self_size.width.ty == CbcValueType::auto,
                &mut min_table_width,
                &mut max_table_width,
            )
        };

        // The table's minimum width is computed and never read.
        min_table_width += table_width_spacing;
        let _ = min_table_width;
        max_table_width += table_width_spacing;
        table_width += table_width_spacing;
        let borders = self.render.item(this).borders;
        grid.calc_horizontal_positions(&borders, self.ri_css(this).m_border_collapse, spacing_x);

        // render cells with computed width
        let row_span_found = self.table_render_cells(cx, grid, &self_size, fmt_ctx);
        if row_span_found {
            self.table_spanned_rows(grid);
        }

        // Calculate vertical table spacing
        let mut table_height_spacing;
        if self.ri_css(this).m_border_collapse == border_collapse_separate {
            table_height_spacing = spacing_y * Px::from_int(rows + 1);
        } else {
            table_height_spacing = Px(0.0);

            if rows != 0 {
                let borders = self.render.item(this).borders;
                table_height_spacing -= borders.top.min(grid.rows[0].border_top);
                table_height_spacing -= borders.bottom.min(grid.rows[rows as usize - 1].border_bottom);
            }

            for row in 1..rows as usize {
                table_height_spacing -= grid.rows[row].border_top.min(grid.rows[row - 1].border_bottom);
            }
        }

        // calculate block height
        let mut block_height = Px(0.0);
        if self_size.height.ty != CbcValueType::auto && self_size.height.value > Px(0.0) {
            let it = self.render.item(this);
            block_height = self_size.height.value - (it.padding.height() + it.borders.height());
        }

        // calculate minimum height from m_css.get_min_height()
        let css_min_height = &self.ri_css(this).m_css_min_height;
        let min_height = if !css_min_height.is_predefined() && css_min_height.units() == CssUnits::css_units_percentage {
            css_min_height.calc_percent(containing_block_size.height.px())
        } else {
            Px(css_min_height.val())
        };

        let minimum_table_height = block_height.max(min_height);

        grid.calc_rows_height(minimum_table_height - table_height_spacing, spacing_y);
        let borders = self.render.item(this).borders;
        grid.calc_vertical_positions(&borders, self.ri_css(this).m_border_collapse, spacing_y);

        // place cells vertically
        let mut table_height = self.table_place_cells(cx, grid);

        if self.ri_css(this).m_border_collapse == border_collapse_collapse {
            if rows != 0 {
                let border_bottom = self.render.item(this).borders.bottom;
                table_height -= border_bottom.min(grid.rows[rows as usize - 1].border_bottom);
            }
        } else {
            table_height += spacing_y;
        }

        // Render table captions
        // Table border doesn't round the caption, so we have to start caption in the border position
        let mut top_captions = -self.render.item(this).borders.top;

        for k in 0..grid.captions.len() {
            let caption = grid.captions[k];
            if self.ri_css(caption).m_caption_side == caption_side_top {
                let borders = self.render.item(this).borders;
                let cb = self_size.new_width(table_width + borders.left + borders.right, size_mode_normal);
                self.render(cx, caption, -borders.left, top_captions, &cb, Some(&mut *fmt_ctx), false);
                top_captions += self.render.item(caption).height();
            }
        }

        if top_captions != Px(0.0) {
            // Add border height to get the top of cells
            top_captions += self.render.item(this).borders.top;

            // Save caption height for draw_background
            grid.top_captions_height = top_captions;

            // Move table cells to the bottom side
            for row in 0..rows as usize {
                if let Some(el_row) = grid.rows[row].el_row {
                    self.render.item_mut(el_row).pos.y += top_captions;
                }
                for col in 0..cols as usize {
                    if let Some(el) = grid.cells[row][col].el {
                        self.render.item_mut(el).pos.y += top_captions;
                    }
                }
            }
        }

        let mut bottom_captions = Px(0.0);

        for k in 0..grid.captions.len() {
            let caption = grid.captions[k];
            if self.ri_css(caption).m_caption_side == caption_side_bottom {
                let borders = self.render.item(this).borders;
                let cb = self_size.new_width(table_width + borders.left + borders.right, size_mode_normal);
                self.render(cx, caption, -borders.left, table_height + top_captions + bottom_captions, &cb, Some(&mut *fmt_ctx), false);
                bottom_captions += self.render.item(caption).height();
            }
        }

        let it = self.render.item_mut(this);
        let (left, top) = (x + it.content_offset_left(), y + it.content_offset_top());
        it.pos.move_to(left, top);
        it.pos.width = table_width;
        it.pos.height = table_height + top_captions + bottom_captions;

        if self_size.width.ty != CbcValueType::absolute {
            let width = table_width.min(max_table_width) + it.content_offset_width();
            return RenderedWidth { natural_width: width, min_width: width };
        }
        let width = table_width + it.content_offset_width();
        RenderedWidth { natural_width: width, min_width: width }
    }

    /// `_render`'s first pass over the cells: in a fixed table each column's CSS width from its first row's cell of
    /// one column (its maximum width when that is not a %); in a one-column table of a known width each cell's
    /// minimum and natural widths at the table's width; else each cell's minimum width (its natural width at no
    /// content width) and maximum width (its column's CSS width, else its natural width at the table's width).
    #[inline(never)]
    fn table_cell_widths(
        &mut self,
        cx: &StyleCx,
        grid: &mut TableGrid,
        fixed_layout: bool,
        self_size: &ContainingBlockContext,
        table_width_spacing: Px,
        fmt_ctx: &mut FormattingContext,
    ) {
        let (cols, rows) = (grid.cols_count, grid.rows_count);
        // Calculate the minimum content width (MCW) of each cell: the formatted content may span any number of lines
        // but may not overflow the cell box. If the specified 'width' (W) of the cell is greater than MCW, W is the
        // minimum cell width. A value of 'auto' means that MCW is the minimum cell width.
        //
        // Also, calculate the "maximum" cell width of each cell: formatting the content without breaking lines other
        // than where explicit line breaks occur.
        if fixed_layout {
            for col in 0..cols {
                let column = &mut grid.columns[col as usize];
                column.css_width.set_predef(0);
                column.min_width = Px(0.0);
                column.max_width = Px(0.0);
                if let Some(cell) = grid.cell(col, 0)
                    && let Some(el) = cell.el
                    && cell.colspan <= 1
                {
                    let w = &self.ri_css(el).m_cell_width;
                    if !w.is_predefined() {
                        grid.columns[col as usize].css_width = w.clone();
                        if w.units() != CssUnits::css_units_percentage {
                            let mut css_w = w.calc_percent(self_size.width.px());
                            if self.ri_css(el).m_box_sizing == box_sizing_content_box {
                                css_w += self.render.item(el).content_offset_width();
                            }
                            grid.columns[col as usize].max_width = css_w;
                        }
                    }
                }
            }
        } else if cols == 1 && self_size.width.ty != CbcValueType::auto {
            for row in 0..rows {
                if let Some(el) = grid.cell(0, row).and_then(|cell| cell.el) {
                    let cb = self_size.new_width(self_size.render_width.value - table_width_spacing, size_mode_normal);
                    let rw = self.measure(cx, el, Px(0.0), Px(0.0), &cb, Some(&mut *fmt_ctx), false, None);
                    let cell = &mut grid.cells[row as usize][0];
                    cell.min_width = rw.min_width;
                    cell.max_width = rw.natural_width;
                }
            }
        } else {
            for row in 0..rows {
                for col in 0..cols {
                    let Some(el) = grid.cell(col, row).and_then(|cell| cell.el) else { continue };
                    let cb = self_size.new_width(self.render.item(el).content_offset_width(), size_mode_normal);
                    let min_width = self.measure(cx, el, Px(0.0), Px(0.0), &cb, Some(&mut *fmt_ctx), false, None).natural_width;
                    grid.cells[row as usize][col as usize].min_width = min_width;
                    let css_width = &grid.columns[col as usize].css_width;
                    let max_width = if !css_width.is_predefined() && css_width.units() != CssUnits::css_units_percentage {
                        let mut css_w = css_width.calc_percent(self_size.width.px());
                        if self.ri_css(el).m_box_sizing == box_sizing_content_box {
                            css_w += self.render.item(el).content_offset_width();
                        }
                        min_width.max(css_w)
                    } else {
                        let cb = self_size.new_width(self_size.render_width.value - table_width_spacing, size_mode_normal);
                        self.measure(cx, el, Px(0.0), Px(0.0), &cb, Some(&mut *fmt_ctx), false, None).natural_width
                    };
                    grid.cells[row as usize][col as usize].max_width = max_width;
                }
            }
        }
    }

    /// `_render`'s cells laid out at their columns' width (a second pass) and given it as their width, each row as
    /// high as its highest cell of one row (a cell's CSS height of a length counting, up to 2^20 px); whether a cell
    /// spans rows.
    #[inline(never)]
    fn table_render_cells(
        &mut self,
        cx: &StyleCx,
        grid: &mut TableGrid,
        self_size: &ContainingBlockContext,
        fmt_ctx: &mut FormattingContext,
    ) -> bool {
        let (cols, rows) = (grid.cols_count, grid.rows_count);
        let mut row_span_found = false;
        for row in 0..rows as usize {
            grid.rows[row].height = Px(0.0);
            for col in 0..cols {
                let cell = &grid.cells[row][col as usize];
                let Some(el) = cell.el else { continue };
                let rowspan = cell.rowspan;
                let mut span_col = col + cell.colspan - 1;
                if span_col >= cols {
                    span_col = cols - 1;
                }
                let cell_width = grid.columns[span_col as usize].right - grid.columns[col as usize].left;

                let cb = self_size.new_width(cell_width, size_mode_normal);
                self.render(cx, el, grid.columns[col as usize].left, Px(0.0), &cb, Some(&mut *fmt_ctx), true);
                let it = self.render.item_mut(el);
                it.pos.width = cell_width - it.content_offset_left() - it.content_offset_right();

                if rowspan <= 1 {
                    let mut cell_height = self.render.item(el).height();
                    let css_height = &self.ri_css(el).m_css_height;
                    if !css_height.is_predefined() && css_height.units() != CssUnits::css_units_percentage {
                        let mut specified = Px(css_height.val());
                        if f64::from(specified.value()).is_finite() && specified > Px(0.0) {
                            specified = specified.min(Px::from_int(1 << 20));
                            let it = self.render.item(el);
                            cell_height = cell_height.max(specified + it.content_offset_height() - self.box_sizing_height(el));
                        }
                    }
                    grid.rows[row].height = grid.rows[row].height.max(cell_height);
                } else {
                    row_span_found = true;
                }
            }
        }
        row_span_found
    }

    /// `_render`'s rows a cell spans grown at the last so that they hold it.
    #[inline(never)]
    fn table_spanned_rows(&mut self, grid: &mut TableGrid) {
        let (cols, rows) = (grid.cols_count, grid.rows_count);
        for col in 0..cols as usize {
            for row in 0..rows {
                let cell = &grid.cells[row as usize][col];
                let Some(el) = cell.el else { continue };
                let mut span_row = row + cell.rowspan - 1;
                if span_row >= rows {
                    span_row = rows - 1;
                }
                if span_row != row {
                    let mut h = Px(0.0);
                    for i in row..=span_row {
                        h += grid.rows[i as usize].height;
                    }
                    let height = self.render.item(el).height();
                    if h < height {
                        grid.rows[span_row as usize].height += height - h;
                    }
                }
            }
        }
    }

    /// `_render`'s cells placed in their rows, as high as the rows they span, and vertically aligned; the bottom of
    /// the lowest row a cell reaches.
    #[inline(never)]
    fn table_place_cells(&mut self, cx: &StyleCx, grid: &TableGrid) -> Px {
        let (cols, rows) = (grid.cols_count, grid.rows_count);
        let mut table_height = Px(0.0);
        for col in 0..cols as usize {
            for row in 0..rows {
                let cell = &grid.cells[row as usize][col];
                let Some(el) = cell.el else { continue };
                let mut span_row = row + cell.rowspan - 1;
                if span_row >= rows {
                    span_row = rows - 1;
                }
                let (top, bottom) = (grid.rows[row as usize].top, grid.rows[span_row as usize].bottom);
                let it = self.render.item_mut(el);
                it.pos.y = top + it.content_offset_top();
                it.pos.height = bottom - top - it.content_offset_top() - it.content_offset_bottom();
                table_height = table_height.max(bottom);
                self.apply_vertical_align(cx, el);
            }
        }
        table_height
    }
}

/// The table's grid, which painting finds in place (layout gives it back after the cells are laid out).
fn grid_of(doc: &Document, id: RenderId) -> Option<&TableGrid> {
    doc.render.item(id).table.as_ref().and_then(|t| t.grid.as_deref())
}

impl Document {
    /// `render_item_table::draw_children(hdc, x, y, clip, flag, zindex)`: its captions (drawn in the blocks' pass),
    /// then row by row the row's background (blocks' pass) and its cells (drawn in the blocks' pass, their borders
    /// deferred when the borders collapse), each one's children for the pass; culled cells and captions left out.
    /// With collapsed borders, in the blocks' pass, the table's borders and then each cell's, where the clip meets
    /// them.
    #[allow(clippy::too_many_arguments)]
    pub fn table_draw_children(
        &mut self,
        cx: &StyleCx,
        id: RenderId,
        hdc: u64,
        x: Px,
        y: Px,
        clip: Option<&Position>,
        flag: crate::paint::DrawFlag,
        zindex: i32,
    ) {
        use crate::paint::DrawFlag;
        let Some((captions, rows, cols)) = grid_of(self, id).map(|g| (g.captions.len(), g.rows_count, g.cols_count)) else {
            return;
        };
        let mut pos = self.render.item(id).pos;
        pos.x += x;
        pos.y += y;
        let cull = clip.is_some() && self.culls_paint();
        let collapsed = self.ri_css(id).m_border_collapse == BorderCollapse::border_collapse_collapse.index();
        let generation = self.paint_generation();
        for k in 0..captions {
            let Some(caption) = grid_of(self, id).map(|g| g.captions[k]) else { return };
            if !self.is_visible(caption) {
                continue;
            }
            if let Some(clip) = clip
                && cull
                && self.ink(caption, generation, false).misses(clip, pos.x.value(), pos.y.value())
            {
                continue;
            }
            if flag == DrawFlag::draw_block {
                self.trace_table_item(caption);
                let element = self.render.item(caption).element;
                self.draw_element(cx, element, hdc, pos.x, pos.y, clip, caption);
            }
            self.draw_children(cx, caption, hdc, pos.x, pos.y, clip, flag, zindex);
        }
        for row in 0..rows {
            let Some(el_row) = grid_of(self, id).and_then(|g| g.rows[row as usize].el_row) else { continue };
            let row_visible = self.is_visible(el_row);
            if row_visible && flag == DrawFlag::draw_block {
                let element = self.render.item(el_row).element;
                self.draw_element_background(cx, element, hdc, pos.x, pos.y, clip, el_row);
            }
            for col in 0..cols {
                let Some(cell_el) = grid_of(self, id).and_then(|g| g.cell(col, row)).and_then(|c| c.el) else { continue };
                if row_visible && self.is_visible(cell_el) {
                    if let Some(clip) = clip
                        && cull
                        && self.ink(cell_el, generation, false).misses(clip, pos.x.value(), pos.y.value())
                    {
                        continue;
                    }
                    if flag == DrawFlag::draw_block {
                        self.trace_table_item(cell_el);
                        let deferred = self.render.item(cell_el).defer_borders;
                        self.render.item_mut(cell_el).defer_borders = collapsed;
                        let element = self.render.item(cell_el).element;
                        self.draw_element(cx, element, hdc, pos.x, pos.y, clip, cell_el);
                        self.render.item_mut(cell_el).defer_borders = deferred;
                    }
                    self.draw_children(cx, cell_el, hdc, pos.x, pos.y, clip, flag, zindex);
                }
            }
        }
        if collapsed && flag == DrawFlag::draw_block {
            let it = self.render.item(id);
            let mut table_box = pos;
            table_box.grow(&it.padding);
            table_box.grow(&it.borders);
            if table_box.does_intersect(clip, false) {
                self.draw_borders(cx, id, hdc, table_box);
            }
            for row in 0..rows {
                let Some(el_row) = grid_of(self, id).and_then(|g| g.rows[row as usize].el_row) else { continue };
                if !self.is_visible(el_row) {
                    continue;
                }
                for col in 0..cols {
                    let Some(cell_el) = grid_of(self, id).and_then(|g| g.cell(col, row)).and_then(|c| c.el) else { continue };
                    if !self.is_visible(cell_el) {
                        continue;
                    }
                    let c = self.render.item(cell_el);
                    let mut bx = c.pos;
                    bx.x += pos.x;
                    bx.y += pos.y;
                    bx.grow(&c.padding);
                    bx.grow(&c.borders);
                    if bx.does_intersect(clip, false) {
                        self.draw_borders(cx, cell_el, hdc, bx);
                    }
                }
            }
        }
    }

    /// `g_paint_trace->push_back(item)` when tracing.
    fn trace_table_item(&mut self, item: RenderId) {
        if let Some(trace) = self.paint.trace.as_mut() {
            trace.push(item);
        }
    }

    /// `render_item_table::add_children_ink(generation, ink)`: its captions' ink, each row's own ink and its cells'
    /// ink, at the table's place.
    pub fn table_add_children_ink(&mut self, id: RenderId, generation: u32, ink: &mut crate::paint::PaintInk) {
        let Some((captions, rows, cols)) = grid_of(self, id).map(|g| (g.captions.len(), g.rows_count, g.cols_count)) else {
            return;
        };
        let dx = self.render.item(id).pos.x.value();
        let dy = self.render.item(id).pos.y.value();
        for k in 0..captions {
            let Some(caption) = grid_of(self, id).map(|g| g.captions[k]) else { return };
            let child = self.ink(caption, generation, false);
            ink.add_ink(&child, dx, dy);
        }
        for row in 0..rows {
            if let Some(el_row) = grid_of(self, id).and_then(|g| g.rows[row as usize].el_row) {
                self.add_self_ink(el_row, ink, dx, dy);
            }
            for col in 0..cols {
                if let Some(cell_el) = grid_of(self, id).and_then(|g| g.cell(col, row)).and_then(|c| c.el) {
                    let child = self.ink(cell_el, generation, false);
                    ink.add_ink(&child, dx, dy);
                }
            }
        }
    }

    /// `render_item_table::get_draw_vertical_offset()`: the top captions' height.
    pub fn table_draw_vertical_offset(&self, id: RenderId) -> Px {
        grid_of(self, id).map_or(Px(0.0), |g| g.top_captions_height)
    }

    /// `render_item_table_row::for_inline_boxes(process)`: each cell's box (its margin box's width, the row's
    /// padding and borders above and below), the first and last marked.
    pub fn table_row_for_inline_boxes(&self, id: RenderId, process: &mut dyn FnMut(&Position, bool, bool) -> bool) -> bool {
        let it = self.render.item(id);
        let mut prev_pos: Option<Position> = None;
        let mut first = true;
        for &el in &it.children {
            if self.ri_css(el).m_display == crate::css_values::StyleDisplay::display_table_cell.index() {
                if let Some(prev) = prev_pos {
                    if !process(&prev, first, false) {
                        return true;
                    }
                    if first {
                        first = false;
                    }
                }
                let c = self.render.item(el);
                let mut pos = Position::default();
                pos.x = c.left() + c.margins.left;
                pos.y = c.top() - it.padding.top - it.borders.top;
                pos.width = c.right() - pos.x - c.margins.right - c.margins.left;
                pos.height = c.height() + it.padding.top + it.padding.bottom + it.borders.top + it.borders.bottom;
                prev_pos = Some(pos);
            }
        }
        if let Some(prev) = prev_pos {
            process(&prev, first, true);
        }
        true
    }
}
