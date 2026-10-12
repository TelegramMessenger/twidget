#include "html.h"
#include "document.h"
#include <string_view>
#include <unordered_set>
#include "document_container.h"
#include "el_anchor.h"
#include "el_base.h"
#include "el_body.h"
#include "el_break.h"
#include "el_cdata.h"
#include "el_comment.h"
#include "el_div.h"
#include "el_font.h"
#include "el_form.h"
#include "el_svg.h"
#include "el_image.h"
#include <unordered_set>
#include <map>
#include "el_link.h"
#include "el_para.h"
#include "el_script.h"
#include "el_space.h"
#include "el_style.h"
#include "el_table.h"
#include "el_td.h"
#include "el_text.h"
#include "el_title.h"
#include "el_tr.h"
#include "gumbo.h"
#include "html_tag.h"
#include "render_block.h"
#include "render_item.h"
#include "render_table.h"
#include "stylesheet.h"
#include "types.h"

#include <cstdlib>
#include <mutex>
#include <string>
#include <unordered_map>

namespace litehtml
{

    document::document(document_container* container)
    {
        m_container = container;
    }

    document::~document()
    {
        if(m_gumbo)
        {
            gumbo_destroy_output(&kGumboDefaultOptions, m_gumbo);
            m_gumbo = nullptr;
        }
        m_over_element = m_active_element = nullptr;
        if(m_container)
        {
            for(auto& font : m_fonts)
            {
                m_container->delete_font(font.second.font);
            }
        }
    }

    document::ptr document::createFromString(const estring& str, document_container* container,
                                             const std::string& master_styles, const std::string& user_styles)
    {
        // Create litehtml::document
        document::ptr doc = std::make_shared<document>(container);

        // Parse document into GumboOutput
        GumboOutput* output = doc->parse_html(str);

        // mode must be set before doc->create_node because it is used in html_tag::set_attr
        switch(output->document->v.document.doc_type_quirks_mode)
        {
        case GUMBO_DOCTYPE_NO_QUIRKS:
            doc->m_mode = no_quirks_mode;
            break;
        case GUMBO_DOCTYPE_QUIRKS:
            doc->m_mode = quirks_mode;
            break;
        case GUMBO_DOCTYPE_LIMITED_QUIRKS:
            doc->m_mode = limited_quirks_mode;
            break;
        }

        // Create litehtml::elements.
        elements_list root_elements;
        doc->create_node(output->root, root_elements, true, true);
        element::ptr root;
        if(!root_elements.empty())
        {
            root = root_elements.back();
        }

        // Destroy GumboOutput
        gumbo_destroy_output(&kGumboDefaultOptions, output);

        doc->finalize_from_external_root(root, master_styles, user_styles);

        return doc;
    }

    document::ptr document::createUpdatableFromString(const std::string& html, document_container* container,
                                                      const std::string& master_styles)
    {
        document::ptr doc = std::make_shared<document>(container);
        doc->m_text        = html;
        GumboOutput* output = gumbo_parse_with_options(&kGumboDefaultOptions, doc->m_text.data(), doc->m_text.size());
        switch(output->document->v.document.doc_type_quirks_mode)
        {
        case GUMBO_DOCTYPE_NO_QUIRKS:
            doc->m_mode = no_quirks_mode;
            break;
        case GUMBO_DOCTYPE_QUIRKS:
            doc->m_mode = quirks_mode;
            break;
        case GUMBO_DOCTYPE_LIMITED_QUIRKS:
            doc->m_mode = limited_quirks_mode;
            break;
        }
        doc->m_track_nodes = true;
        elements_list root_elements;
        doc->create_node(output->root, root_elements, true, true);
        doc->m_track_nodes = false;
        element::ptr root;
        if(!root_elements.empty())
        {
            root = root_elements.back();
        }
        doc->m_gumbo         = output;
        doc->m_master_styles = master_styles;
        doc->finalize_from_external_root(root, master_styles, {});
        doc->build_template();
        return doc;
    }

    namespace
    {
        struct node_patch
        {
            const GumboNode* old_node;
            const GumboNode* new_node;
            bool             text;
        };

        bool same_text(const char* a, const char* b)
        {
            return (a ? std::string_view(a) : std::string_view()) == (b ? std::string_view(b) : std::string_view());
        }

        // Walks both parse trees in parallel. Returns false on any difference other than the value of a
        // `style` attribute or the text of a text node outside <style>, <script>, <title> and <textarea>.
        bool diff_nodes(const GumboNode* a, const GumboNode* b, std::vector<node_patch>& patches,
                        std::vector<std::pair<const GumboNode*, const GumboNode*>>& pairs, bool raw_text)
        {
            if(a->type != b->type)
                return false;
            pairs.emplace_back(a, b);
            switch(a->type)
            {
            case GUMBO_NODE_DOCUMENT:
            case GUMBO_NODE_ELEMENT:
            case GUMBO_NODE_TEMPLATE:
            {
                const GumboVector* ca = a->type == GUMBO_NODE_DOCUMENT ? &a->v.document.children : &a->v.element.children;
                const GumboVector* cb = b->type == GUMBO_NODE_DOCUMENT ? &b->v.document.children : &b->v.element.children;
                bool               child_raw = false;
                if(a->type != GUMBO_NODE_DOCUMENT)
                {
                    const GumboElement& ea = a->v.element;
                    const GumboElement& eb = b->v.element;
                    if(ea.tag != eb.tag || ea.tag_namespace != eb.tag_namespace)
                        return false;
                    if(ea.tag == GUMBO_TAG_UNKNOWN &&
                       std::string_view(ea.original_tag.data, ea.original_tag.length) !=
                           std::string_view(eb.original_tag.data, eb.original_tag.length))
                        return false;
                    if(ea.attributes.length != eb.attributes.length)
                        return false;
                    bool style_changed = false;
                    for(unsigned int i = 0; i < ea.attributes.length; i++)
                    {
                        auto* x = static_cast<const GumboAttribute*>(ea.attributes.data[i]);
                        auto* y = static_cast<const GumboAttribute*>(eb.attributes.data[i]);
                        if(!same_text(x->name, y->name))
                            return false;
                        if(same_text(x->value, y->value))
                            continue;
                        if(strcmp(x->name, "style") != 0)
                            return false;
                        if(strstr(y->value, "counter") || strstr(x->value, "counter") || strstr(y->value, "var(") ||
                           strstr(x->value, "var(") || strstr(y->value, "--") || strstr(x->value, "--"))
                            return false;
                        style_changed = true;
                    }
                    if(style_changed)
                        patches.push_back({a, b, false});
                    child_raw = ea.tag == GUMBO_TAG_STYLE || ea.tag == GUMBO_TAG_SCRIPT || ea.tag == GUMBO_TAG_TITLE ||
                                ea.tag == GUMBO_TAG_TEXTAREA || ea.tag == GUMBO_TAG_NOSCRIPT;
                }
                if(ca->length != cb->length)
                    return false;
                for(unsigned int i = 0; i < ca->length; i++)
                {
                    if(!diff_nodes(static_cast<const GumboNode*>(ca->data[i]), static_cast<const GumboNode*>(cb->data[i]),
                                   patches, pairs, child_raw))
                        return false;
                }
                return true;
            }
            case GUMBO_NODE_TEXT:
                if(same_text(a->v.text.text, b->v.text.text))
                    return true;
                if(raw_text)
                    return false;
                patches.push_back({a, b, true});
                return true;
            default:
                return same_text(a->v.text.text, b->v.text.text);
            }
        }
    }

    // The document as literal text with holes at its style attribute values and text nodes. A frame that
    // differs only inside the holes is matched without parsing: a quoted attribute value cannot contain its
    // quote and text cannot contain '<', so each hole ends at the first delimiter that starts the next literal.
    static bool svg_geometry_attribute(const char* name)
    {
        static const std::unordered_set<std::string> names = {
            "points", "d", "x", "y", "x1", "y1", "x2", "y2", "cx", "cy", "r", "rx", "ry", "dx", "dy", "transform",
            "fill", "stroke", "stroke-width", "stroke-dasharray", "stroke-dashoffset", "opacity", "fill-opacity",
            "stroke-opacity", "offset", "stop-color", "stop-opacity", "text-anchor", "width", "height", "display", "visibility",
        };
        return names.count(name) != 0;
    }

    static std::vector<std::pair<std::string, std::string>> split_style_declarations(const std::string& text)
    {
        std::vector<std::pair<std::string, std::string>> out;
        size_t start = 0;
        int    depth = 0;
        char   quote = 0;
        for(size_t i = 0; i <= text.size(); i++)
        {
            const char c = i < text.size() ? text[i] : ';';
            if(quote)
            {
                if(c == quote)
                    quote = 0;
                continue;
            }
            if(c == '"' || c == '\'')
                quote = c;
            else if(c == '(')
                depth++;
            else if(c == ')')
                depth--;
            else if(c == ';' && depth <= 0)
            {
                std::string decl = text.substr(start, i - start);
                start = i + 1;
                size_t colon = decl.find(':');
                if(colon == std::string::npos)
                {
                    if(decl.find_first_not_of(" \t\n") != std::string::npos)
                        out.emplace_back(decl, std::string());
                    continue;
                }
                size_t a = decl.find_first_not_of(" \t\n"), b = decl.find_last_not_of(" \t\n", colon - 1);
                std::string name = a == std::string::npos || a >= colon ? std::string() : decl.substr(a, b - a + 1);
                out.emplace_back(name, decl.substr(colon + 1));
            }
        }
        return out;
    }

    // A style hole may be patched in place unless the edit touches custom properties, var() or counters,
    // whose effects reach other elements.
    static bool style_change_is_paint_only(const std::string& before, const std::string& after)
    {
        static const std::unordered_set<std::string> paint = {
            "color", "background", "background-color", "background-image", "background-position", "background-repeat",
            "opacity", "transform", "transform-origin", "box-shadow", "text-shadow", "filter", "outline", "outline-color",
            "outline-width", "outline-style", "outline-offset", "visibility", "fill", "fill-opacity", "stroke", "stroke-opacity",
            "stroke-width", "stroke-dasharray", "stroke-dashoffset", "stop-color", "stop-opacity", "border-color",
            "border-top-color", "border-right-color", "border-bottom-color", "border-left-color", "text-decoration-color",
            "caret-color", "cursor", "-lh-anim"};
        auto a = split_style_declarations(before), b = split_style_declarations(after);
        std::map<std::string, std::string> left, right;
        for(const auto& d : a)
            left[d.first] = d.second;
        for(const auto& d : b)
            right[d.first] = d.second;
        for(const auto& d : left)
        {
            auto it = right.find(d.first);
            if((it == right.end() || it->second != d.second) && !paint.count(d.first))
                return false;
        }
        for(const auto& d : right)
        {
            if(!left.count(d.first) && !paint.count(d.first))
                return false;
        }
        return true;
    }

    // The edit only repaints inside the element's own border box (svg = false), or inside the outermost <svg>
    // around it (svg = true); nothing it changes is inherited or paints outside those boxes.
    static int style_change_region(const std::string& before, const std::string& after)
    {
        static const std::unordered_set<std::string> box = {
            "background", "background-color", "background-image", "background-position", "background-repeat",
            "border-color", "border-top-color", "border-right-color", "border-bottom-color", "border-left-color",
            "caret-color", "cursor", "-lh-anim"};
        static const std::unordered_set<std::string> svg = {
            "fill", "fill-opacity", "stroke", "stroke-opacity", "stroke-width", "stroke-dasharray", "stroke-dashoffset",
            "stop-color", "stop-opacity"};
        auto a = split_style_declarations(before), b = split_style_declarations(after);
        std::map<std::string, std::string> left, right;
        for(const auto& d : a)
            left[d.first] = d.second;
        for(const auto& d : b)
            right[d.first] = d.second;
        int  region = 1;
        auto note   = [&](const std::string& name) {
            if(box.count(name))
                return;
            region = svg.count(name) && region > 0 ? 2 : 0;
        };
        for(const auto& d : left)
        {
            auto it = right.find(d.first);
            if(it == right.end() || it->second != d.second)
                note(d.first);
        }
        for(const auto& d : right)
        {
            if(!left.count(d.first))
                note(d.first);
        }
        return region;
    }

    static element::ptr outermost_svg(const element::ptr& el)
    {
        element::ptr found;
        for(auto p = el; p; p = p->parent())
        {
            if(std::dynamic_pointer_cast<el_svg>(p))
                found = p;
        }
        return found;
    }

    static bool style_change_is_local(const std::string& before, const std::string& after)
    {
        if(before.find("counter") != std::string::npos || after.find("counter") != std::string::npos)
            return false;
        auto a = split_style_declarations(before), b = split_style_declarations(after);
        auto sensitive = [](const std::pair<std::string, std::string>& d) {
            return d.first.compare(0, 2, "--") == 0;
        };
        size_t i = 0;
        while(i < a.size() && i < b.size() && a[i] == b[i])
            i++;
        size_t ea = a.size(), eb = b.size();
        while(ea > i && eb > i && a[ea - 1] == b[eb - 1])
        {
            ea--;
            eb--;
        }
        for(size_t k = i; k < ea; k++)
            if(sensitive(a[k]))
                return false;
        for(size_t k = i; k < eb; k++)
            if(sensitive(b[k]))
                return false;
        return true;
    }

    void document::build_template()
    {
        m_template_ok = false;
        m_literals.clear();
        m_holes.clear();
        m_style_holes.clear();
        if(!m_gumbo)
            return;
        const char* base = m_text.data();
        const size_t size = m_text.size();
        struct span
        {
            size_t   start, end;
            html_hole hole;
        };
        std::vector<span> spans;
        bool              ok = true;
        std::unordered_set<std::string> selected;
        for(const char* p = static_cast<const char*>(memchr(base, '[', size)); p; p = static_cast<const char*>(memchr(p + 1, '[', base + size - p - 1)))
        {
            const char* q = p + 1;
            while(q < base + size && (isalnum(static_cast<unsigned char>(*q)) || *q == '-' || *q == '_' || *q == ':'))
                q++;
            if(q > p + 1)
                selected.emplace(p + 1, q - p - 1);
            if(q >= base + size - 1)
                break;
        }
        std::function<void(const GumboNode*, bool)> walk = [&](const GumboNode* node, bool raw) {
            if(!ok)
                return;
            switch(node->type)
            {
            case GUMBO_NODE_DOCUMENT:
                for(unsigned int i = 0; i < node->v.document.children.length; i++)
                    walk(static_cast<const GumboNode*>(node->v.document.children.data[i]), false);
                break;
            case GUMBO_NODE_ELEMENT:
            case GUMBO_NODE_TEMPLATE:
            {
                const GumboElement& e = node->v.element;
                if(m_node_elements.find(node) == m_node_elements.end())
                    break;
                for(unsigned int i = 0; i < e.attributes.length; i++)
                {
                    auto*      attr = static_cast<const GumboAttribute*>(e.attributes.data[i]);
                    const bool src  = e.tag == GUMBO_TAG_IMG && strcmp(attr->name, "src") == 0;
                    const bool geometry = !src && e.tag_namespace == GUMBO_NAMESPACE_SVG && e.tag != GUMBO_TAG_SVG &&
                                          svg_geometry_attribute(attr->name) && !selected.count(attr->name);
                    if(!src && !geometry && strcmp(attr->name, "style") != 0)
                        continue;
                    const GumboStringPiece& v = attr->original_value;
                    auto it = m_node_elements.find(node);
                    if(!v.data || v.length < 2 || (v.data[0] != '"' && v.data[0] != '\'') || v.data < base ||
                       v.data + v.length > base + size || it == m_node_elements.end() || it->second.size() != 1)
                    {
                        ok = false;
                        return;
                    }
                    span s;
                    s.start       = (size_t)(v.data - base) + 1;
                    s.end         = (size_t)(v.data - base) + v.length - 1;
                    s.hole.close  = v.data[0];
                    s.hole.src    = src;
                    if(geometry)
                        s.hole.attr = attr->name;
                    s.hole.owner  = it->second.front();
                    s.hole.value  = std::string(base + s.start, s.end - s.start);
                    spans.push_back(std::move(s));
                }
                const bool child_raw = e.tag == GUMBO_TAG_STYLE || e.tag == GUMBO_TAG_SCRIPT || e.tag == GUMBO_TAG_TITLE ||
                                       e.tag == GUMBO_TAG_TEXTAREA || e.tag == GUMBO_TAG_NOSCRIPT;
                for(unsigned int i = 0; i < e.children.length; i++)
                    walk(static_cast<const GumboNode*>(e.children.data[i]), child_raw);
                break;
            }
            case GUMBO_NODE_TEXT:
            {
                if(raw)
                    break;
                const GumboStringPiece& t  = node->v.text.original_text;
                auto                    it = m_node_elements.find(node);
                if(!t.data || t.length == 0 || t.data < base || t.data + t.length > base + size ||
                   it == m_node_elements.end() || it->second.empty())
                {
                    ok = false;
                    return;
                }
                span s;
                s.start                = (size_t)(t.data - base);
                s.end                  = s.start + t.length;
                s.hole.text            = true;
                s.hole.text_elements   = it->second;
                s.hole.value           = std::string(base + s.start, t.length);
                if(s.end >= size || base[s.end] != '<')
                {
                    ok = false;
                    return;
                }
                spans.push_back(std::move(s));
                break;
            }
            default:
                break;
            }
        };
        walk(m_gumbo->document, false);
        if(!ok)
            return;
        std::sort(spans.begin(), spans.end(), [](const span& a, const span& b) { return a.start < b.start; });
        size_t pos = 0;
        for(auto& s : spans)
        {
            if(s.start < pos)
                return;
            m_literals.emplace_back(base + pos, s.start - pos);
            m_holes.push_back(std::move(s.hole));
            pos = s.end;
        }
        m_literals.emplace_back(base + pos, size - pos);
        m_template_ok = true;
    }

    namespace
    {
        bool tree_neutral_change(const std::string& before, const std::string& after)
        {
            static const std::unordered_set<std::string> neutral = {
                "width", "height", "min-width", "min-height", "max-width", "max-height", "left", "top", "right", "bottom",
                "margin", "margin-left", "margin-right", "margin-top", "margin-bottom", "padding", "padding-left",
                "padding-right", "padding-top", "padding-bottom", "border-radius", "border-top-left-radius",
                "border-top-right-radius", "border-bottom-left-radius", "border-bottom-right-radius", "border-color",
                "border-top-color", "border-right-color", "border-bottom-color", "border-left-color", "color", "background",
                "background-color", "background-image", "background-position", "background-size", "opacity", "transform",
                "transform-origin", "box-shadow", "text-shadow", "outline-color", "filter", "fill", "stroke", "stroke-width",
                "stroke-dasharray", "stroke-dashoffset", "fill-opacity", "stroke-opacity", "z-index", "text-align",
                "vertical-align", "letter-spacing", "flex-grow", "flex-shrink", "flex-basis", "gap", "row-gap", "column-gap",
                "justify-content", "align-items", "align-self"};
            auto parse = [](const std::string& text, std::map<std::string, std::string>& out) {
                if(text.find_first_of("\"'\\{}") != std::string::npos || text.find("/*") != std::string::npos ||
                   text.find("url(") != std::string::npos)
                    return false;
                size_t start = 0;
                while(start <= text.size())
                {
                    size_t end   = text.find(';', start);
                    std::string part = text.substr(start, end == std::string::npos ? std::string::npos : end - start);
                    size_t colon = part.find(':');
                    if(colon != std::string::npos)
                    {
                        std::string name = part.substr(0, colon);
                        name.erase(0, name.find_first_not_of(" \t\n"));
                        name.erase(name.find_last_not_of(" \t\n") + 1);
                        for(char& c : name)
                            c = char(tolower(static_cast<unsigned char>(c)));
                        out[name] = part.substr(colon + 1);
                    } else if(part.find_first_not_of(" \t\n") != std::string::npos)
                        return false;
                    if(end == std::string::npos)
                        break;
                    start = end + 1;
                }
                return true;
            };
            std::map<std::string, std::string> a, b;
            if(!parse(before, a) || !parse(after, b))
                return false;
            for(const auto& kv : a)
            {
                auto it = b.find(kv.first);
                if((it == b.end() || it->second != kv.second) && !neutral.count(kv.first))
                    return false;
            }
            for(const auto& kv : b)
            {
                if(!a.count(kv.first) && !neutral.count(kv.first))
                    return false;
            }
            return true;
        }
    }

    // 1: patched (or nothing changed), 0: the frame does not fit the template, -1: fits but needs a rebuild.
    int document::update_from_template(const std::string& html, bool* changed)
    {
        std::vector<std::pair<size_t, std::string>> changes;
        size_t                                      pos = 0;
        for(size_t i = 0; i < m_holes.size(); i++)
        {
            const std::string& literal = m_literals[i];
            if(html.compare(pos, literal.size(), literal) != 0)
                return 0;
            pos += literal.size();
            size_t end = html.find(m_holes[i].text ? '<' : m_holes[i].close, pos);
            if(end == std::string::npos)
                return 0;
            if(html.compare(pos, end - pos, m_holes[i].value) != 0)
                changes.emplace_back(i, html.substr(pos, end - pos));
            pos = end;
        }
        if(html.size() - pos != m_literals.back().size() || html.compare(pos, std::string::npos, m_literals.back()) != 0)
            return 0;
        return apply_template_values(changes, changed);
    }

    bool document::update_styles(const std::vector<std::pair<std::shared_ptr<element>, std::string>>& styles, bool* changed)
    {
        m_paint_only_patch = false;
        m_patch_confined   = false;
        m_patch_regions.clear();
        if(changed)
            *changed = false;
        if(!m_root || !m_finalized || !m_template_ok || getenv("LH_NO_TEMPLATE"))
            return false;
        if(m_style_holes.empty())
        {
            for(size_t i = 0; i < m_holes.size(); i++)
            {
                const html_hole& hole = m_holes[i];
                if(!hole.text && !hole.src && hole.attr.empty() && hole.owner)
                    m_style_holes.emplace(hole.owner.get(), i);
            }
        }
        std::vector<std::pair<size_t, std::string>> changes;
        changes.reserve(styles.size());
        for(const auto& entry : styles)
        {
            auto it = entry.first ? m_style_holes.find(entry.first.get()) : m_style_holes.end();
            if(it == m_style_holes.end())
                return false;
            const html_hole& hole = m_holes[it->second];
            if(entry.second.find(hole.close) != std::string::npos || entry.second.find('<') != std::string::npos)
                return false;
            if(entry.second == hole.value)
                continue;
            for(const auto& change : changes)
            {
                if(change.first == it->second)
                    return false;
            }
            changes.emplace_back(it->second, entry.second);
        }
        std::sort(changes.begin(), changes.end(), [](const auto& a, const auto& b) { return a.first < b.first; });
        return apply_template_values(changes, changed) == 1;
    }

    bool document::template_html(std::string& out) const
    {
        if(!m_template_ok || m_literals.size() != m_holes.size() + 1)
            return false;
        size_t size = 0;
        for(const auto& literal : m_literals)
            size += literal.size();
        for(const auto& hole : m_holes)
            size += hole.value.size();
        out.clear();
        out.reserve(size);
        for(size_t i = 0; i < m_holes.size(); i++)
        {
            out += m_literals[i];
            out += m_holes[i].value;
        }
        out += m_literals.back();
        return true;
    }

    int document::apply_template_values(std::vector<std::pair<size_t, std::string>>& changes, bool* changed)
    {
        std::vector<size_t> dirty;
        for(const auto& change : changes)
        {
            const size_t       i = change.first;
            const std::string& v = change.second;
            if(i >= m_holes.size() || v == m_holes[i].value)
                continue;
            if(v.find_first_of("&\r") != std::string::npos || v.find('\0') != std::string::npos)
                return -1;
            if(m_holes[i].text)
            {
                if(v.find_first_not_of(" \t\n\f") == std::string::npos)
                    return -1;
            } else if(!m_holes[i].src && m_holes[i].attr.empty() && !style_change_is_local(m_holes[i].value, v))
            {
                return -1;
            }
            if(m_holes[i].text && !m_holes[i].text_elements.front()->parent())
                return -1;
            dirty.push_back(&change - changes.data());
        }
        if(dirty.empty())
        {
            m_paint_only_patch = true;
            m_patch_confined   = true;
            m_patch_regions.clear();
            return 1;
        }
        if(changed)
            *changed = true;

        std::vector<element::ptr> restyle;
        std::vector<element::ptr> new_text;
        bool                      keep_tree  = !getenv("LH_NO_KEEP_TREE");
        bool                      paint_only = !getenv("LH_NO_PAINT_ONLY");
        m_patch_regions.clear();
        m_patch_confined = !getenv("LH_NO_PATCH_REGIONS");
        for(size_t k : dirty)
        {
            const std::string& value = changes[k].second;
            html_hole&         hole  = m_holes[changes[k].first];
            if(m_patch_confined)
            {
                element::ptr region;
                if(!hole.text && !hole.src)
                {
                    const int kind = hole.attr.empty() ? style_change_region(hole.value, value) : 2;
                    if(kind == 1)
                        region = hole.owner;
                    else if(kind == 2)
                        region = outermost_svg(hole.owner);
                }
                if(region)
                    m_patch_regions.push_back(region);
                else
                    m_patch_confined = false;
            }
            if(paint_only)
            {
                if(hole.text || hole.src)
                {
                    paint_only = false;
                } else if(!hole.attr.empty())
                {
                    bool inside_svg = false;
                    for(auto p = hole.owner->parent(); p; p = p->parent())
                    {
                        if(std::dynamic_pointer_cast<el_svg>(p))
                        {
                            inside_svg = true;
                            break;
                        }
                    }
                    paint_only = inside_svg;
                } else
                {
                    paint_only = style_change_is_paint_only(hole.value, value);
                }
            }
            if(keep_tree && (hole.text || hole.src || (hole.attr.empty() && !tree_neutral_change(hole.value, value))))
                keep_tree = false;
            hole.value      = value;
            if(hole.src)
            {
                string_map  attrs = {{"src", hole.value}};
                std::string tag   = "img";
                const bool  keep  = m_container->sanitize_element(tag, false, attrs) && attrs.count("src");
                hole.owner->set_attr("src", keep ? attrs["src"].c_str() : "");
                if(auto image = std::dynamic_pointer_cast<el_image>(hole.owner))
                    image->update_src();
                restyle.push_back(hole.owner);
                continue;
            }
            if(!hole.attr.empty())
            {
                string_map  attrs = {{hole.attr, hole.value}};
                std::string tag   = hole.owner->get_tagName();
                const bool  keep  = m_container->sanitize_element(tag, true, attrs) && attrs.count(hole.attr);
                hole.owner->set_attr(hole.attr.c_str(), keep ? attrs[hole.attr].c_str() : "");
                restyle.push_back(hole.owner);
                continue;
            }
            if(!hole.text)
            {
                hole.owner->set_attr("style", hole.value.c_str());
                restyle.push_back(hole.owner);
                continue;
            }
            elements_list created;
            m_container->split_text(
                hole.value.c_str(),
                [this, &created](const char* text) { created.push_back(std::make_shared<el_text>(text, shared_from_this())); },
                [this, &created](const char* text) { created.push_back(std::make_shared<el_space>(text, shared_from_this())); });
            hole.text_elements.front()->parent()->replace_children(hole.text_elements, created);
            hole.text_elements.assign(created.begin(), created.end());
            new_text.insert(new_text.end(), created.begin(), created.end());
        }
        m_gumbo_stale = true;
        std::vector<bool> groups;
        if(paint_only)
        {
            groups.reserve(restyle.size());
            for(const auto& el : restyle)
                groups.push_back(el->css().is_paint_group());
        }
        restyle_after_patch(restyle, new_text, keep_tree);
        if(paint_only)
        {
            for(size_t k = 0; k < restyle.size() && paint_only; k++)
            {
                if(restyle[k]->css().is_paint_group() != groups[k])
                    paint_only = false;
            }
        }
        m_paint_only_patch = paint_only && keep_tree;
        if(!m_paint_only_patch)
        {
            m_patch_confined = false;
            m_patch_regions.clear();
        }
        return 1;
    }

    bool document::take_table_cells(int count)
    {
        if(m_table_cells + count > max_table_cells)
        {
            if(!m_table_limited)
            {
                m_table_limited = true;
                m_container->sanitizer_limit("table");
            }
            return false;
        }
        m_table_cells += count;
        return true;
    }

    void document::restyle_after_patch(const std::vector<element::ptr>& restyle, const std::vector<element::ptr>& new_text,
                                       bool keep_tree)
    {
        m_share_cache.clear();
        for(const auto& el : restyle)
        {
            if(auto tag = std::dynamic_pointer_cast<html_tag>(el))
                tag->restore_base_style();
            el->compute_styles(true);
        }
        for(const auto& el : new_text)
            el->compute_styles(true);

        if(keep_tree && new_text.empty() && m_root_render)
            return;
        m_tabular_elements.clear();
        m_table_cells = 0;
        m_root_render = m_root->create_render_item(nullptr);
        fix_tables_layout();
        if(m_root_render)
            m_root_render = m_root_render->init();
    }

    bool document::update_from_string(const std::string& html, bool* changed)
    {
        m_paint_only_patch = false;
        m_patch_confined   = false;
        m_patch_regions.clear();
        if(changed)
            *changed = false;
        if(!m_gumbo || !m_root || !m_finalized)
            return false;
        if(m_template_ok && !getenv("LH_NO_TEMPLATE"))
        {
            int result = update_from_template(html, changed);
            if(result == 1)
                return true;
            if(result == -1)
                return false;
        }
        if(m_gumbo_stale)
            return false;
        std::string  text   = html;
        GumboOutput* output = gumbo_parse_with_options(&kGumboDefaultOptions, text.data(), text.size());
        if(!output)
            return false;
        auto fail = [&]() {
            gumbo_destroy_output(&kGumboDefaultOptions, output);
            return false;
        };
        if(output->document->v.document.doc_type_quirks_mode != m_gumbo->document->v.document.doc_type_quirks_mode)
            return fail();
        std::vector<node_patch>                                     patches;
        std::vector<std::pair<const GumboNode*, const GumboNode*>> pairs;
        if(!diff_nodes(m_gumbo->document, output->document, patches, pairs, false))
            return fail();
        for(const auto& patch : patches)
        {
            auto it = m_node_elements.find(patch.old_node);
            if(it == m_node_elements.end() || (!patch.text && it->second.size() != 1))
                return fail();
            if(patch.text && (it->second.empty() || !it->second.front()->parent()))
                return fail();
        }

        // Everything below can no longer fail.
        std::vector<element::ptr> restyle;
        std::vector<element::ptr> new_text;
        for(const auto& patch : patches)
        {
            auto& old_elements = m_node_elements[patch.old_node];
            if(!patch.text)
            {
                const GumboElement& e = patch.new_node->v.element;
                for(unsigned int i = 0; i < e.attributes.length; i++)
                {
                    auto* attr = static_cast<const GumboAttribute*>(e.attributes.data[i]);
                    if(!strcmp(attr->name, "style"))
                        old_elements.front()->set_attr(attr->name, attr->value);
                }
                restyle.push_back(old_elements.front());
                continue;
            }
            elements_list created;
            m_track_nodes = true;
            create_node(const_cast<GumboNode*>(patch.new_node), created, true, true);
            m_track_nodes = false;
            auto parent = old_elements.front()->parent();
            parent->replace_children(old_elements, created);
            for(const auto& el : created)
                new_text.push_back(el);
            m_node_elements.erase(patch.old_node);
        }

        std::unordered_map<const void*, std::vector<element::ptr>> remapped;
        remapped.reserve(m_node_elements.size());
        for(const auto& pair : pairs)
        {
            auto it = m_node_elements.find(pair.second);
            if(it != m_node_elements.end())
            {
                remapped[pair.second] = it->second;
                continue;
            }
            it = m_node_elements.find(pair.first);
            if(it != m_node_elements.end())
                remapped[pair.second] = it->second;
        }
        m_node_elements.swap(remapped);
        gumbo_destroy_output(&kGumboDefaultOptions, m_gumbo);
        m_gumbo = output;
        m_text.swap(text);
        build_template();

        if(patches.empty())
            return true;
        if(changed)
            *changed = true;
        restyle_after_patch(restyle, new_text);
        return true;
    }

    bool document::style_sharing_enabled() const
    {
        static const bool enabled = getenv("LH_NO_STYLE_SHARING") == nullptr;
        return enabled;
    }

    void document::set_document_mode(document_mode mode)
    {
        m_mode = mode;
    }

    void document::finalize_from_external_root(const std::shared_ptr<element>& root, const std::string& base_master_styles,
                                               const std::string& user_styles)
    {
        // Finalization accumulates state that is never rolled back: the parsed stylesheets, the media
        // lists, the tabular elements, the render tree, and the used selectors of every element. A
        // second run would duplicate all of it, so a document is finalized once, exactly like a parsed
        // one. Build a new document to rebuild.
        if(m_finalized)
        {
            return;
        }
        m_finalized = true;

        m_root = root;

        const std::string master_styles =
            m_mode == quirks_mode && base_master_styles != ""
                ? base_master_styles + "\ntable { font-size: medium; font-weight: normal; font-style: normal; }\n"
                : base_master_styles;
        if(master_styles != "")
        {
            static std::mutex                           master_cache_mutex;
            static std::unordered_map<std::string, css> master_cache;
            const bool                                  cacheable =
                master_styles.find("@media") == std::string::npos && master_styles.find("@import") == std::string::npos;
            bool hit = false;
            if(cacheable)
            {
                std::lock_guard<std::mutex> lock(master_cache_mutex);
                auto                        it = master_cache.find(master_styles);
                if(it != master_cache.end())
                {
                    m_master_css = it->second;
                    hit          = true;
                }
            }
            if(!hit)
            {
                container()->trusted_styles(true);
                m_master_css.parse_css_stylesheet(master_styles, "", shared_from_this());
                container()->trusted_styles(false);
                m_master_css.sort_selectors();
                if(cacheable)
                {
                    std::lock_guard<std::mutex> lock(master_cache_mutex);
                    if(master_cache.size() > 64)
                        master_cache.clear();
                    master_cache.emplace(master_styles, m_master_css);
                }
            }
        }
        if(user_styles != "")
        {
            container()->trusted_styles(true);
            m_user_css.parse_css_stylesheet(user_styles, "", shared_from_this());
            container()->trusted_styles(false);
            m_user_css.sort_selectors();
        }

        // Let's process created elements tree
        if(m_root)
        {
            container()->get_media_features(m_media);

            m_root->set_pseudo_class(_root_, true);

            // apply master CSS
            m_root->apply_stylesheet(m_master_css);

            // parse elements attributes
            m_root->parse_attributes();

            // Parsed and sorted document stylesheets are immutable, so documents built from the same
            // <style> text share one parse. Media and imports register per-document state while parsing,
            // so sheets that use them are parsed every time.
            static std::mutex                           styles_cache_mutex;
            static std::unordered_map<std::string, css> styles_cache;
            std::string                                 styles_key;
            bool                                        styles_cacheable = !m_css.empty();
            for(const auto& css : m_css)
            {
                if(css.media != "" || css.text.find("@media") != std::string::npos || css.text.find("@import") != std::string::npos)
                {
                    styles_cacheable = false;
                    break;
                }
            }
            bool styles_hit = false;
            if(styles_cacheable)
            {
                styles_key.push_back(static_cast<char>('0' + static_cast<int>(m_mode)));
                for(const auto& css : m_css)
                {
                    styles_key.append(css.baseurl);
                    styles_key.push_back('\x1e');
                    styles_key.append(css.text);
                    styles_key.push_back('\x1f');
                }
                std::lock_guard<std::mutex> lock(styles_cache_mutex);
                auto                        it = styles_cache.find(styles_key);
                if(it != styles_cache.end())
                {
                    m_styles   = it->second;
                    styles_hit = true;
                }
            }
            if(!styles_hit)
            {
                // parse style sheets linked in document
                for(const auto& css : m_css)
                {
                    media_query_list_list::ptr media;
                    if(css.media != "")
                    {
                        auto mq_list = parse_media_query_list(css.media, shared_from_this());
                        media        = std::make_shared<media_query_list_list>();
                        media->add(mq_list);
                    }
                    m_styles.parse_css_stylesheet(css.text, css.baseurl, shared_from_this(), media);
                }
                // Sort css selectors using CSS rules.
                m_styles.sort_selectors();
                if(styles_cacheable)
                {
                    std::lock_guard<std::mutex> lock(styles_cache_mutex);
                    if(styles_cache.size() > 128)
                        styles_cache.clear();
                    styles_cache.emplace(std::move(styles_key), m_styles);
                }
            }

            // Apply media features.
            update_media_lists(m_media);

            // Apply parsed styles.
            m_root->apply_stylesheet(m_styles);

            // Apply user styles if any
            m_root->apply_stylesheet(m_user_css);

            // Initialize element::m_css
            m_root->compute_styles();

            // Create rendering tree
            m_table_cells = 0;
            m_root_render = m_root->create_render_item(nullptr);

            // Now the m_tabular_elements is filled with tabular elements.
            // We have to check the tabular elements for missing table elements
            // and create the anonymous boxes in visual table layout
            fix_tables_layout();

            // Finally initialize elements
            // init() returns pointer to the render_init element because it can change its type
            if(m_root_render)
            {
                m_root_render = m_root_render->init();
            }
        }
    }

    // https://html.spec.whatwg.org/multipage/parsing.html#change-the-encoding
    encoding adjust_meta_encoding(encoding meta_encoding, encoding current_encoding)
    {
        // 1.
        if(current_encoding == encoding::utf_16le || current_encoding == encoding::utf_16be)
        {
            return current_encoding;
        }

        // 2.
        if(meta_encoding == encoding::utf_16le || meta_encoding == encoding::utf_16be)
        {
            return encoding::utf_8;
        }

        // 3.
        if(meta_encoding == encoding::x_user_defined)
        {
            return encoding::windows_1252;
        }

        // 4,5,6.
        return meta_encoding;
    }

    // https://html.spec.whatwg.org/multipage/parsing.html#parsing-main-inhead:change-the-encoding
    encoding get_meta_encoding(GumboNode* root)
    {
        // find <head>
        GumboNode* head = nullptr;
        for(size_t i = 0; i < root->v.element.children.length; i++)
        {
            auto* node = static_cast<GumboNode*>(root->v.element.children.data[i]);
            if(node->type == GUMBO_NODE_ELEMENT && node->v.element.tag == GUMBO_TAG_HEAD)
            {
                head = node;
                break;
            }
        }
        if(!head)
        {
            return encoding::null;
        }

        // go through <meta> tags in <head>
        for(size_t i = 0; i < head->v.element.children.length; i++)
        {
            auto* node = static_cast<GumboNode*>(head->v.element.children.data[i]);
            if(node->type != GUMBO_NODE_ELEMENT || node->v.element.tag != GUMBO_TAG_META)
            {
                continue;
            }

            auto* charset    = gumbo_get_attribute(&node->v.element.attributes, "charset");
            auto* http_equiv = gumbo_get_attribute(&node->v.element.attributes, "http-equiv");
            auto* content    = gumbo_get_attribute(&node->v.element.attributes, "content");
            // 1. If the element has a charset attribute...
            if(charset)
            {
                auto encoding = get_encoding(charset->value);
                if(encoding != encoding::null)
                {
                    return encoding;
                }
            }
            // 2. Otherwise, if the element has an http-equiv attribute...
            else if(http_equiv && t_strcasecmp(http_equiv->value, "content-type") == 0 && content)
            {
                auto encoding = extract_encoding_from_meta_element(content->value);
                if(encoding != encoding::null)
                {
                    return encoding;
                }
            }
        }

        return encoding::null;
    }

    // substitute for gumbo_parse that handles encodings
    GumboOutput* document::parse_html(estring str)
    {
        // https://html.spec.whatwg.org/multipage/parsing.html#the-input-byte-stream
        encoding_sniffing_algorithm(str);
        // cannot store output in local variable because gumbo keeps pointers into it,
        // which will be accessed later in gumbo_tag_from_original_text
        if(str.encoding == encoding::utf_8)
        {
            m_text = str;
        } else
        {
            decode(str, str.encoding, m_text);
        }

        // Gumbo does not support callbacks on node creation, so we cannot change encoding while parsing.
        // Instead, we parse entire file and then handle <meta> tags.

        // Using gumbo_parse_with_options to pass string length (m_text may contain NUL chars).
        GumboOutput* output = gumbo_parse_with_options(&kGumboDefaultOptions, m_text.data(), m_text.size());

        if(str.confidence == confidence::certain)
        {
            return output;
        }

        // Otherwise: confidence is tentative.

        // If valid HTML encoding is specified in <meta> tag...
        encoding meta_encoding = get_meta_encoding(output->root);
        if(meta_encoding != encoding::null)
        {
            // ...and it is different from currently used encoding...
            encoding new_encoding = adjust_meta_encoding(meta_encoding, str.encoding);
            if(new_encoding != str.encoding)
            {
                // ...reparse with the new encoding.
                gumbo_destroy_output(&kGumboDefaultOptions, output);
                m_text.clear();

                if(new_encoding == encoding::utf_8)
                {
                    m_text = str;
                } else
                {
                    decode(str, new_encoding, m_text);
                }
                output = gumbo_parse_with_options(&kGumboDefaultOptions, m_text.data(), m_text.size());
            }
        }

        return output;
    }

    void document::create_node(void* gnode, elements_list& elements, bool parseTextNode, bool process_root)
    {
        auto* node = static_cast<GumboNode*>(gnode);
        switch(node->type)
        {
        case GUMBO_NODE_ELEMENT:
            {
                if(process_root)
                {
                    string_map      attrs;
                    GumboAttribute* attr = nullptr;
                    for(unsigned int i = 0; i < node->v.element.attributes.length; i++)
                    {
                        attr              = static_cast<GumboAttribute*>(node->v.element.attributes.data[i]);
                        attrs[attr->name] = attr->value;
                    }

                    element::ptr ret;
                    const char*  tag = gumbo_normalized_tagname(node->v.element.tag);
                    std::string  name;
                    if(tag[0])
                    {
                        name = tag;
                    } else if(node->v.element.original_tag.data && node->v.element.original_tag.length)
                    {
                        gumbo_tag_from_original_text(&node->v.element.original_tag);
                        name.assign(node->v.element.original_tag.data, node->v.element.original_tag.length);
                    }
                    if(!name.empty())
                    {
                        if(m_build_depth >= max_build_depth || m_build_elements >= max_build_elements)
                        {
                            m_container->sanitizer_limit(m_build_depth >= max_build_depth ? "depth" : "elements");
                            break;
                        }
                        if(!m_container->sanitize_element(name, node->v.element.tag_namespace == GUMBO_NAMESPACE_SVG, attrs))
                        {
                            break;
                        }
                        ret = create_element(name.c_str(), attrs);
                        m_build_elements++;
                    }
                    if(!strcmp(tag, "script"))
                    {
                        parseTextNode = false;
                    }
                    if(ret)
                    {
                        if(m_track_nodes)
                            m_node_elements[node] = {ret};
                        elements_list child;
                        m_build_depth++;
                        for(unsigned int i = 0; i < node->v.element.children.length; i++)
                        {
                            child.clear();
                            create_node(static_cast<GumboNode*>(node->v.element.children.data[i]), child, parseTextNode,
                                        true);
                            std::for_each(child.begin(), child.end(),
                                          [&ret](element::ptr& el) { ret->appendChild(el); });
                        }
                        m_build_depth--;
                        elements.push_back(ret);
                    }
                } else
                {
                    for(unsigned int i = 0; i < node->v.element.children.length; i++)
                    {
                        create_node(static_cast<GumboNode*>(node->v.element.children.data[i]), elements, parseTextNode,
                                    true);
                    }
                }
            }
            break;
        case GUMBO_NODE_TEXT:
            {
                const size_t before = elements.size();
                if(!parseTextNode)
                {
                    elements.push_back(std::make_shared<el_text>(node->v.text.text, shared_from_this()));
                } else
                {
                    m_container->split_text(
                        node->v.text.text,
                        [this, &elements](const char* text) {
                            elements.push_back(std::make_shared<el_text>(text, shared_from_this()));
                        },
                        [this, &elements](const char* text) {
                            elements.push_back(std::make_shared<el_space>(text, shared_from_this()));
                        });
                }
                if(m_track_nodes)
                {
                    auto it = elements.begin();
                    std::advance(it, before);
                    m_node_elements[node] = std::vector<element::ptr>(it, elements.end());
                }
            }
            break;
        case GUMBO_NODE_CDATA:
            {
                element::ptr ret = std::make_shared<el_cdata>(shared_from_this());
                ret->set_data(node->v.text.text);
                elements.push_back(ret);
            }
            break;
        case GUMBO_NODE_COMMENT:
            {
                element::ptr ret = std::make_shared<el_comment>(shared_from_this());
                ret->set_data(node->v.text.text);
                elements.push_back(ret);
            }
            break;
        case GUMBO_NODE_WHITESPACE:
            {
                std::string str = node->v.text.text;
                for(size_t i = 0; i < str.length(); i++)
                {
                    elements.push_back(std::make_shared<el_space>(str.substr(i, 1).c_str(), shared_from_this()));
                }
            }
            break;
        default:
            break;
        }
    }

    element::ptr document::create_element(const char* tag_name, const string_map& attributes)
    {
        element::ptr  newTag;
        document::ptr this_doc = shared_from_this();
        if(m_container)
        {
            newTag = m_container->create_element(tag_name, attributes, this_doc);
        }
        if(!newTag)
        {
            if(!strcmp(tag_name, "br"))
            {
                newTag = std::make_shared<el_break>(this_doc);
            } else if(!strcmp(tag_name, "p"))
            {
                newTag = std::make_shared<el_para>(this_doc);
            } else if(!strcmp(tag_name, "img"))
            {
                newTag = std::make_shared<el_image>(this_doc);
            } else if(!strcmp(tag_name, "table"))
            {
                newTag = std::make_shared<el_table>(this_doc);
            } else if(!strcmp(tag_name, "td") || !strcmp(tag_name, "th"))
            {
                newTag = std::make_shared<el_td>(this_doc);
            } else if(!strcmp(tag_name, "link"))
            {
                newTag = std::make_shared<el_link>(this_doc);
            } else if(!strcmp(tag_name, "title"))
            {
                newTag = std::make_shared<el_title>(this_doc);
            } else if(!strcmp(tag_name, "a"))
            {
                newTag = std::make_shared<el_anchor>(this_doc);
            } else if(!strcmp(tag_name, "tr"))
            {
                newTag = std::make_shared<el_tr>(this_doc);
            } else if(!strcmp(tag_name, "style"))
            {
                newTag = std::make_shared<el_style>(this_doc);
            } else if(!strcmp(tag_name, "base"))
            {
                newTag = std::make_shared<el_base>(this_doc);
            } else if(!strcmp(tag_name, "body"))
            {
                newTag = std::make_shared<el_body>(this_doc);
            } else if(!strcmp(tag_name, "div"))
            {
                newTag = std::make_shared<el_div>(this_doc);
            } else if(!strcmp(tag_name, "script"))
            {
                newTag = std::make_shared<el_script>(this_doc);
            } else if(!strcmp(tag_name, "font"))
            {
                newTag = std::make_shared<el_font>(this_doc);
            } else if(!strcmp(tag_name, "svg"))
            {
                newTag = std::make_shared<el_svg>(this_doc);
            } else if(!strcmp(tag_name, "input"))
            {
                newTag = std::make_shared<el_input>(this_doc);
            } else if(!strcmp(tag_name, "select"))
            {
                newTag = std::make_shared<el_select>(this_doc);
            } else if(!strcmp(tag_name, "button"))
            {
                newTag = std::make_shared<el_button>(this_doc);
            } else
            {
                newTag = std::make_shared<html_tag>(this_doc);
            }
        }

        if(newTag)
        {
            newTag->set_tagName(tag_name);
            for(const auto& attribute : attributes)
            {
                newTag->set_attr(attribute.first.c_str(), attribute.second.c_str());
            }
        }

        return newTag;
    }

    uint_ptr document::add_font(const font_description& descr, font_metrics* fm)
    {
        uint_ptr ret = 0;

        std::string key = descr.hash();

        if(m_fonts.find(key) == m_fonts.end())
        {
            font_item fi = {0, {}};

            fi.font      = m_container->create_font(descr, this, &fi.metrics);
            m_fonts[key] = fi;
            ret          = fi.font;
            if(fm)
            {
                *fm = fi.metrics;
            }
        }
        return ret;
    }

    uint_ptr document::get_font(const font_description& descr, font_metrics* fm)
    {
        if(descr.size == 0_px)
        {
            return 0;
        }

        auto key = descr.hash();

        auto el = m_fonts.find(key);

        if(el != m_fonts.end())
        {
            if(fm)
            {
                *fm = el->second.metrics;
            }
            return el->second.font;
        }
        return add_font(descr, fm);
    }

    pixel_t document::render(pixel_t max_width, render_type rt)
    {
        pixel_t ret = 0_px;
        if(m_root && m_root_render)
        {
            position viewport;
            m_container->get_viewport(viewport);
            containing_block_context cb_context;
            cb_context.width       = max_width;
            cb_context.width.type  = containing_block_context::cbc_value_type_absolute;
            cb_context.height      = viewport.height;
            cb_context.height.type = containing_block_context::cbc_value_type_absolute;

            if(rt == render_fixed_only)
            {
                m_fixed_boxes.clear();
                m_root_render->render_positioned(rt);
            } else
            {
                ret = m_root_render->render(0_px, 0_px, cb_context, nullptr).natural_width;
                if(m_root_render->fetch_positioned())
                {
                    m_fixed_boxes.clear();
                    m_root_render->render_positioned(rt);
                }
                m_size.width  = 0;
                m_size.height = 0;
                m_root_render->calc_document_size(m_size);
            }
        }
        return ret;
    }

    void document::draw(uint_ptr hdc, pixel_t x, pixel_t y, const position* clip)
    {
        if(m_root && m_root_render)
        {
            m_root->draw(hdc, x, y, clip, m_root_render);
            m_root_render->draw_stacking_context(hdc, x, y, clip, true);
        }
    }

    pixel_t document::to_pixels(const css_length& val, const font_metrics& metrics, pixel_t size) const
    {
        if(val.is_predefined())
        {
            return 0_px;
        }
        pixel_t ret;
        switch(val.units())
        {
        case css_units_percentage:
            ret = val.calc_percent(size);
            break;
        case css_units_em:
            ret = metrics.font_size * val.val();
            break;

        // https://drafts.csswg.org/css-values-4/#absolute-lengths
        case css_units_pt:
            ret = m_container->pt_to_px(val.val());
            break;
        case css_units_in:
            ret = m_container->pt_to_px(val.val() * 72); // 1in = 72pt
            break;
        case css_units_pc:
            ret = m_container->pt_to_px(val.val() * 12); // 1pc = (1/6)in = 12pt
            break;
        case css_units_cm:
            ret = m_container->pt_to_px(val.val() * 72 / 2.54f); // 1cm = (1/2.54)in = (72/2.54)pt
            break;
        case css_units_mm:
            ret = m_container->pt_to_px(val.val() * 72 / 2.54f / 10);
            break;

        case css_units_vw:
            ret = pixel_t(m_media.width.value() * val.val() / 100.0f);
            break;
        case css_units_vh:
            ret = pixel_t(m_media.height.value() * val.val() / 100.0f);
            break;
        case css_units_vmin:
            ret = pixel_t(std::min(m_media.height.value(), m_media.width.value()) * val.val() / 100.0f);
            break;
        case css_units_vmax:
            ret = pixel_t(std::max(m_media.height.value(), m_media.width.value()) * val.val() / 100.0f);
            break;
        case css_units_rem:
            ret = pixel_t(m_root->css().get_font_size().value() * val.val());
            break;
        case css_units_ex:
            ret = pixel_t(metrics.x_height.value() * val.val());
            break;
        case css_units_ch:
            ret = pixel_t(metrics.ch_width.value() * val.val());
            break;
        default:
            ret = pixel_t(val.val());
            break;
        }
        return ret;
    }

    void document::cvt_units(css_length& val, const font_metrics& metrics, pixel_t size) const
    {
        if(val.is_predefined())
        {
            return;
        }
        if(val.units() != css_units_percentage)
        {
            val.set_value(static_cast<float>(to_pixels(val, metrics, size)), css_units_px);
        }
    }

    pixel_t document::width() const
    {
        return m_size.width;
    }

    pixel_t document::height() const
    {
        return m_size.height;
    }

    void document::add_stylesheet(const char* str, const char* baseurl, const char* media)
    {
        if(str && str[0])
        {
            m_css.emplace_back(str, baseurl, media);
        }
    }

    bool document::on_mouse_over(pixel_t x, pixel_t y, pixel_t client_x, pixel_t client_y,
                                 const std::function<void(const position&)>& redraw_box)
    {
        if(!m_root || !m_root_render)
        {
            return false;
        }

        element::ptr over_el = m_root_render->get_element_by_point(x, y, client_x, client_y, nullptr);

        bool state_was_changed = false;

        if(over_el != m_over_element)
        {
            if(m_over_element)
            {
                if(m_over_element->on_mouse_leave())
                {
                    m_container->on_mouse_event(m_over_element, mouse_event_leave);
                    state_was_changed = true;
                }
            }
            m_over_element = over_el;
        }

        std::string cursor;

        if(m_over_element)
        {
            if(m_over_element->on_mouse_over())
            {
                state_was_changed = true;
            }
            cursor = m_over_element->css().get_cursor();
        }

        m_container->set_cursor(cursor.c_str());

        if(state_was_changed)
        {
            m_container->on_mouse_event(m_over_element, mouse_event_enter);
            return m_root->find_styles_changes(redraw_box);
        }
        return false;
    }

    std::vector<scroll_values> document::on_scroll(pixel_t dx, pixel_t dy, pixel_t x, pixel_t y, pixel_t client_x,
                                                   pixel_t client_y) const
    {
        if(dy == 0_px && dx == 0_px)
        {
            return {};
        }

        element::ptr vscroll_el;
        element::ptr hscroll_el;

        if(dy != 0_px)
        {
            vscroll_el = m_root_render->get_element_by_point(
                x, y, client_x, client_y,
                [dy](const std::shared_ptr<render_item>& el) -> bool { return el->is_v_scrollable(dy); });
        }

        if(dx != 0_px)
        {
            hscroll_el = m_root_render->get_element_by_point(
                x, y, client_x, client_y,
                [dx](const std::shared_ptr<render_item>& el) -> bool { return el->is_h_scrollable(dx); });
        }

        if(!vscroll_el && !hscroll_el)
        {
            return {};
        }

        if(vscroll_el == hscroll_el)
        {
            scroll_values sv;
            sv.dx         = hscroll_el->h_scroll(dx);
            sv.dy         = vscroll_el->v_scroll(dy);
            sv.scroll_box = hscroll_el->get_placement();
            return {sv};
        }

        std::vector<scroll_values> ret;
        ret.reserve(2);

        if(vscroll_el)
        {
            scroll_values sv;
            sv.dy         = vscroll_el->v_scroll(dy);
            sv.scroll_box = vscroll_el->get_placement();
            ret.push_back(sv);
        }

        if(hscroll_el)
        {
            scroll_values sv;
            sv.dx         = hscroll_el->h_scroll(dx);
            sv.scroll_box = hscroll_el->get_placement();
            ret.push_back(sv);
        }

        return ret;
    }

    bool document::on_mouse_leave(const std::function<void(const position&)>& redraw_box)
    {
        if(!m_root || !m_root_render)
        {
            return false;
        }
        if(m_over_element)
        {
            auto el        = m_over_element;
            m_over_element = nullptr;
            if(el->on_mouse_leave())
            {
                m_container->on_mouse_event(el, mouse_event_leave);
                return m_root->find_styles_changes(redraw_box);
            }
        }
        return false;
    }

    bool document::on_lbutton_down(pixel_t x, pixel_t y, pixel_t client_x, pixel_t client_y,
                                   const std::function<void(const position&)>& redraw_box)
    {
        if(!m_root || !m_root_render)
        {
            return false;
        }

        element::ptr over_el = m_root_render->get_element_by_point(x, y, client_x, client_y, nullptr);
        m_active_element     = over_el;

        bool state_was_changed = false;

        if(over_el != m_over_element)
        {
            if(m_over_element)
            {
                if(m_over_element->on_mouse_leave())
                {
                    m_container->on_mouse_event(m_over_element, mouse_event_leave);
                    state_was_changed = true;
                }
            }
            m_over_element = over_el;
            if(m_over_element)
            {
                if(m_over_element->on_mouse_over())
                {
                    state_was_changed = true;
                }
            }
        }

        std::string cursor;

        if(m_over_element)
        {
            if(m_over_element->on_lbutton_down())
            {
                state_was_changed = true;
            }
            cursor = m_over_element->css().get_cursor();
        }

        m_container->set_cursor(cursor.c_str());

        if(state_was_changed)
        {
            m_container->on_mouse_event(m_over_element, mouse_event_enter);
            return m_root->find_styles_changes(redraw_box);
        }

        return false;
    }

    bool document::on_lbutton_up(pixel_t /*x*/, pixel_t /*y*/, pixel_t /*client_x*/, pixel_t /*client_y*/,
                                 const std::function<void(const position&)>& redraw_box)
    {
        if(!m_root || !m_root_render)
        {
            return false;
        }
        if(m_over_element)
        {
            if(m_over_element->on_lbutton_up(m_active_element == m_over_element))
            {
                return m_root->find_styles_changes(redraw_box);
            }
        }
        return false;
    }

    bool document::on_button_cancel(const std::function<void(const position&)>& redraw_box)
    {
        m_active_element = nullptr;
        return on_mouse_leave(redraw_box);
    }

    void document::get_fixed_boxes(position::vector& fixed_boxes)
    {
        fixed_boxes = m_fixed_boxes;
    }

    void document::add_fixed_box(const position& pos)
    {
        m_fixed_boxes.push_back(pos);
    }

    bool document::media_changed()
    {
        container()->get_media_features(m_media);
        if(update_media_lists(m_media))
        {
            m_root->refresh_styles();
            m_root->compute_styles();
            // The set of rendered elements can change across a media breakpoint
            // (e.g. display:none <-> block on responsive nav/hero blocks). The render
            // tree is built once in createFromString() from the computed display values,
            // so rebuild it here to add/remove render items for elements whose display
            // just changed; otherwise a newly-shown element keeps no render item and
            // never lays out (it collapses to zero).
            m_table_cells = 0;
            m_root_render = m_root->create_render_item(nullptr);
            if(m_root_render)
            {
                m_root_render = m_root_render->init();
            }
            return true;
        }
        return false;
    }

    bool document::lang_changed()
    {
        if(!m_media_lists.empty())
        {
            std::string culture;
            container()->get_language(m_lang, culture);
            if(!culture.empty())
            {
                m_culture = m_lang + '-' + culture;
            } else
            {
                m_culture.clear();
            }
            m_root->refresh_styles();
            m_root->compute_styles();
            return true;
        }
        return false;
    }

    // Apply media features (determine which selectors are active).
    bool document::update_media_lists(const media_features& features)
    {
        bool update_styles = false;
        for(auto& media_list : m_media_lists)
        {
            if(media_list->apply_media_features(features))
            {
                update_styles = true;
            }
        }
        return update_styles;
    }

    void document::add_media_list(const media_query_list_list::ptr& list)
    {
        if(list && !contains(m_media_lists, list))
        {
            m_media_lists.push_back(list);
        }
    }

    void document::fix_tables_layout()
    {
        const std::size_t limit = m_tabular_elements.size() * 8 + 64;
        std::size_t       processed = 0;
        for(const auto& el_ptr : m_tabular_elements)
        {
            if(++processed > limit)
            {
                break;
            }
            switch(el_ptr->src_el()->css().get_display())
            {
            case display_inline_table:
            case display_table:
                fix_table_children(el_ptr, display_table_row_group, "table-row-group");
                break;
            case display_table_footer_group:
            case display_table_row_group:
            case display_table_header_group:
                {
                    auto parent = el_ptr->parent();
                    if(parent)
                    {
                        if(parent->src_el()->css().get_display() != display_inline_table)
                        {
                            fix_table_parent(el_ptr, display_table, "table");
                        }
                    }
                    fix_table_children(el_ptr, display_table_row, "table-row");
                }
                break;
            case display_table_row:
                fix_table_parent(el_ptr, display_table_row_group, "table-row-group");
                fix_table_children(el_ptr, display_table_cell, "table-cell");
                break;
            case display_table_cell:
                fix_table_parent(el_ptr, display_table_row, "table-row");
                break;
            // TODO: make table layout fix for table-caption, table-column etc. elements
            case display_table_caption:
            case display_table_column:
            case display_table_column_group:
            default:
                break;
            }
        }
    }

    void document::fix_table_children(const std::shared_ptr<render_item>& el_ptr, style_display disp,
                                      const char* disp_str)
    {
        std::list<std::shared_ptr<render_item>> tmp;
        auto                                    first_iter = el_ptr->children().begin();
        auto                                    cur_iter   = el_ptr->children().begin();

        auto flush_elements = [&]() {
            element::ptr annon_tag = std::make_shared<html_tag>(el_ptr->src_el(), std::string("display:") + disp_str);
            std::shared_ptr<render_item> annon_ri;
            if(annon_tag->css().get_display() == display_table_cell)
            {
                annon_tag->set_tagName("table_cell");
                annon_ri = std::make_shared<render_item_block>(annon_tag);
            } else if(annon_tag->css().get_display() == display_table_row)
            {
                annon_ri = std::make_shared<render_item_table_row>(annon_tag);
            } else
            {
                annon_ri = std::make_shared<render_item_table_part>(annon_tag);
            }
            for(const auto& el : tmp)
            {
                annon_ri->add_child(el);
            }
            // add annon item as tabular for future processing
            add_tabular(annon_ri);
            annon_ri->parent(el_ptr);
            first_iter = el_ptr->children().insert(first_iter, annon_ri);
            cur_iter   = std::next(first_iter);
            while(cur_iter != el_ptr->children().end() && (*cur_iter)->parent() != el_ptr)
            {
                cur_iter = el_ptr->children().erase(cur_iter);
            }
            first_iter = cur_iter;
            tmp.clear();
        };

        while(cur_iter != el_ptr->children().end())
        {
            if((*cur_iter)->src_el()->css().get_display() != disp)
            {
                if(!(*cur_iter)->src_el()->is_table_skip() || ((*cur_iter)->src_el()->is_table_skip() && !tmp.empty()))
                {
                    if(disp != display_table_row_group ||
                       (*cur_iter)->src_el()->css().get_display() != display_table_caption)
                    {
                        if(tmp.empty())
                        {
                            first_iter = cur_iter;
                        }
                        tmp.push_back((*cur_iter));
                    }
                }
                cur_iter++;
            } else if(!tmp.empty())
            {
                flush_elements();
            } else
            {
                cur_iter++;
            }
        }
        if(!tmp.empty())
        {
            flush_elements();
        }
    }

    void document::fix_table_parent(const std::shared_ptr<render_item>& el_ptr, style_display disp,
                                    const char* disp_str)
    {
        auto parent = el_ptr->parent();

        if(parent->src_el()->css().get_display() != disp)
        {
            auto this_element = std::find_if(parent->children().begin(), parent->children().end(),
                                             [&](const std::shared_ptr<render_item>& el) { return el == el_ptr; });
            if(this_element != parent->children().end())
            {
                style_display el_disp = el_ptr->src_el()->css().get_display();
                auto          first   = this_element;
                auto          last    = this_element;
                auto          cur     = this_element;

                // find first element with same display
                while(true)
                {
                    if(cur == parent->children().begin())
                    {
                        break;
                    }
                    cur--;
                    if((*cur)->src_el()->is_table_skip() || (*cur)->src_el()->css().get_display() == el_disp)
                    {
                        first = cur;
                    } else
                    {
                        break;
                    }
                }

                // find last element with same display
                cur = this_element;
                while(true)
                {
                    cur++;
                    if(cur == parent->children().end())
                    {
                        break;
                    }

                    if((*cur)->src_el()->is_table_skip() || (*cur)->src_el()->css().get_display() == el_disp)
                    {
                        last = cur;
                    } else
                    {
                        break;
                    }
                }

                // extract elements with the same display and wrap them with anonymous object
                element::ptr annon_tag =
                    std::make_shared<html_tag>(parent->src_el(), std::string("display:") + disp_str);
                std::shared_ptr<render_item> annon_ri;
                if(annon_tag->css().get_display() == display_table ||
                   annon_tag->css().get_display() == display_inline_table)
                {
                    annon_ri = std::make_shared<render_item_table>(annon_tag);
                } else if(annon_tag->css().get_display() == display_table_row)
                {
                    annon_ri = std::make_shared<render_item_table_row>(annon_tag);
                } else
                {
                    annon_ri = std::make_shared<render_item_table_part>(annon_tag);
                }
                std::for_each(first, std::next(last, 1),
                              [&annon_ri](std::shared_ptr<render_item>& el) { annon_ri->add_child(el); });
                first = parent->children().erase(first, std::next(last));
                parent->children().insert(first, annon_ri);
                add_tabular(annon_ri);
                annon_ri->parent(parent);
            }
        }
    }

    void document::append_children_from_string(element& parent, const char* str, bool replace_existing)
    {
        // parent must belong to this document
        if(parent.get_document().get() != this)
        {
            return;
        }

        GumboOptions opts = kGumboDefaultOptions;
        // This is require to prevent creating html, head, body tags around the fragment
        // Although Gumbo always creates html tag anyway. We have to ignore it in create_node.
        opts.fragment_context = GUMBO_TAG_BODY;
        // parse document into GumboOutput
        GumboOutput* output = gumbo_parse_with_options(&opts, str, strlen(str));

        // Create litehtml::elements.
        elements_list child_elements;
        // Create elements excluding the root node
        create_node(output->root, child_elements, true, false);

        // Destroy GumboOutput
        gumbo_destroy_output(&kGumboDefaultOptions, output);

        auto parent_render = parent.get_render_item();

        if(replace_existing)
        {
            parent.clearRecursive();
            parent_render->children().clear();
        }

        // Let's process created elements tree
        for(const auto& child : child_elements)
        {
            // Add the child element to parent
            parent.appendChild(child);

            // apply master CSS
            child->apply_stylesheet(m_master_css);

            // parse elements attributes
            child->parse_attributes();

            // Apply parsed styles.
            child->apply_stylesheet(m_styles);

            // Apply user styles if any
            child->apply_stylesheet(m_user_css);

            // Initialize m_css
            child->compute_styles();

            // Finally initialize elements
            if(parent_render)
            {
                auto child_render = child->create_render_item(parent_render);
                if(child_render)
                {
                    child_render = child_render->init();
                    parent_render->add_child(child_render);
                }
            }
        }
        // Now the m_tabular_elements is filled with tabular elements.
        // We have to check the tabular elements for missing table elements
        // and create the anonymous boxes in visual table layout
        fix_tables_layout();
    }

    void document::dump(dumper& cout)
    {
        if(m_root_render)
        {
            m_root_render->dump(cout);
        }
    }

} // namespace litehtml
