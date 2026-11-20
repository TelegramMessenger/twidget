//! `render_table.h` and `render_table.cpp` as far as the render tree goes, with `iterators.cpp`'s walks over a
//! table: `render_item_table::init()` builds the grid from the rows inside the table and its row groups and the
//! cells of each row (a row group met inside a row taken as a cell too, and its own cells after it; each one made
//! what its `init()` makes it), then the captions, and reads the border spacing. `render_item_table_part` and
//! `render_item_table_row` have the base class's `init()`. Laying the table out is a later stage's.

use crate::Px;
use crate::document::{Document, StyleCx};
use crate::render_item::*;
use crate::table::TableGrid;

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
