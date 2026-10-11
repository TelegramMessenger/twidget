#ifndef LITEHTML_CSS_EFFECTS_H
#define LITEHTML_CSS_EFFECTS_H

#include <vector>
#include "css_length.h"
#include "types.h"
#include "web_color.h"

namespace litehtml
{
    class document_container;

    struct box_shadow
    {
        css_length x;
        css_length y;
        css_length blur;
        css_length spread;
        web_color  color = web_color::current_color;
        bool       inset = false;
    };
    using box_shadow_vector = std::vector<box_shadow>;

    struct transform_function
    {
        enum kind_t
        {
            translate,
            scale,
            rotate,
            skew,
            matrix,
            rotate_x,
            rotate_y,
        };
        kind_t     kind = translate;
        css_length x;
        css_length y;
        float      v[6] = {0, 0, 0, 0, 0, 0};
    };
    using transform_vector = std::vector<transform_function>;

    struct transform_matrix
    {
        float a  = 1;
        float b  = 0;
        float c  = 0;
        float d  = 1;
        float tx = 0;
        float ty = 0;

        bool is_identity() const { return a == 1 && b == 0 && c == 0 && d == 1 && tx == 0 && ty == 0; }
        transform_matrix then(const transform_matrix& m) const
        {
            return {a * m.a + b * m.c, a * m.b + b * m.d, c * m.a + d * m.c, c * m.b + d * m.d,
                    tx * m.a + ty * m.c + m.tx, tx * m.b + ty * m.d + m.ty};
        }
    };

    struct shadow_paint
    {
        pixel_t   x;
        pixel_t   y;
        pixel_t   blur;
        pixel_t   spread;
        web_color color;
        bool      inset = false;
    };

    struct paint_group
    {
        float            opacity = 1;
        transform_matrix matrix;
        bool             backface_hidden = false;
        position         bounds;
    };

    struct svg_gradient
    {
        int                                    type = 0;
        float                                  x1 = 0, y1 = 0, x2 = 1, y2 = 0, r = 0.5f, fx = 0.5f, fy = 0.5f;
        std::vector<std::pair<float, web_color>> stops;
    };

    struct vector_path
    {
        enum op_t
        {
            move,
            line,
            quad,
            cubic,
            close,
        };
        std::vector<op_t>  ops;
        std::vector<float> points;
        bool               fill = false;
        web_color          fill_color;
        bool               evenodd = false;
        bool               stroke  = false;
        web_color          stroke_color;
        float              stroke_width = 1;
        int                line_cap     = 0;
        int                line_join    = 0;
        float              miter_limit  = 4;
        std::vector<float> dashes;
        float              dash_offset = 0;
        svg_gradient       fill_gradient;
    };

    struct grid_track_size
    {
        enum type_t
        {
            length,
            fraction,
            automatic,
            min_content,
            max_content,
        };
        type_t     min_type = automatic;
        type_t     max_type = automatic;
        css_length min_len;
        css_length max_len;
        float      fr = 0;
    };

    struct grid_track_list
    {
        std::vector<grid_track_size> tracks;
        std::vector<grid_track_size> repeat;
        int                          repeat_at = -1;
        bool                         auto_fit  = false;

        bool empty() const { return tracks.empty() && repeat.empty(); }
    };

    struct grid_placement
    {
        int start = 0;
        int end   = 0;
        int span  = 0;
    };

    bool parse_box_shadow(const css_token_vector& tokens, box_shadow_vector& shadows, document_container* container);
    bool parse_transform(const css_token_vector& tokens, transform_vector& functions);
    bool parse_transform_origin(const css_token_vector& tokens, css_length& x, css_length& y);
    bool parse_grid_track_list(const css_token_vector& tokens, grid_track_list& list);
    bool parse_grid_placement(const css_token_vector& tokens, grid_placement& placement);
} // namespace litehtml

#endif // LITEHTML_CSS_EFFECTS_H
