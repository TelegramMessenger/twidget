//! `table.h` and `table.cpp`'s structure: a table's grid of rows, cells and columns as `render_item_table::init()`
//! builds it (`begin_row`, `add_cell` with spans and the document's cell budget, `is_rowspanned`, `finish` with
//! `limit_columns`, the columns' border widths and widths, which `finish` writes into its cells' computed widths).
//! The widths' distribution and the rows' and columns' placement are layout's, a later stage's.

use crate::Px;
use crate::css_length::CssLength;
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
