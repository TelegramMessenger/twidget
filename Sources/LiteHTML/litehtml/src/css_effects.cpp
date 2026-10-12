#include <cstring>
#include <cmath>
#include "html.h"
#include "css_effects.h"
#include "style.h"
#include "web_color.h"

namespace litehtml
{
    static bool parse_shadow_length(const css_token& tok, css_length& len)
    {
        return len.from_token(tok, f_length);
    }

    bool parse_box_shadow(const css_token_vector& tokens, box_shadow_vector& shadows, document_container* container)
    {
        shadows.clear();
        if(tokens.size() == 1 && tokens[0].ident() == "none")
        {
            return true;
        }
        box_shadow shadow;
        int        lengths   = 0;
        bool       has_color = false;
        auto       flush     = [&]() -> bool {
            if(lengths < 2)
            {
                return false;
            }
            shadows.push_back(shadow);
            shadow    = box_shadow();
            lengths   = 0;
            has_color = false;
            return true;
        };
        for(const auto& tok : tokens)
        {
            if(tok.ch == ',')
            {
                if(!flush())
                {
                    shadows.clear();
                    return false;
                }
                continue;
            }
            if(tok.ident() == "inset")
            {
                shadow.inset = true;
                continue;
            }
            css_length len;
            if(lengths < 4 && parse_shadow_length(tok, len))
            {
                css_length* slots[4] = {&shadow.x, &shadow.y, &shadow.blur, &shadow.spread};
                *slots[lengths++]    = len;
                continue;
            }
            web_color color;
            if(!has_color && parse_color(tok, color, container))
            {
                shadow.color = color;
                has_color    = true;
                continue;
            }
            shadows.clear();
            return false;
        }
        if(!flush())
        {
            shadows.clear();
            return false;
        }
        return true;
    }

    static bool function_args(const css_token& tok, std::vector<css_token>& args)
    {
        args.clear();
        for(const auto& arg : tok.value)
        {
            if(arg.ch != ',')
            {
                args.push_back(arg);
            }
        }
        return !args.empty();
    }

    static bool parse_number(const css_token& tok, float& value)
    {
        if(tok.type == NUMBER)
        {
            value = tok.n.number;
            return true;
        }
        if(tok.type == PERCENTAGE)
        {
            value = tok.n.number / 100;
            return true;
        }
        return false;
    }

    static bool parse_radians(const css_token& tok, float& value)
    {
        float degrees = 0;
        if(!parse_angle(tok, degrees))
        {
            return false;
        }
        value = degrees * 3.14159265f / 180;
        return true;
    }

    struct calc_value
    {
        double      number = 0;
        std::string unit;
    };

    static bool calc_sum(const css_token_vector& tokens, size_t& index, calc_value& out, int depth);

    static bool calc_atom(const css_token_vector& tokens, size_t& index, calc_value& out, int depth)
    {
        while(index < tokens.size() && tokens[index].type == WHITESPACE)
            index++;
        if(index >= tokens.size() || depth > 16)
            return false;
        const css_token& tok = tokens[index++];
        if(tok.type == NUMBER)
        {
            out = {tok.n.number, ""};
            return true;
        }
        if(tok.type == PERCENTAGE)
        {
            out = {tok.n.number, "%"};
            return true;
        }
        if(tok.type == DIMENSION)
        {
            out = {tok.n.number, lowcase(tok.unit())};
            return true;
        }
        if(tok.type == ROUND_BLOCK || (tok.type == CV_FUNCTION && lowcase(tok.name()) == "calc"))
        {
            size_t inner = 0;
            if(!calc_sum(tok.value, inner, out, depth + 1))
                return false;
            while(inner < tok.value.size() && tok.value[inner].type == WHITESPACE)
                inner++;
            return inner == tok.value.size();
        }
        return false;
    }

    static int calc_operator(const css_token_vector& tokens, size_t& index, const char* allowed)
    {
        size_t probe = index;
        while(probe < tokens.size() && tokens[probe].type == WHITESPACE)
            probe++;
        if(probe < tokens.size() && tokens[probe].ch > 0 && tokens[probe].ch < 128 && strchr(allowed, tokens[probe].ch))
        {
            index = probe + 1;
            return tokens[probe].ch;
        }
        return 0;
    }

    static bool calc_product(const css_token_vector& tokens, size_t& index, calc_value& out, int depth)
    {
        if(!calc_atom(tokens, index, out, depth))
            return false;
        while(int op = calc_operator(tokens, index, "*/"))
        {
            calc_value right;
            if(!calc_atom(tokens, index, right, depth))
                return false;
            if(op == '*')
            {
                if(!out.unit.empty() && !right.unit.empty())
                    return false;
                out = {out.number * right.number, out.unit.empty() ? right.unit : out.unit};
            } else
            {
                if(!right.unit.empty() || right.number == 0)
                    return false;
                out.number /= right.number;
            }
        }
        return std::isfinite(out.number);
    }

    static bool calc_sum(const css_token_vector& tokens, size_t& index, calc_value& out, int depth)
    {
        if(!calc_product(tokens, index, out, depth))
            return false;
        while(int op = calc_operator(tokens, index, "+-"))
        {
            calc_value right;
            if(!calc_product(tokens, index, right, depth))
                return false;
            if(out.unit != right.unit && out.number != 0 && right.number != 0)
                return false;
            std::string unit = out.unit.empty() ? right.unit : out.unit;
            out = {op == '+' ? out.number + right.number : out.number - right.number, unit};
        }
        return std::isfinite(out.number);
    }

    static bool fold_calc(css_token& tok)
    {
        if(tok.type != CV_FUNCTION || lowcase(tok.name()) != "calc")
            return true;
        calc_value value;
        size_t     index = 0;
        if(!calc_sum(tok.value, index, value, 0))
            return false;
        while(index < tok.value.size() && tok.value[index].type == WHITESPACE)
            index++;
        if(index != tok.value.size())
            return false;
        if(value.unit.empty())
            tok = css_token(NUMBER, float(value.number), css_number_number);
        else if(value.unit == "%")
            tok = css_token(PERCENTAGE, float(value.number), css_number_number);
        else
            tok = css_token(DIMENSION, float(value.number), css_number_number, value.unit);
        return true;
    }

    bool parse_transform(const css_token_vector& tokens, transform_vector& functions)
    {
        functions.clear();
        if(tokens.size() == 1 && tokens[0].ident() == "none")
        {
            return true;
        }
        std::vector<css_token> args;
        for(const auto& tok : tokens)
        {
            if(tok.type != CV_FUNCTION || !function_args(tok, args))
            {
                functions.clear();
                return false;
            }
            for(auto& arg : args)
            {
                if(!fold_calc(arg))
                {
                    functions.clear();
                    return false;
                }
            }
            std::string        name = lowcase(tok.name());
            transform_function fn;
            bool               ok = true;
            if(name == "translate" || name == "translatex" || name == "translatey" || name == "translate3d")
            {
                fn.kind = transform_function::translate;
                if(name == "translatey")
                {
                    ok = fn.y.from_token(args[0], f_length_percentage);
                } else
                {
                    ok = fn.x.from_token(args[0], f_length_percentage);
                    if(ok && args.size() > 1 && name != "translatex")
                    {
                        ok = fn.y.from_token(args[1], f_length_percentage);
                    }
                }
            } else if(name == "scale" || name == "scalex" || name == "scaley" || name == "scale3d")
            {
                fn.kind = transform_function::scale;
                float sx = 1, sy = 1;
                if(name == "scaley")
                {
                    ok = parse_number(args[0], sy);
                } else
                {
                    ok = parse_number(args[0], sx);
                    sy = name == "scalex" ? 1 : sx;
                    if(ok && args.size() > 1 && name != "scalex")
                    {
                        ok = parse_number(args[1], sy);
                    }
                }
                fn.v[0] = sx;
                fn.v[1] = sy;
            } else if(name == "rotate" || name == "rotatez")
            {
                fn.kind = transform_function::rotate;
                ok      = parse_radians(args[0], fn.v[0]);
            } else if(name == "rotatex" || name == "rotatey")
            {
                fn.kind = name == "rotatex" ? transform_function::rotate_x : transform_function::rotate_y;
                ok      = parse_radians(args[0], fn.v[0]);
            } else if(name == "rotate3d")
            {
                float x = 0, y = 0, z = 0;
                ok = args.size() == 4 && parse_number(args[0], x) && parse_number(args[1], y) &&
                     parse_number(args[2], z) && parse_radians(args[3], fn.v[0]);
                if(ok && x == 0 && y == 0)
                {
                    fn.kind = transform_function::rotate;
                    fn.v[0] = z < 0 ? -fn.v[0] : fn.v[0];
                } else if(ok && y == 0 && z == 0)
                {
                    fn.kind = transform_function::rotate_x;
                } else if(ok && x == 0 && z == 0)
                {
                    fn.kind = transform_function::rotate_y;
                } else
                {
                    continue;
                }
            } else if(name == "skew" || name == "skewx" || name == "skewy")
            {
                fn.kind = transform_function::skew;
                if(name == "skewy")
                {
                    ok = parse_radians(args[0], fn.v[1]);
                } else
                {
                    ok = parse_radians(args[0], fn.v[0]);
                    if(ok && args.size() > 1 && name == "skew")
                    {
                        ok = parse_radians(args[1], fn.v[1]);
                    }
                }
            } else if(name == "matrix")
            {
                fn.kind = transform_function::matrix;
                ok      = args.size() == 6;
                for(size_t i = 0; ok && i < 6; i++)
                {
                    ok = args[i].type == NUMBER;
                    if(ok)
                    {
                        fn.v[i] = args[i].n.number;
                    }
                }
            } else if(name == "perspective" || name == "translatez")
            {
                continue;
            } else
            {
                ok = false;
            }
            if(!ok)
            {
                functions.clear();
                return false;
            }
            functions.push_back(fn);
        }
        return true;
    }

    bool parse_transform_origin(const css_token_vector& tokens, css_length& x, css_length& y)
    {
        x = css_length(50, css_units_percentage);
        y = css_length(50, css_units_percentage);
        int  index    = 0;
        bool x_set    = false;
        bool y_set    = false;
        for(const auto& tok : tokens)
        {
            if(index >= 2)
            {
                break;
            }
            std::string ident = tok.ident();
            css_length  len;
            if(ident == "left" || ident == "right")
            {
                x     = css_length(ident == "left" ? 0.f : 100.f, css_units_percentage);
                x_set = true;
            } else if(ident == "top" || ident == "bottom")
            {
                y     = css_length(ident == "top" ? 0.f : 100.f, css_units_percentage);
                y_set = true;
            } else if(ident == "center")
            {
            } else if(len.from_token(tok, f_length_percentage))
            {
                if(index == 0 && !x_set)
                {
                    x     = len;
                    x_set = true;
                } else
                {
                    y     = len;
                    y_set = true;
                }
            } else
            {
                return false;
            }
            index++;
        }
        return true;
    }

    static bool parse_track_breadth(const css_token& tok, grid_track_size::type_t& type, css_length& len, float& fr)
    {
        std::string ident = tok.ident();
        if(ident == "auto")
        {
            type = grid_track_size::automatic;
            return true;
        }
        if(ident == "min-content")
        {
            type = grid_track_size::min_content;
            return true;
        }
        if(ident == "max-content")
        {
            type = grid_track_size::max_content;
            return true;
        }
        if(tok.type == DIMENSION && lowcase(tok.unit()) == "fr" && tok.n.number >= 0)
        {
            type = grid_track_size::fraction;
            fr   = tok.n.number;
            return true;
        }
        if(len.from_token(tok, f_length_percentage | f_positive))
        {
            type = grid_track_size::length;
            return true;
        }
        return false;
    }

    static bool parse_track_size(const css_token& tok, grid_track_size& size)
    {
        if(tok.type == CV_FUNCTION && lowcase(tok.name()) == "minmax")
        {
            std::vector<css_token> args;
            if(!function_args(tok, args) || args.size() != 2)
            {
                return false;
            }
            float fr = 0;
            if(!parse_track_breadth(args[0], size.min_type, size.min_len, fr) ||
               size.min_type == grid_track_size::fraction)
            {
                return false;
            }
            return parse_track_breadth(args[1], size.max_type, size.max_len, size.fr);
        }
        if(tok.type == CV_FUNCTION && lowcase(tok.name()) == "fit-content")
        {
            std::vector<css_token> args;
            if(!function_args(tok, args) || args.size() != 1 || !size.max_len.from_token(args[0], f_length_percentage))
            {
                return false;
            }
            size.min_type = grid_track_size::automatic;
            size.max_type = grid_track_size::length;
            return true;
        }
        if(!parse_track_breadth(tok, size.max_type, size.max_len, size.fr))
        {
            return false;
        }
        if(size.max_type == grid_track_size::fraction)
        {
            size.min_type = grid_track_size::automatic;
        } else
        {
            size.min_type = size.max_type;
            size.min_len  = size.max_len;
        }
        return true;
    }

    bool parse_grid_track_list(const css_token_vector& tokens, grid_track_list& list)
    {
        list = grid_track_list();
        if(tokens.size() == 1 && tokens[0].ident() == "none")
        {
            return true;
        }
        for(const auto& tok : tokens)
        {
            if(tok.type == SQUARE_BLOCK)
            {
                continue;
            }
            if(tok.type == CV_FUNCTION && lowcase(tok.name()) == "repeat")
            {
                std::vector<css_token> args;
                if(!function_args(tok, args) || args.size() < 2)
                {
                    return false;
                }
                std::vector<grid_track_size> sizes;
                for(size_t i = 1; i < args.size(); i++)
                {
                    if(args[i].type == SQUARE_BLOCK)
                    {
                        continue;
                    }
                    grid_track_size size;
                    if(!parse_track_size(args[i], size))
                    {
                        return false;
                    }
                    sizes.push_back(size);
                }
                if(sizes.empty())
                {
                    return false;
                }
                std::string count = args[0].ident();
                if(count == "auto-fill" || count == "auto-fit")
                {
                    if(list.repeat_at >= 0)
                    {
                        return false;
                    }
                    list.repeat_at = static_cast<int>(list.tracks.size());
                    list.auto_fit  = count == "auto-fit";
                    list.repeat    = sizes;
                } else if(args[0].type == NUMBER && args[0].n.number_type == css_number_integer && args[0].n.number > 0)
                {
                    int n = std::min(static_cast<int>(args[0].n.number), 1000);
                    for(int i = 0; i < n; i++)
                    {
                        list.tracks.insert(list.tracks.end(), sizes.begin(), sizes.end());
                    }
                } else
                {
                    return false;
                }
                continue;
            }
            grid_track_size size;
            if(!parse_track_size(tok, size))
            {
                return false;
            }
            list.tracks.push_back(size);
        }
        return true;
    }

    bool parse_grid_placement(const css_token_vector& tokens, grid_placement& placement)
    {
        placement = grid_placement();
        int  part = 0;
        bool span = false;
        for(const auto& tok : tokens)
        {
            if(tok.ch == '/')
            {
                if(++part > 1)
                {
                    return false;
                }
                span = false;
                continue;
            }
            std::string ident = tok.ident();
            if(ident == "span")
            {
                span = true;
                continue;
            }
            if(ident == "auto")
            {
                continue;
            }
            if(tok.type != NUMBER || tok.n.number_type != css_number_integer || tok.n.number == 0)
            {
                return false;
            }
            int value = std::max(-1000, std::min(1000, static_cast<int>(tok.n.number)));
            if(span)
            {
                if(value < 0)
                {
                    return false;
                }
                placement.span = value;
            } else if(part == 0)
            {
                placement.start = value;
            } else
            {
                placement.end = value;
            }
        }
        return true;
    }
} // namespace litehtml
