//! `table.h` and `table.cpp`: a table's grid of rows, cells and columns as `render_item_table::init()` builds it
//! (`begin_row`, `add_cell` with spans and the document's cell budget, `is_rowspanned`, `finish` with
//! `limit_columns`, the columns' border widths and widths, which `finish` writes into its cells' computed widths),
//! and what layout (stage L12) asks of it: a spanning cell's width handed to its columns (`distribute_min_width`,
//! `distribute_max_width`), the columns' widths in an auto and a fixed table (`calc_table_width`,
//! `calc_fixed_table_width`), the rows' heights (`calc_rows_height`) and the columns' and rows' places. The
//! three-step `distribute_width(width, start, end)` and `clear()` are not ported: nothing calls them.

use crate::Px;
use crate::css_length::CssLength;
use crate::css_values::{BorderCollapse, CssUnits};
use crate::cxx::{max_f32, max_list_f32, min_f32};
use crate::document::{Document, StyleCx};
use crate::render_item::RenderId;
use crate::types::Margins;

/// `table_row`.
#[derive(Clone, Debug)]
pub struct TableRow {
    pub height: Px,
    pub border_top: Px,
    pub border_bottom: Px,
    /// `el_row`, which the grid owns.
    pub el_row: Option<RenderId>,
    pub top: Px,
    pub bottom: Px,
    pub css_height: CssLength,
    pub min_height: Px,
}

/// `table_row()`: zeros, no row, an `auto` height.
impl Default for TableRow {
    fn default() -> TableRow {
        TableRow {
            height: Px(0.0),
            border_top: Px(0.0),
            border_bottom: Px(0.0),
            el_row: None,
            top: Px(0.0),
            bottom: Px(0.0),
            css_height: CssLength::predef_value(0),
            min_height: Px(0.0),
        }
    }
}

/// `table_column`.
#[derive(Clone, Debug)]
pub struct TableColumn {
    pub min_width: Px,
    pub max_width: Px,
    pub width: Px,
    pub css_width: CssLength,
    pub border_left: Px,
    pub border_right: Px,
    pub left: Px,
    pub right: Px,
}

/// `table_column(min_w, max_w)` of zeros: an `auto` width.
impl Default for TableColumn {
    fn default() -> TableColumn {
        TableColumn {
            min_width: Px(0.0),
            max_width: Px(0.0),
            width: Px(0.0),
            css_width: CssLength::predef_value(0),
            border_left: Px(0.0),
            border_right: Px(0.0),
            left: Px(0.0),
            right: Px(0.0),
        }
    }
}

/// `table_cell`: a cell's item (owned) or none for a place a span covers or a short row fills, its spans, its sizes
/// and its border widths.
#[derive(Clone, Debug)]
pub struct TableCell {
    pub el: Option<RenderId>,
    pub colspan: i32,
    pub rowspan: i32,
    pub min_width: Px,
    pub min_height: Px,
    pub max_width: Px,
    pub max_height: Px,
    pub width: Px,
    pub height: Px,
    pub borders: Margins,
}

impl Default for TableCell {
    fn default() -> TableCell {
        TableCell {
            el: None,
            colspan: 1,
            rowspan: 1,
            min_width: Px(0.0),
            min_height: Px(0.0),
            max_width: Px(0.0),
            max_height: Px(0.0),
            width: Px(0.0),
            height: Px(0.0),
            borders: Margins::default(),
        }
    }
}

/// `table_grid`.
#[derive(Clone, Debug, Default)]
pub struct TableGrid {
    pub rows_count: i32,
    pub cols_count: i32,
    /// `m_cells`: the rows' cells.
    pub cells: Vec<Vec<TableCell>>,
    pub columns: Vec<TableColumn>,
    pub rows: Vec<TableRow>,
    pub top_captions_height: Px,
    pub bottom_captions_height: Px,
    /// `m_captions`, owned.
    pub captions: Vec<RenderId>,
}

impl TableGrid {
    /// `cell(t_col, t_row)`: the cell, none outside the grid.
    pub fn cell(&self, t_col: i32, t_row: i32) -> Option<&TableCell> {
        if t_col >= 0 && t_col < self.cols_count && t_row >= 0 && t_row < self.rows_count {
            return self.cells.get(t_row as usize).and_then(|row| row.get(t_col as usize));
        }
        None
    }

    /// `is_rowspanned(r, c)`: a cell of a row above spans down to row `r` at column `c`.
    pub fn is_rowspanned(&self, r: i32, c: i32) -> bool {
        let mut row = r - 1;
        while row >= 0 {
            if let Some(cells) = self.cells.get(row as usize)
                && c < cells.len() as i32
            {
                let rowspan = cells[c as usize].rowspan;
                // `rowspan >= r - row + 1`
                if rowspan > 1 && rowspan > r - row {
                    return true;
                }
            }
            row -= 1;
        }
        false
    }

    /// The items the grid owns, in its order: the rows', the cells', the captions'.
    pub(crate) fn owned(&self, out: &mut Vec<RenderId>) {
        out.extend(self.rows.iter().filter_map(|r| r.el_row));
        for row in &self.cells {
            out.extend(row.iter().filter_map(|c| c.el));
        }
        out.extend(self.captions.iter().copied());
    }

    /// `cell(t_col, t_row)` to change: the cell, none outside the grid.
    pub fn cell_mut(&mut self, t_col: i32, t_row: i32) -> Option<&mut TableCell> {
        if t_col >= 0 && t_col < self.cols_count && t_row >= 0 && t_row < self.rows_count {
            return self.cells.get_mut(t_row as usize).and_then(|row| row.get_mut(t_col as usize));
        }
        None
    }

    /// `distribute_max_width(width, start, end)`.
    pub fn distribute_max_width(&mut self, width: Px, start: i32, end: i32) {
        self.distribute_width_of(width, start, end, |col| &mut col.max_width);
    }

    /// `distribute_min_width(width, start, end)`.
    pub fn distribute_min_width(&mut self, width: Px, start: i32, end: i32) {
        self.distribute_width_of(width, start, end, |col| &mut col.min_width);
    }

    /// `distribute_width(width, start, end, acc)`: `width` handed to the columns `start` to `end` (the field `acc`
    /// names) in proportion to their maximum widths, evenly when those are all 0, what rounding leaves to the first.
    fn distribute_width_of(&mut self, width: Px, start: i32, end: i32, acc: fn(&mut TableColumn) -> &mut Px) {
        if start < 0 || start >= self.cols_count || end < 0 || end >= self.cols_count {
            return;
        }

        let mut cols_width = Px(0.0);
        for col in start..=end {
            cols_width += self.columns[col as usize].max_width;
        }

        let mut add = width / Px::from_int(end - start + 1);
        let mut added_width = Px(0.0);
        for col in start..=end {
            if cols_width != Px(0.0) {
                add = width * (self.columns[col as usize].max_width / cols_width);
            }
            added_width += add;
            *acc(&mut self.columns[col as usize]) += add;
        }
        if added_width < width {
            *acc(&mut self.columns[start as usize]) += width - added_width;
        }
    }

    /// `calc_table_width(block_width, is_auto, min_table_width, max_table_width)`: the columns' widths, in floats:
    /// each column's minimum first, then the room left to the % columns (cut from the last when they pass 100%),
    /// the fixed ones up to their width, the auto ones by their maximum widths (evenly when none has one), the
    /// fixed ones again and the % ones by their shares, then evenly from the last; a lack of room taken back from
    /// the auto, the fixed, then the % columns past their minimums, from the last. Each width rounded up to 1/64 px;
    /// their sum. The minimum and maximum widths of the table come back too, the maximum from what the % columns
    /// need.
    pub fn calc_table_width(&mut self, block_width: Px, is_auto: bool, min_table_width: &mut Px, max_table_width: &mut Px) -> Px {
        #[derive(Clone, Copy, PartialEq)]
        enum Kind {
            Auto,
            Fixed,
            Percent,
        }
        let n = self.cols_count.max(0) as usize;
        let mut kinds = vec![Kind::Auto; n];
        let mut fixed = vec![0.0f32; n];
        let mut percent = vec![0.0f32; n];
        let mut mins = vec![0.0f32; n];
        let mut maxs = vec![0.0f32; n];
        let mut widths = vec![0.0f32; n];
        let (mut min_sum, mut max_sum, mut max_percent, mut max_non_percent, mut remaining_percent) =
            (0.0f32, 0.0f32, 0.0f32, 0.0f32, 100.0f32);
        let epsilon = 1.0f32 / 128.0f32;
        for col in 0..n {
            let column = &self.columns[col];
            mins[col] = column.min_width.value();
            maxs[col] = max_f32(column.max_width.value(), mins[col]);
            if !column.css_width.is_predefined() {
                if column.css_width.units() == CssUnits::css_units_percentage {
                    kinds[col] = Kind::Percent;
                    percent[col] = max_f32(0.0, column.css_width.val());
                } else {
                    kinds[col] = Kind::Fixed;
                    fixed[col] = maxs[col];
                }
            }
            min_sum += mins[col];
            max_sum += maxs[col];
            if kinds[col] == Kind::Percent {
                let p = min_f32(percent[col], remaining_percent);
                max_percent = max_f32(max_percent, maxs[col] * 100.0 / max_f32(p, epsilon));
                remaining_percent -= p;
            } else {
                max_non_percent += maxs[col];
            }
        }
        max_non_percent = max_non_percent * 100.0 / max_f32(remaining_percent, epsilon);
        let mut max_width = max_list_f32(&[max_sum, max_non_percent, max_percent]);
        if remaining_percent >= 100.0 {
            max_width = max_sum;
        }
        *min_table_width = Px(min_sum);
        *max_table_width = Px(max_width);

        let mut table = block_width.value();
        if is_auto {
            table = min_f32(table, max_width);
        }
        table = max_f32(table, min_sum);

        let mut available = table;
        let mut have_percent = false;
        let (mut num_auto, mut num_fixed) = (0i32, 0i32);
        let (mut total_auto, mut total_fixed, mut total_percent, mut alloc_auto) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        for col in 0..n {
            widths[col] = mins[col];
            available -= mins[col];
            match kinds[col] {
                Kind::Percent => {
                    have_percent = true;
                    total_percent += percent[col];
                }
                Kind::Fixed => {
                    num_fixed += 1;
                    total_fixed += maxs[col];
                }
                Kind::Auto => {
                    num_auto += 1;
                    total_auto += maxs[col];
                    alloc_auto += mins[col];
                }
            }
        }
        if available > 0.0 && have_percent {
            for col in 0..n {
                if kinds[col] == Kind::Percent {
                    let w = max_f32(mins[col], table * percent[col] / 100.0);
                    available += widths[col] - w;
                    widths[col] = w;
                }
            }
            if total_percent > 100.0 {
                let mut excess = table * (total_percent - 100.0) / 100.0;
                for col in (0..n).rev() {
                    if kinds[col] == Kind::Percent {
                        let w = widths[col];
                        let reduction = min_f32(w, excess);
                        excess -= reduction;
                        let next = max_f32(mins[col], w - reduction);
                        available += w - next;
                        widths[col] = next;
                    }
                }
            }
        }
        if available > 0.0 {
            for col in 0..n {
                if kinds[col] == Kind::Fixed && fixed[col] > widths[col] {
                    available += widths[col] - fixed[col];
                    widths[col] = fixed[col];
                }
            }
        }
        if available > 0.0 && num_auto != 0 {
            available += alloc_auto;
            let mut autos_left = num_auto;
            for col in 0..n {
                if kinds[col] == Kind::Auto {
                    // Auto columns with no content to weigh them by share evenly.
                    let share = if total_auto > 0.0 { available * maxs[col] / total_auto } else { available / autos_left as f32 };
                    let w = max_f32(widths[col], share);
                    available -= w;
                    total_auto -= maxs[col];
                    autos_left -= 1;
                    widths[col] = w;
                }
            }
        }
        if available > 0.0 && num_fixed != 0 && total_fixed > 0.0 {
            for col in 0..n {
                if kinds[col] == Kind::Fixed {
                    let w = available * maxs[col] / total_fixed;
                    available -= w;
                    total_fixed -= maxs[col];
                    widths[col] += w;
                    if available <= 0.0 || total_fixed <= 0.0 {
                        break;
                    }
                }
            }
        }
        if available > 0.0 && have_percent && total_percent > 0.0 && total_percent < 100.0 {
            for col in 0..n {
                if kinds[col] == Kind::Percent {
                    let w = available * percent[col] / total_percent;
                    available -= w;
                    total_percent -= percent[col];
                    widths[col] += w;
                    if available <= 0.0 || total_percent <= 0.0 {
                        break;
                    }
                }
            }
        }
        if available > 0.0 && n > 0 {
            let mut total = n as i32;
            for col in (0..n).rev() {
                let w = available / total as f32;
                available -= w;
                total -= 1;
                widths[col] += w;
            }
        }
        if available < 0.0 {
            for k in [Kind::Auto, Kind::Fixed, Kind::Percent] {
                let mut beyond = 0.0f32;
                for col in (0..n).rev() {
                    if kinds[col] == k {
                        beyond += widths[col] - mins[col];
                    }
                }
                let mut col = n;
                while col > 0 && available < 0.0 && beyond > 0.0 {
                    col -= 1;
                    if kinds[col] == k {
                        let diff = widths[col] - mins[col];
                        let reduce = available * diff / beyond;
                        widths[col] += reduce;
                        available -= reduce;
                        beyond -= diff;
                    }
                }
                if available >= 0.0 {
                    break;
                }
            }
        }
        let mut cur_width = Px(0.0);
        for col in 0..n {
            self.columns[col].width = Px((widths[col] * 64.0 - 0.01).ceil() / 64.0);
            cur_width += self.columns[col].width;
        }
        cur_width
    }

    /// `calc_fixed_table_width(block_width)`: the columns' widths in a `table-layout: fixed` table, from their CSS
    /// widths alone: % columns of the room, fixed ones at their width, the auto ones sharing what is left evenly
    /// (the last auto one taking what rounding leaves); with no auto column, or past the room, the fixed columns
    /// scaled up to the room and the % ones sharing what the fixed leave; room still left spread evenly from the
    /// last. Each width at least 0, rounded up to 1/64 px; their sum.
    pub fn calc_fixed_table_width(&mut self, block_width: Px) -> Px {
        let n = self.cols_count.max(0) as usize;
        let mut widths = vec![-1.0f32; n];
        let mut percent = vec![0.0f32; n];
        let (mut total_fixed, mut total_percent, mut total_percent_width) = (0.0f32, 0.0f32, 0.0f32);
        let mut auto_span = 0i32;
        for col in 0..n {
            let w = &self.columns[col].css_width;
            if !w.is_predefined() && w.units() == CssUnits::css_units_percentage {
                percent[col] = max_f32(0.0, w.val());
                widths[col] = block_width.value() * percent[col] / 100.0;
                total_percent += percent[col];
                total_percent_width += widths[col];
            } else if !w.is_predefined() {
                widths[col] = self.columns[col].max_width.value();
                total_fixed += widths[col];
            } else {
                auto_span += 1;
            }
        }
        let table = max_f32(block_width.value(), total_fixed);
        let mut total = total_fixed + total_percent_width;
        if auto_span == 0 || total > table {
            if total != table {
                if total_fixed > 0.0 && total < table {
                    let scaled = total;
                    total_fixed = 0.0;
                    for col in 0..n {
                        if widths[col] >= 0.0 && percent[col] == 0.0 {
                            widths[col] = widths[col] * table / scaled;
                            total_fixed += widths[col];
                        }
                    }
                }
                if total_percent > 0.0 {
                    total_percent_width = 0.0;
                    for col in 0..n {
                        if percent[col] > 0.0 {
                            widths[col] = percent[col] * (table - total_fixed) / total_percent;
                            total_percent_width += widths[col];
                        }
                    }
                }
                total = total_fixed + total_percent_width;
            }
        } else {
            let mut remaining = table - total_fixed - total_percent_width;
            let mut last_auto = 0usize;
            for col in 0..n {
                if widths[col] < 0.0 {
                    let w = remaining / auto_span as f32;
                    widths[col] = w;
                    remaining -= w;
                    auto_span -= 1;
                    last_auto = col;
                }
            }
            if remaining != 0.0 {
                widths[last_auto] += remaining;
            }
            total = table;
        }
        if total < table && n > 0 {
            let mut remaining = table - total;
            let mut count = n;
            while count != 0 {
                let w = remaining / count as f32;
                remaining -= w;
                count -= 1;
                widths[count] += w;
            }
            widths[n - 1] += remaining;
        }
        let mut cur_width = Px(0.0);
        for col in 0..n {
            self.columns[col].width = Px((max_f32(0.0, widths[col]) * 64.0 - 0.01).ceil() / 64.0);
            cur_width += self.columns[col].width;
        }
        cur_width
    }

    /// `calc_horizontal_positions(table_borders, bc, bdr_space_x)`: each column's left and right, after the spacing
    /// when the borders are separate, else overlapping its neighbours and the table by the thinner border.
    pub fn calc_horizontal_positions(&mut self, table_borders: &Margins, bc: i32, bdr_space_x: Px) {
        let n = self.cols_count.max(0) as usize;
        if bc == BorderCollapse::border_collapse_separate.index() {
            let mut left = bdr_space_x;
            for column in self.columns.iter_mut().take(n) {
                column.left = left;
                column.right = column.left + column.width;
                left = column.right + bdr_space_x;
            }
        } else {
            let mut left = Px(0.0);
            if n != 0 {
                left -= table_borders.left.min(self.columns[0].border_left);
            }
            for i in 0..n {
                if i > 0 {
                    left -= self.columns[i - 1].border_right.min(self.columns[i].border_left);
                }

                self.columns[i].left = left;
                self.columns[i].right = self.columns[i].left + self.columns[i].width;
                left = self.columns[i].right;
            }
        }
    }

    /// `calc_vertical_positions(table_borders, bc, bdr_space_y)`: each row's top and bottom, as the columns'.
    pub fn calc_vertical_positions(&mut self, table_borders: &Margins, bc: i32, bdr_space_y: Px) {
        let n = self.rows_count.max(0) as usize;
        if bc == BorderCollapse::border_collapse_separate.index() {
            let mut top = bdr_space_y;
            for row in self.rows.iter_mut().take(n) {
                row.top = top;
                row.bottom = row.top + row.height;
                top = row.bottom + bdr_space_y;
            }
        } else {
            let mut top = Px(0.0);
            if n != 0 {
                top -= table_borders.top.min(self.rows[0].border_top);
            }
            for i in 0..n {
                if i > 0 {
                    top -= self.rows[i - 1].border_bottom.min(self.rows[i].border_top);
                }

                self.rows[i].top = top;
                self.rows[i].bottom = self.rows[i].top + self.rows[i].height;
                top = self.rows[i].bottom;
            }
        }
    }

    /// `calc_rows_height(blockHeight, borderSpacingY)`: each row at least its CSS height (a % one's against the
    /// table's), every row's minimum the height it then has; a taller table's extra height given to the % rows,
    /// then shared by the auto rows (or by all when there is none), or what the % rows took past it taken back from
    /// the last rows down to their minimums.
    pub fn calc_rows_height(&mut self, block_height: Px, _border_spacing_y: Px) {
        let mut min_table_height = Px(0.0);

        // compute vertical size inferred by cells
        for row in &mut self.rows {
            if !row.css_height.is_predefined()
                && row.css_height.units() != CssUnits::css_units_percentage
                && row.height < Px(row.css_height.val())
            {
                row.height = Px(row.css_height.val());
            }
            row.min_height = row.height;
            min_table_height += row.height;
        }

        if block_height > min_table_height {
            let mut extra_height = block_height - min_table_height;
            let mut auto_count = 0i32; // number of rows with height=auto
            for row in &mut self.rows {
                if !row.css_height.is_predefined() && row.css_height.units() == CssUnits::css_units_percentage {
                    row.height = row.css_height.calc_percent(block_height);
                    if row.height < row.min_height {
                        row.height = row.min_height;
                    }

                    extra_height -= row.height - row.min_height;

                    if extra_height <= Px(0.0) {
                        break;
                    }
                } else if row.css_height.is_predefined() {
                    auto_count += 1;
                }
            }
            if extra_height > Px(0.0) {
                if auto_count != 0 {
                    // distribute height to the rows with height=auto
                    let extra_row_height = extra_height / Px::from_int(auto_count);
                    for row in &mut self.rows {
                        if row.css_height.is_predefined() {
                            row.height += extra_row_height;
                        }
                    }
                } else if !self.rows.is_empty() {
                    // We don't have rows with height=auto, so distribute height to all rows
                    let extra_row_height = extra_height / Px::from_int(self.rows.len() as i32);
                    for row in &mut self.rows {
                        row.height += extra_row_height;
                    }
                }
            } else if extra_height < Px(0.0) {
                extra_height = -extra_height;
                let mut rows = self.rows.iter_mut().rev();
                while extra_height > Px(0.0) {
                    let Some(row) = rows.next() else { break };
                    if row.height > row.min_height {
                        if row.height - extra_height >= row.min_height {
                            row.height -= extra_height;
                            extra_height = Px(0.0);
                        } else {
                            extra_height -= row.height - row.min_height;
                            row.height = row.min_height;
                        }
                    }
                }
            }
        }
    }
}

impl Document {
    /// `begin_row(row)`.
    pub(crate) fn grid_begin_row(&mut self, grid: &mut TableGrid, row: RenderId) {
        grid.cells.push(Vec::new());
        self.render.hold(row);
        let css_height = self.el(self.render.item(row).element).css.m_css_height.clone();
        grid.rows.push(TableRow { el_row: Some(row), css_height, ..TableRow::default() });
    }

    /// `add_cell(el)`: the cell after the places spans from above take, its `colspan` and `rowspan` (1 to 1000)
    /// taken of the document's budget (or 1 and 1 when they do not fit), and a place for each further column it spans.
    pub(crate) fn grid_add_cell(&mut self, cx: &StyleCx, grid: &mut TableGrid, el: RenderId) {
        let element = self.render.item(el).element;
        let span = |name: &[u8]| twidget_cstd::atoi(crate::gumbo::c_str(self.get_attr(element, name).unwrap_or(b"1")));
        let mut cell = TableCell { el: Some(el), colspan: span(b"colspan"), rowspan: span(b"rowspan"), ..TableCell::default() };
        self.render.hold(el);
        cell.colspan = cell.colspan.clamp(1, 1000);
        cell.rowspan = cell.rowspan.clamp(1, 1000);
        if !self.take_table_cells(cx, cell.colspan * cell.rowspan) {
            cell.colspan = 1;
            cell.rowspan = 1;
            self.take_table_cells(cx, 1);
        }
        cell.borders = self.render.item(el).borders;

        if grid.cells.is_empty() {
            // The C++ reads past an empty grid here; every cell comes after its row.
            grid.cells.push(Vec::new());
        }
        let r = grid.cells.len() as i32 - 1;
        while grid.is_rowspanned(r, grid.cells[r as usize].len() as i32) {
            grid.cells[r as usize].push(TableCell::default());
        }

        let colspan = cell.colspan;
        let row = &mut grid.cells[r as usize];
        row.push(cell);
        for _ in 1..colspan {
            row.push(TableCell::default());
        }
    }

    /// `finish()`: the grid's size, its columns limited by the budget, every row filled out to them; each column's
    /// border widths the thinnest of its cells' (the first non-zero kept), each row's too, and a column's width the
    /// first width a cell of one column declares, which every such cell's computed width then becomes.
    pub(crate) fn grid_finish(&mut self, cx: &StyleCx, grid: &mut TableGrid) {
        grid.rows_count = grid.cells.len() as i32;
        grid.cols_count = 0;
        for cell in &grid.cells {
            grid.cols_count = grid.cols_count.max(cell.len() as i32);
        }
        self.grid_limit_columns(cx, grid);
        let cols = grid.cols_count as usize;
        for cell in &mut grid.cells {
            while cell.len() < cols {
                cell.push(TableCell::default());
            }
        }

        grid.columns.clear();
        for _ in 0..grid.cols_count {
            grid.columns.push(TableColumn::default());
        }

        for col in 0..grid.cols_count {
            for row in 0..grid.rows_count {
                let Some(cell) = grid.cell(col, row) else { continue };
                let (el, borders, colspan) = (cell.el, cell.borders, cell.colspan);
                if el.is_some() {
                    let column = &mut grid.columns[col as usize];
                    // find minimum left border width
                    column.border_left = if column.border_left != Px(0.0) { column.border_left.min(borders.left) } else { borders.left };
                    // find minimum right border width
                    column.border_right =
                        if column.border_right != Px(0.0) { column.border_right.min(borders.right) } else { borders.right };
                    let r = &mut grid.rows[row as usize];
                    // find minimum top border width
                    r.border_top = if r.border_top != Px(0.0) { r.border_top.min(borders.top) } else { borders.top };
                    // find minimum bottom border width
                    r.border_bottom = if r.border_bottom != Px(0.0) { r.border_bottom.min(borders.bottom) } else { borders.bottom };
                }

                if let Some(el) = el
                    && colspan <= 1
                {
                    let cell_width = &self.el(self.render.item(el).element).css.m_cell_width;
                    if !cell_width.is_predefined() && grid.columns[col as usize].css_width.is_predefined() {
                        grid.columns[col as usize].css_width = cell_width.clone();
                    }
                }
            }
        }

        for col in 0..grid.cols_count {
            for row in 0..grid.rows_count {
                if let Some(cell) = grid.cell(col, row)
                    && let Some(el) = cell.el
                    && cell.colspan == 1
                {
                    let element = self.render.item(el).element;
                    self.el_mut(element).css.m_css_width = grid.columns[col as usize].css_width.clone();
                }
            }
        }
    }

    /// `limit_columns()`: as many columns, by halving steps from the largest power of two, as the cells that fill
    /// the rows out to them can be taken of the document's budget; the rows cut to them and their spans with them.
    fn grid_limit_columns(&mut self, cx: &StyleCx, grid: &mut TableGrid) {
        if grid.cols_count < 2 || grid.rows.first().is_none_or(|row| row.el_row.is_none()) {
            return;
        }
        // Padding every row to the widest one makes cells too: they come out of the same budget.
        let padding = |cells: &[Vec<TableCell>], cols: i32| -> i64 {
            let mut count = 0i64;
            for row in cells {
                count += i64::from(0.max(cols - row.len() as i32));
            }
            count
        };
        let mut kept = 1;
        let mut charged = padding(&grid.cells, 1);
        let mut step = 1;
        while step <= grid.cols_count / 2 {
            step *= 2;
        }
        while step > 0 {
            if kept + step <= grid.cols_count {
                let count = padding(&grid.cells, kept + step) - charged;
                if count == 0 || (count < (1 << 30) && self.take_table_cells(cx, count as i32)) {
                    kept += step;
                    charged += count;
                }
            }
            step /= 2;
        }
        if kept == grid.cols_count {
            return;
        }
        grid.cols_count = kept;
        for row in &mut grid.cells {
            let mut cut = Vec::new();
            while row.len() as i32 > kept {
                if let Some(cell) = row.pop() {
                    cut.extend(cell.el);
                }
            }
            for col in 0..row.len() as i32 {
                row[col as usize].colspan = row[col as usize].colspan.min(kept - col);
            }
            for el in cut {
                self.release_render(el);
            }
        }
    }

    /// `document::take_table_cells(count)`: the cells taken of the document's 200000, or none and the container told
    /// once that the table limit was reached.
    pub(crate) fn take_table_cells(&mut self, cx: &StyleCx, count: i32) -> bool {
        if self.render.table_cells + count > crate::document::max_table_cells {
            if !self.render.table_limited {
                self.render.table_limited = true;
                cx.container.sanitizer_limit(b"table");
            }
            return false;
        }
        self.render.table_cells += count;
        true
    }
}
