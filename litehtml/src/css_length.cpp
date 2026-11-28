#include "html.h"
#include "css_length.h"

#include <algorithm>
#include <cmath>
#include <vector>

namespace litehtml
{
    namespace
    {
        struct calc_node
        {
            enum kind_t : uint8_t
            {
                leaf,
                sum,     // args, each with its sign in `signs`
                scaled,  // args[0] times value
                min,
                max,
                clamp
            } kind = leaf;
            float             value = 0;  // leaf: the number; scaled: the factor
            css_units         unit  = css_units_none;
            std::vector<calc_node> args;
            std::vector<int8_t>    signs;
        };

        constexpr size_t MAX_NODES = 128;

        size_t node_count(const calc_node& root)
        {
            size_t                        count = 0;
            std::vector<const calc_node*> pending = {&root};
            while(!pending.empty())
            {
                const calc_node* node = pending.back();
                pending.pop_back();
                count++;
                for(const auto& arg : node->args)
                    pending.push_back(&arg);
            }
            return count;
        }

        void skip_whitespace(const css_token_vector& tokens, size_t& i)
        {
            while(i < tokens.size() && tokens[i].type == WHITESPACE)
                i++;
        }

        bool parse_sum(const css_token_vector& tokens, size_t& i, calc_node& out, int depth);

        bool parse_list(const css_token_vector& tokens, std::vector<calc_node>& out, int depth)
        {
            size_t i = 0;
            while(true)
            {
                calc_node arg;
                if(!parse_sum(tokens, i, arg, depth))
                    return false;
                out.push_back(std::move(arg));
                skip_whitespace(tokens, i);
                if(i == tokens.size())
                    return out.size() <= 32;
                if(tokens[i].ch != ',')
                    return false;
                i++;
            }
        }

        bool parse_atom(const css_token_vector& tokens, size_t& i, calc_node& out, int depth)
        {
            skip_whitespace(tokens, i);
            if(i >= tokens.size() || depth > 16)
                return false;
            const css_token& tok = tokens[i++];
            if(tok.type == NUMBER || tok.type == PERCENTAGE)
            {
                out.value = tok.n.number;
                out.unit  = tok.type == NUMBER ? css_units_none : css_units_percentage;
                return true;
            }
            if(tok.type == DIMENSION)
            {
                auto unit = css_values(css_units_strings).value_index(lowcase(tok.unit()));
                if(!unit || is_one_of(*unit, css_units_none, css_units_percentage, css_units_calc))
                    return false;
                out.value = tok.n.number;
                out.unit  = static_cast<css_units>(*unit);
                return true;
            }
            const std::string name = tok.type == CV_FUNCTION ? lowcase(tok.name()) : std::string();
            if(tok.type == ROUND_BLOCK || name == "calc")
            {
                size_t inner = 0;
                if(!parse_sum(tok.value, inner, out, depth + 1))
                    return false;
                skip_whitespace(tok.value, inner);
                return inner == tok.value.size();
            }
            if(is_one_of(name, "min", "max", "clamp"))
            {
                out.kind = name == "min" ? calc_node::min : name == "max" ? calc_node::max : calc_node::clamp;
                return parse_list(tok.value, out.args, depth + 1) && (out.kind != calc_node::clamp || out.args.size() == 3);
            }
            return false;
        }

        // A plain number, unitless and constant: the only thing a product may scale by.
        bool is_number(const calc_node& node)
        {
            return node.kind == calc_node::leaf && node.unit == css_units_none;
        }

        bool parse_product(const css_token_vector& tokens, size_t& i, calc_node& out, int depth)
        {
            if(!parse_atom(tokens, i, out, depth))
                return false;
            while(true)
            {
                size_t probe = i;
                skip_whitespace(tokens, probe);
                if(probe >= tokens.size() || (tokens[probe].ch != '*' && tokens[probe].ch != '/'))
                    return true;
                const bool divide = tokens[probe].ch == '/';
                i = probe + 1;
                calc_node right;
                if(!parse_atom(tokens, i, right, depth))
                    return false;
                if(divide || is_number(right))
                {
                    if(!is_number(right) || (divide && right.value == 0))
                        return false;
                    float factor = divide ? 1 / right.value : right.value;
                    if(is_number(out) || out.kind == calc_node::scaled)
                    {
                        out.value *= factor;
                        continue;
                    }
                    calc_node scaled;
                    scaled.kind  = calc_node::scaled;
                    scaled.value = factor;
                    scaled.args.push_back(std::move(out));
                    out = std::move(scaled);
                } else if(is_number(out))
                {
                    calc_node scaled;
                    scaled.kind  = calc_node::scaled;
                    scaled.value = out.value;
                    scaled.args.push_back(std::move(right));
                    out = std::move(scaled);
                } else
                {
                    return false;
                }
            }
        }

        bool parse_sum(const css_token_vector& tokens, size_t& i, calc_node& out, int depth)
        {
            calc_node first;
            if(!parse_product(tokens, i, first, depth))
                return false;
            calc_node sum;
            sum.kind = calc_node::sum;
            sum.args.push_back(std::move(first));
            sum.signs.push_back(1);
            while(true)
            {
                size_t probe = i;
                skip_whitespace(tokens, probe);
                if(probe >= tokens.size() || (tokens[probe].ch != '+' && tokens[probe].ch != '-'))
                    break;
                i = probe + 1;
                calc_node term;
                if(!parse_product(tokens, i, term, depth))
                    return false;
                sum.args.push_back(std::move(term));
                sum.signs.push_back(tokens[probe].ch == '-' ? -1 : 1);
            }
            out = sum.args.size() == 1 ? std::move(sum.args.front()) : std::move(sum);
            return true;
        }

        float evaluate(const calc_node& node, float base, const calc_convert_fn& convert)
        {
            switch(node.kind)
            {
            case calc_node::leaf:
                if(node.unit == css_units_percentage)
                    return base * node.value / 100;
                return convert && node.unit != css_units_none && node.unit != css_units_px ? convert(node.value, node.unit) : node.value;
            case calc_node::sum:
                {
                    float total = 0;
                    for(size_t i = 0; i < node.args.size(); i++)
                        total += node.signs[i] * evaluate(node.args[i], base, convert);
                    return total;
                }
            case calc_node::scaled:
                return node.value * evaluate(node.args[0], base, convert);
            case calc_node::clamp:
                {
                    float low = evaluate(node.args[0], base, convert);
                    return std::max(low, std::min(evaluate(node.args[1], base, convert), evaluate(node.args[2], base, convert)));
                }
            default:
                {
                    float value = evaluate(node.args[0], base, convert);
                    for(size_t i = 1; i < node.args.size(); i++)
                    {
                        float arg = evaluate(node.args[i], base, convert);
                        value     = node.kind == calc_node::min ? std::min(value, arg) : std::max(value, arg);
                    }
                    return value;
                }
            }
        }

        bool has_percent(const calc_node& node)
        {
            if(node.kind == calc_node::leaf)
                return node.unit == css_units_percentage;
            return std::any_of(node.args.begin(), node.args.end(), has_percent);
        }

        bool is_converted(const calc_node& node)
        {
            if(node.kind == calc_node::leaf)
                return is_one_of(node.unit, css_units_percentage, css_units_none, css_units_px);
            return std::all_of(node.args.begin(), node.args.end(), is_converted);
        }

        void convert_leaves(calc_node& node, const calc_convert_fn& convert)
        {
            if(node.kind == calc_node::leaf)
            {
                if(node.unit != css_units_percentage && node.unit != css_units_none && node.unit != css_units_px)
                {
                    node.value = convert(node.value, node.unit);
                    node.unit  = css_units_px;
                }
                return;
            }
            for(auto& arg : node.args)
                convert_leaves(arg, convert);
        }
    } // namespace

    struct calc_expression
    {
        calc_node root;
    };

    pixel_t calc_evaluate(const calc_expression* expression, pixel_t percent_base, const calc_convert_fn& convert)
    {
        if(!expression)
            return 0_px;
        float value = evaluate(expression->root, percent_base.value(), convert);
        return pixel_t(std::isfinite(value) ? value : 0.f);
    }

    std::shared_ptr<const calc_expression> calc_resolve_units(const std::shared_ptr<const calc_expression>& expression,
                                                              const calc_convert_fn& convert, float& px)
    {
        px = 0;
        if(!expression)
            return nullptr;
        const bool converted = is_converted(expression->root);
        if(converted && has_percent(expression->root))
            return expression;
        calc_node node = expression->root;
        if(!converted)
            convert_leaves(node, convert);
        if(!has_percent(node))
        {
            px = evaluate(node, 0, nullptr);
            if(!std::isfinite(px))
                px = 0;
            return nullptr;
        }
        return std::make_shared<const calc_expression>(calc_expression{std::move(node)});
    }

    bool css_length::from_token(const css_token& token, int options, css_values keywords)
    {
        if((options & f_positive) && is_one_of(token.type, NUMBER, DIMENSION, PERCENTAGE) && token.n.number < 0)
        {
            return false;
        }

        // Math the stylesheet could not fold (style.cpp, fold_math) mixes units: it waits for layout.
        if(token.type == CV_FUNCTION && (options & f_length) && (options & f_percentage) &&
           is_one_of(lowcase(token.name()), "calc", "min", "max", "clamp"))
        {
            calc_node node;
            size_t    i = 0;
            css_token_vector single = {token};
            if(!parse_atom(single, i, node, 0) || is_number(node) || node_count(node) > MAX_NODES)
            {
                return false;
            }
            set_calc(std::make_shared<const calc_expression>(calc_expression{std::move(node)}));
            return true;
        }

        if(token.type == IDENT)
        {
            auto idx = keywords.value_index(lowcase(token.name()));
            if(!idx.has_value())
            {
                return false;
            }
            predef(idx.value());
            return true;
        }
        if(token.type == DIMENSION)
        {
            if(!(options & f_length))
            {
                return false;
            }

            auto idx = css_values(css_units_strings).value_index(lowcase(token.unit()));
            // note: 1none and 1\% are invalid
            if(!idx.has_value() || is_one_of(idx.value(), css_units_none, css_units_percentage, css_units_calc))
            {
                return false;
            }

            set_value(token.n.number, static_cast<css_units>(idx.value()));
            return true;
        }
        if(token.type == PERCENTAGE)
        {
            if(!(options & f_percentage))
            {
                return false;
            }

            set_value(token.n.number, css_units_percentage);
            return true;
        }
        if(token.type == NUMBER)
        {
            // if token is a nonzero number and neither f_number nor f_integer is specified in the options
            if(!(options & (f_number | f_integer)) && pixel_t(token.n.number) != 0_px)
            {
                return false;
            }
            // if token is a zero number and neither of f_number, f_integer or f_length are specified in the options
            if(!(options & (f_number | f_integer | f_length)) && pixel_t(token.n.number) == 0_px)
            {
                return false;
            }
            if((options & f_integer) && token.n.number_type != css_number_integer)
            {
                return false;
            }

            set_value(token.n.number, css_units_none);
            return true;
        }
        return false;
    }

    std::string css_length::to_string() const
    {
        if(m_is_predefined)
        {
            return "def(" + std::to_string(m_predef) + ")";
        }
        std::string res  = std::to_string(m_value) + "{";
        res             += css_values(css_units_strings).value_by_index(m_units);
        res             += "}";
        return res;
    }

    css_length css_length::predef_value(int val)
    {
        css_length len;
        len.predef(val);
        return len;
    }

} // namespace litehtml
