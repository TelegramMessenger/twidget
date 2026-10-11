#include "html.h"
#include "el_form.h"
#include "el_text.h"
#include "document.h"
#include "document_container.h"
#include "render_item.h"
#include "css_parser.h"
#include <cstdlib>

namespace litehtml
{
    static void append_content(html_tag* parent, const char* tag, const std::string& style, const std::string& text)
    {
        auto doc   = parent->get_document();
        auto child = std::make_shared<html_tag>(doc);
        child->set_tagName(tag);
        child->set_attr("style", style.c_str());
        if(!text.empty())
        {
            child->appendChild(std::make_shared<el_text>(text.c_str(), doc));
        }
        parent->appendChild(child);
    }

    static void make_flex(css_properties& css, flex_direction direction, flex_justify_content justify,
                          flex_align_items align)
    {
        switch(css.get_display())
        {
        case display_block:
            css.set_display(display_flex);
            break;
        case display_inline:
        case display_inline_block:
            css.set_display(display_inline_flex);
            break;
        default:
            return;
        }
        css.set_flex_layout(direction, justify, align);
    }

    static void fill_box(uint_ptr hdc, document_container* container, const position& box, pixel_t radius,
                         const web_color& color)
    {
        background_layer layer;
        layer.border_box = layer.clip_box = layer.origin_box = box;
        layer.border_radius.top_left_x = layer.border_radius.top_left_y = radius;
        layer.border_radius.top_right_x = layer.border_radius.top_right_y = radius;
        layer.border_radius.bottom_right_x = layer.border_radius.bottom_right_y = radius;
        layer.border_radius.bottom_left_x = layer.border_radius.bottom_left_y = radius;
        layer.repeat = background_repeat_no_repeat;
        container->draw_solid_fill(hdc, layer, color);
    }

    static float attr_number(const html_tag* el, const char* name, float def)
    {
        const char* value = el->get_attr(name);
        if(!value || !*value)
        {
            return def;
        }
        char* end    = nullptr;
        float result = std::strtof(value, &end);
        return end == value ? def : result;
    }

    static web_color accent_or_default(const css_properties& css)
    {
        web_color accent = css.get_accent_color();
        return accent.alpha ? accent : web_color(0, 117, 255);
    }

    el_input::el_input(const document::ptr& doc) :
        html_tag(doc)
    {
    }

    bool el_input::is_text_like() const
    {
        return !is_one_of(m_type, "hidden", "checkbox", "radio", "range", "color", "button", "submit", "reset",
                          "image", "file");
    }

    bool el_input::is_button_like() const
    {
        return is_one_of(m_type, "button", "submit", "reset");
    }

    void el_input::parse_attributes()
    {
        html_tag::parse_attributes();
        const char* type = get_attr("type", "text");
        m_type           = lowcase(type ? type : "text");
        const char* value = get_attr("value");
        if(is_text_like())
        {
            std::string text        = value ? value : "";
            bool        placeholder = false;
            if(text.empty())
            {
                const char* hint = get_attr("placeholder");
                if(hint && *hint)
                {
                    text        = hint;
                    placeholder = true;
                }
            }
            if(m_type == "password" && !placeholder)
            {
                std::string dots;
                for(unsigned char ch : text)
                {
                    if((ch & 0xC0) != 0x80)
                    {
                        dots += "\xE2\x80\xA2";
                    }
                }
                text = dots;
            }
            std::string style = "display:block;flex:1 1 auto;min-width:0;overflow:hidden;white-space:pre";
            if(placeholder)
            {
                style += ";color:#757575";
            }
            append_content(this, "lh-input-text", style, text.empty() ? "\xE2\x80\x8B" : text);
        } else if(is_button_like())
        {
            std::string text = value ? value : (m_type == "reset" ? "Reset" : m_type == "submit" ? "Submit" : "");
            append_content(this, "lh-input-text", "display:block;white-space:pre",
                           text.empty() ? "\xE2\x80\x8B" : text);
        } else if(m_type == "checkbox" || m_type == "radio")
        {
            m_checked = get_attr("checked") != nullptr;
            if(m_checked && m_type == "checkbox")
            {
                append_content(this, "lh-check",
                               "display:block;color:#ffffff;font-size:10px;line-height:11px;font-weight:700;"
                               "text-align:center",
                               "\xE2\x9C\x93");
            } else if(m_checked)
            {
                append_content(this, "lh-check",
                               "display:block;width:5px;height:5px;margin:3px auto 0 auto;border-radius:50%;"
                               "background-color:#ffffff",
                               "");
            }
        } else if(m_type == "range")
        {
            float min   = attr_number(this, "min", 0);
            float max   = attr_number(this, "max", 100);
            float val   = attr_number(this, "value", min + (max - min) / 2);
            m_fraction  = max > min ? std::max(0.f, std::min(1.f, (val - min) / (max - min))) : 0.f;
        } else if(m_type == "color")
        {
            m_swatch = web_color::black;
            if(value && *value)
            {
                auto tokens = normalize(value, f_componentize | f_remove_whitespace);
                if(tokens.size() == 1)
                {
                    web_color color;
                    if(parse_color(tokens[0], color, get_document()->container()))
                    {
                        m_swatch = color;
                    }
                }
            }
        }
    }

    void el_input::compute_styles(bool recursive)
    {
        html_tag::compute_styles(false);
        if(is_text_like())
        {
            make_flex(css_w(), flex_direction_row, flex_justify_content_flex_start, flex_align_items_center);
        } else if(is_button_like())
        {
            css_w().set_center_content(true);
        } else if(m_checked)
        {
            web_color   accent = accent_or_default(css());
            background  bg     = css().get_bg();
            bg.m_color         = accent;
            css_w().set_bg(bg);
            css_borders borders = css().get_borders();
            borders.left.color = borders.right.color = borders.top.color = borders.bottom.color = accent;
            css_w().set_borders(borders);
        }
        if(recursive)
        {
            for(const auto& el : m_children)
            {
                el->compute_styles();
            }
        }
    }

    void el_input::draw(uint_ptr hdc, pixel_t x, pixel_t y, const position* clip,
                        const std::shared_ptr<render_item>& ri)
    {
        html_tag::draw(hdc, x, y, clip, ri);
        position pos  = ri->pos();
        pos.x        += x;
        pos.y        += y;
        auto container = get_document()->container();
        if(m_type == "range" && pos.width > 0_px && pos.height > 0_px)
        {
            web_color accent = accent_or_default(css());
            pixel_t   thumb  = std::min(16_px, pos.height);
            pixel_t   track  = 4_px;
            pixel_t   travel = std::max(0_px, pos.width - thumb);
            pixel_t   center = pos.x + thumb / 2_px + travel * pixel_t(m_fraction);
            position  rest(pos.x, pos.y + (pos.height - track) / 2_px, pos.width, track);
            fill_box(hdc, container, rest, track / 2_px, web_color(217, 217, 217));
            position filled(pos.x, rest.y, center - pos.x, track);
            if(filled.width > 0_px)
            {
                fill_box(hdc, container, filled, track / 2_px, accent);
            }
            position knob(center - thumb / 2_px, pos.y + (pos.height - thumb) / 2_px, thumb, thumb);
            fill_box(hdc, container, knob, thumb / 2_px, accent);
        } else if(m_type == "color" && pos.width > 0_px && pos.height > 0_px)
        {
            fill_box(hdc, container, pos, 2_px, m_swatch);
        }
    }

    el_select::el_select(const document::ptr& doc) :
        html_tag(doc)
    {
    }

    void el_select::parse_attributes()
    {
        html_tag::parse_attributes();
        std::string                                     label;
        bool                                            found = false;
        std::function<void(const element::ptr&, bool)> visit = [&](const element::ptr& el, bool any) {
            for(const auto& child : el->children())
            {
                if(found)
                {
                    return;
                }
                if(child->tag() == _option_)
                {
                    if(any || child->get_attr("selected"))
                    {
                        child->get_text(label);
                        found = true;
                        return;
                    }
                } else if(child->tag() == _optgroup_)
                {
                    visit(child, any);
                }
            }
        };
        visit(shared_from_this(), false);
        if(!found)
        {
            visit(shared_from_this(), true);
        }
        std::string trimmed = label;
        trim(trimmed);
        append_content(this, "lh-select-value", "display:block;flex:1 1 auto;min-width:0;overflow:hidden;white-space:pre",
                       trimmed.empty() ? "\xE2\x80\x8B" : trimmed);
        append_content(this, "lh-select-arrow", "display:block;flex:none;margin-left:6px;font-size:0.8em",
                       "\xE2\x8C\x84");
    }

    void el_select::compute_styles(bool recursive)
    {
        html_tag::compute_styles(false);
        make_flex(css_w(), flex_direction_row, flex_justify_content_flex_start, flex_align_items_center);
        if(recursive)
        {
            for(const auto& el : m_children)
            {
                el->compute_styles();
            }
        }
    }

    el_button::el_button(const document::ptr& doc) :
        html_tag(doc)
    {
    }

    void el_button::compute_styles(bool recursive)
    {
        html_tag::compute_styles(false);
        css_w().set_center_content(true);
        if(recursive)
        {
            for(const auto& el : m_children)
            {
                el->compute_styles();
            }
        }
    }
} // namespace litehtml
