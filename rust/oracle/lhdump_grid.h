// lhdump's grid probe (stage L11): render_grid.cpp keeps nothing of a layout pass, so the copy lhdump compiles calls record() as _render_content returns, and the Y dump writes what it kept.
#pragma once

#include <cstdint>
#include <vector>

namespace litehtml
{
    class render_item;
}

namespace lhdump_grid
{
    // A grid_track: its size's types and fr, then what sizing left.
    struct Track
    {
        int     min_type = 0;
        int     max_type = 0;
        float   fr = 0;
        float   base = 0;
        float   limit = 0;
        bool    infinite = false;
        bool    repeated = false;
        bool    occupied = false;
        bool    collapsed = false;
        float   content = 0;
        float   min_content = 0;
    };

    // A grid_cell: its item, area and measures.
    struct Cell
    {
        const litehtml::render_item* el = nullptr;
        int     col = 0;
        int     row = 0;
        int     col_span = 0;
        int     row_span = 0;
        float   min_content = 0;
        float   max_content = 0;
        bool    auto_min = false;
        float   width = 0;
        float   height = 0;
    };

    // One _render_content of a grid as it returns.
    struct Pass
    {
        const litehtml::render_item* grid = nullptr;
        float   available = 0;
        float   column_gap = 0;
        bool    definite_height = false;
        float   container_height = 0;
        float   row_gap = 0;
        int     explicit_columns = 0;
        int     explicit_rows = 0;
        std::vector<Track> columns;
        std::vector<Track> rows;
        std::vector<Cell> cells;
        std::vector<float> column_x;
        std::vector<float> row_y;
        float   row_start = 0;
        float   row_between = 0;
        float   natural_width = 0;
        float   min_width = 0;
    };

    // The passes since lhdump last cleared them: at a case's start and after every step but a layout.
    inline std::vector<Pass>& trace()
    {
        static std::vector<Pass> passes;
        return passes;
    }

    template <typename T> Track track(const T& t)
    {
        Track out;
        out.min_type = int(t.size.min_type);
        out.max_type = int(t.size.max_type);
        out.fr = t.size.fr;
        out.base = t.base.value();
        out.limit = t.limit.value();
        out.infinite = t.infinite;
        out.repeated = t.repeated;
        out.occupied = t.occupied;
        out.collapsed = t.collapsed;
        out.content = t.content.value();
        out.min_content = t.min_content.value();
        return out;
    }

    template <typename Tracks, typename Cells, typename Pixels, typename Px, typename Ret>
    void record(const litehtml::render_item* grid, Px available, Px column_gap, bool definite_height, Px container_height, Px row_gap, int explicit_columns, int explicit_rows,
                const Tracks& columns, const Tracks& rows, const Cells& cells, const Pixels& column_x, const Pixels& row_y, Px row_start, Px row_between, const Ret& ret)
    {
        Pass pass;
        pass.grid = grid;
        pass.available = available.value();
        pass.column_gap = column_gap.value();
        pass.definite_height = definite_height;
        pass.container_height = container_height.value();
        pass.row_gap = row_gap.value();
        pass.explicit_columns = explicit_columns;
        pass.explicit_rows = explicit_rows;
        for (const auto& t : columns) pass.columns.push_back(track(t));
        for (const auto& t : rows) pass.rows.push_back(track(t));
        for (const auto& c : cells) {
            Cell cell;
            cell.el = c.el.get();
            cell.col = c.col;
            cell.row = c.row;
            cell.col_span = c.col_span;
            cell.row_span = c.row_span;
            cell.min_content = c.min_content.value();
            cell.max_content = c.max_content.value();
            cell.auto_min = c.auto_min;
            cell.width = c.width.value();
            cell.height = c.height.value();
            pass.cells.push_back(cell);
        }
        for (const auto& x : column_x) pass.column_x.push_back(x.value());
        for (const auto& y : row_y) pass.row_y.push_back(y.value());
        pass.row_start = row_start.value();
        pass.row_between = row_between.value();
        pass.natural_width = ret.natural_width.value();
        pass.min_width = ret.min_width.value();
        trace().push_back(std::move(pass));
    }
} // namespace lhdump_grid
