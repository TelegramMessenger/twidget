// lhdump's 'i' and 'I' records: the fork's DOM as document.cpp builds it from Gumbo's tree, and what parse_attributes adds to each element's style (rust/crates/litehtml/tests/common/dom.rs lays them out).
#pragma once

#include "document.h"
#include "document_container.h"
#include "el_anchor.h"
#include "el_base.h"
#include "el_before_after.h"
#include "el_body.h"
#include "el_break.h"
#include "el_cdata.h"
#include "el_comment.h"
#include "el_div.h"
#include "el_font.h"
#include "el_form.h"
#include "el_image.h"
#include "el_link.h"
#include "el_para.h"
#include "el_script.h"
#include "el_space.h"
#include "el_style.h"
#include "el_svg.h"
#include "el_table.h"
#include "el_td.h"
#include "el_text.h"
#include "el_title.h"
#include "el_tr.h"
#include "gumbo.h"
#include "html_tag.h"
#include "style.h"

#include <cstdint>
#include <memory>
#include <string>
#include <typeindex>
#include <typeinfo>
#include <unordered_map>
#include <vector>

namespace lhdump_dom {

using namespace litehtml;

// The fork's private members, reached as the language allows without changing the fork: an explicit instantiation may name any member.
template <auto Member, typename Tag> struct Rob {
    friend auto robbed(Tag) { return Member; }
};

#define LHDUMP_ROB(Name, Member) \
    struct Name {}; \
    auto robbed(Name); \
    template struct Rob<&Member, Name>;

LHDUMP_ROB(DocText, document::m_text)
LHDUMP_ROB(DocGumbo, document::m_gumbo)
LHDUMP_ROB(DocTrackNodes, document::m_track_nodes)
LHDUMP_ROB(DocBuildLimited, document::m_build_limited)
LHDUMP_ROB(DocBuildElements, document::m_build_elements)
LHDUMP_ROB(DocBuildDepth, document::m_build_depth)
LHDUMP_ROB(DocNodeElements, document::m_node_elements)
LHDUMP_ROB(DocLiterals, document::m_literals)
LHDUMP_ROB(DocHoles, document::m_holes)
LHDUMP_ROB(DocTemplateOk, document::m_template_ok)
LHDUMP_ROB(DocNodeHoles, document::m_node_holes)
LHDUMP_ROB(DocCss, document::m_css)
LHDUMP_ROB(DocCreateNode, document::create_node)
LHDUMP_ROB(DocBuildTemplate, document::build_template)
LHDUMP_ROB(TagStyle, html_tag::m_style)
LHDUMP_ROB(StyleProperties, style::m_properties)
LHDUMP_ROB(StyleChildren, el_style::m_children)
LHDUMP_ROB(ScriptText, el_script::m_text)
LHDUMP_ROB(ImageSrc, el_image::m_src)
LHDUMP_ROB(ImageLoaded, el_image::m_loaded)
LHDUMP_ROB(InputType, el_input::m_type)
LHDUMP_ROB(InputChecked, el_input::m_checked)
LHDUMP_ROB(InputFraction, el_input::m_fraction)
LHDUMP_ROB(InputSwatch, el_input::m_swatch)

#undef LHDUMP_ROB

template <typename Tag, typename T> auto& field(T& object) {
    return object.*robbed(Tag{});
}

// Every element class the fork has, numbered as the port's ElementKind.
inline int kind_of(const element& el) {
    static const std::unordered_map<std::type_index, int> kinds = {
        {typeid(html_tag), 0}, {typeid(el_anchor), 1}, {typeid(el_base), 2}, {typeid(el_body), 3}, {typeid(el_break), 4},
        {typeid(el_div), 5}, {typeid(el_font), 6}, {typeid(el_image), 7}, {typeid(el_link), 8}, {typeid(el_para), 9},
        {typeid(el_table), 10}, {typeid(el_td), 11}, {typeid(el_title), 12}, {typeid(el_tr), 13}, {typeid(el_svg), 14},
        {typeid(el_input), 15}, {typeid(el_select), 16}, {typeid(el_button), 17}, {typeid(el_textarea), 18},
        {typeid(el_fieldset), 19}, {typeid(el_meter), 20}, {typeid(el_before), 21}, {typeid(el_after), 22},
        {typeid(el_script), 23}, {typeid(el_style), 24}, {typeid(el_text), 25}, {typeid(el_space), 26},
        {typeid(el_comment), 27}, {typeid(el_cdata), 28},
    };
    auto it = kinds.find(std::type_index(typeid(el)));
    return it == kinds.end() ? 255 : it->second;
}

// What the container is asked while a document is built and its attributes parsed, in order.
struct Calls {
    struct Call {
        char kind;
        std::string a, b;
        bool null = false;
        const element* el = nullptr;
    };
    std::vector<Call> list;
};

// A container that logs what document.cpp and the element classes ask of it.
template <class Base> class LoggingContainer : public Base {
public:
    Calls calls;
    void sanitizer_limit(const char* what) override { calls.list.push_back({'L', what ? what : "", {}, !what}); }
    void set_base_url(const char* url) override { calls.list.push_back({'B', url ? url : "", {}, !url}); }
    void set_caption(const char* caption) override { calls.list.push_back({'C', caption ? caption : "", {}, !caption}); }
    void link(const std::shared_ptr<document>&, const element::ptr& el) override { calls.list.push_back({'K', {}, {}, false, el.get()}); }
    void import_css(std::string& text, const std::string& url, std::string& baseurl) override {
        const std::string before = baseurl;
        Base::import_css(text, url, baseurl);
        calls.list.push_back({'M', url, before});
    }
};

// A document as createUpdatableFromString builds it, up to finalize_from_external_root: Gumbo's tree, the elements, the template.
struct Built {
    document::ptr doc;
    element::ptr root;
    GumboOutput* output = nullptr;
};

inline Built build(document_container* container, const std::string& html, bool parse_attributes) {
    Built b;
    b.doc = std::make_shared<document>(container);
    document& doc = *b.doc;
    field<DocText>(doc) = html;
    const std::string& text = field<DocText>(doc);
    GumboOutput* output = gumbo_parse_with_options(&kGumboDefaultOptions, text.data(), text.size());
    if (output->truncated) {
        container->sanitizer_limit("elements");
        field<DocBuildLimited>(doc) = true;
    }
    switch (output->document->v.document.doc_type_quirks_mode) {
    case GUMBO_DOCTYPE_NO_QUIRKS: doc.set_document_mode(no_quirks_mode); break;
    case GUMBO_DOCTYPE_QUIRKS: doc.set_document_mode(quirks_mode); break;
    case GUMBO_DOCTYPE_LIMITED_QUIRKS: doc.set_document_mode(limited_quirks_mode); break;
    }
    field<DocTrackNodes>(doc) = true;
    elements_list root_elements;
    (doc.*robbed(DocCreateNode{}))(output->root, root_elements, true, true);
    field<DocTrackNodes>(doc) = false;
    if (!root_elements.empty()) b.root = root_elements.back();
    field<DocGumbo>(doc) = output;
    b.output = output;
    // finalize_from_external_root parses the attributes after the master sheet; the master sheet and the cascade are stage L5's.
    if (parse_attributes && b.root) b.root->parse_attributes();
    (doc.*robbed(DocBuildTemplate{}))();
    return b;
}

// Gumbo's nodes in pre-order from the document, as lhdump_gumbo.h numbers them.
inline std::unordered_map<const void*, uint32_t> gumbo_order(const GumboOutput* output, std::vector<const GumboNode*>& order) {
    std::unordered_map<const void*, uint32_t> index;
    std::vector<const GumboNode*> stack{output->document};
    while (!stack.empty()) {
        const GumboNode* node = stack.back();
        stack.pop_back();
        index[node] = uint32_t(order.size());
        order.push_back(node);
        const GumboVector* children = node->type == GUMBO_NODE_DOCUMENT ? &node->v.document.children
            : (node->type == GUMBO_NODE_ELEMENT || node->type == GUMBO_NODE_TEMPLATE) ? &node->v.element.children
                                                                                       : nullptr;
        if (children)
            for (unsigned int i = children->length; i-- > 0;) stack.push_back(static_cast<const GumboNode*>(children->data[i]));
    }
    return index;
}

// The elements in the order the dump numbers them: the tree from the root, a style element's own list after its children, then what the node map holds that the tree does not reach, in Gumbo's order.
struct Walk {
    std::vector<element::ptr> order;
    std::unordered_map<const element*, int32_t> index;

    void visit(const element::ptr& el) {
        if (!el || index.count(el.get())) return;
        index[el.get()] = int32_t(order.size());
        order.push_back(el);
        for (const auto& child : el->children()) visit(child);
        if (typeid(*el) == typeid(el_style))
            for (const auto& child : field<StyleChildren>(static_cast<el_style&>(*el))) visit(child);
    }

    int32_t of(const element* el) const {
        if (!el) return -1;
        auto it = index.find(el);
        return it == index.end() ? -2 : it->second;
    }
};

inline Walk walk(const Built& b, const std::vector<const GumboNode*>& nodes) {
    Walk w;
    w.visit(b.root);
    auto& map = field<DocNodeElements>(*b.doc);
    for (const GumboNode* node : nodes) {
        auto it = map.find(node);
        if (it != map.end())
            for (const auto& el : it->second) w.visit(el);
    }
    return w;
}

template <class Out> void write_names(Out& o, const std::vector<string_id>& ids) {
    o.u32(uint32_t(ids.size()));
    for (string_id id : ids) o.str(_s(id));
}

template <class Out> void write_element(Out& o, const Walk& w, const element::ptr& el) {
    const int kind = kind_of(*el);
    o.u8(uint8_t(kind));
    o.i32(w.of(el->parent().get()));
    o.str(_s(el->tag()));
    o.str(el->get_tagName());
    for (bool flag : {el->is_text(), el->is_space(), el->is_comment(), el->is_body(), el->is_break(), el->is_replaced()}) o.u8(flag ? 1 : 0);
    o.u8(uint8_t(el->css().get_display()));
    if (auto* tag = dynamic_cast<html_tag*>(el.get())) {
        o.str(_s(tag->id()));
        o.u32(uint32_t(tag->str_classes().size()));
        for (const auto& cls : tag->str_classes()) o.str(cls);
        write_names(o, tag->classes());
        o.u32(uint32_t(tag->attributes().size()));
        for (const auto& [name, value] : tag->attributes()) {
            o.str(name);
            o.str(value);
        }
        std::vector<string_id> pseudo;
        for (const char* name : {"root", "hover", "active", "link"})
            if (tag->has_pseudo_class(_id(name))) pseudo.push_back(_id(name));
        write_names(o, pseudo);
    }
    std::string text;
    el->get_text(text);
    o.str(text);
    if (kind == 23) o.str(field<ScriptText>(static_cast<el_script&>(*el)));
    o.u32(uint32_t(el->children().size()));
    for (const auto& child : el->children()) o.i32(w.of(child.get()));
    if (kind == 24) {
        const auto& own = field<StyleChildren>(static_cast<el_style&>(*el));
        o.u32(uint32_t(own.size()));
        for (const auto& child : own) o.i32(w.of(child.get()));
    }
    if (kind == 7) {
        auto& image = static_cast<el_image&>(*el);
        o.str(field<ImageSrc>(image));
        o.u8(field<ImageLoaded>(image) ? 1 : 0);
    }
    if (kind == 15) {
        auto& input = static_cast<el_input&>(*el);
        o.str(field<InputType>(input));
        o.u8(field<InputChecked>(input) ? 1 : 0);
        o.f32(field<InputFraction>(input));
        const web_color& c = field<InputSwatch>(input);
        for (byte v : {c.red, c.green, c.blue, c.alpha}) o.u8(v);
        o.u8(c.is_current_color ? 1 : 0);
    }
}

// The literals, the holes and the node holes of a document's template.
template <class Out, class NodeOf> void write_holes(Out& o, document& doc, const Walk& w, NodeOf node_of, const std::vector<const GumboNode*>& nodes) {
    const auto& literals = field<DocLiterals>(doc);
    o.u32(uint32_t(literals.size()));
    for (const auto& literal : literals) o.str(literal);
    const auto& holes = field<DocHoles>(doc);
    o.u32(uint32_t(holes.size()));
    for (const auto& hole : holes) {
        o.u8(hole.text ? 1 : 0);
        o.u8(hole.src ? 1 : 0);
        o.u8(hole.cls ? 1 : 0);
        o.u32(node_of(hole.node));
        o.str(hole.name);
        o.str(hole.attr);
        o.u8(uint8_t(hole.close));
        o.str(hole.value);
        o.i32(w.of(hole.owner.get()));
        o.u32(uint32_t(hole.text_elements.size()));
        for (const auto& el : hole.text_elements) o.i32(w.of(el.get()));
    }
    const auto& node_holes = field<DocNodeHoles>(doc);
    uint32_t holed = 0;
    for (const GumboNode* node : nodes) holed += node_holes.count(node) ? 1 : 0;
    o.u32(holed);
    for (const GumboNode* node : nodes) {
        auto it = node_holes.find(node);
        if (it == node_holes.end()) continue;
        o.u32(node_of(node));
        o.u32(uint32_t(it->second.size()));
        for (size_t i : it->second) o.u32(uint32_t(i));
    }
}

// u8 flags (bit 0: parse the attributes as finalize does; bit 1: leave out the literals and holes, whose order std::sort leaves to the library when spans tie), then the html: the document's DOM, node map, template and what the container was asked.
template <class In, class Out, class Base> void dom(In& in, Out& o) {
    LoggingContainer<Base> container;
    const uint8_t flags = in.u8();
    const bool parse_attributes = (flags & 1) != 0;
    const bool holes_out = (flags & 2) == 0;
    const std::string html = in.rest();
    Built b = build(&container, html, parse_attributes);
    document& doc = *b.doc;
    std::vector<const GumboNode*> nodes;
    const auto node_index = gumbo_order(b.output, nodes);
    const Walk w = walk(b, nodes);
    auto node_of = [&](const void* node) -> uint32_t {
        auto it = node_index.find(node);
        return it == node_index.end() ? 0xFFFFFFFFu : it->second;
    };

    o.u8(uint8_t(doc.mode()));
    o.u8(b.output->truncated ? 1 : 0);
    o.u8(field<DocBuildLimited>(doc) ? 1 : 0);
    o.i32(field<DocBuildElements>(doc));
    o.i32(field<DocBuildDepth>(doc));
    o.u8(doc.has_svg() ? 1 : 0);
    o.i32(w.of(b.root.get()));
    o.u32(uint32_t(w.order.size()));
    for (const auto& el : w.order) write_element(o, w, el);

    const auto& map = field<DocNodeElements>(doc);
    uint32_t mapped = 0;
    for (const GumboNode* node : nodes) mapped += map.count(node) ? 1 : 0;
    o.u32(mapped);
    for (const GumboNode* node : nodes) {
        auto it = map.find(node);
        if (it == map.end()) continue;
        o.u32(node_of(node));
        o.u32(uint32_t(it->second.size()));
        for (const auto& el : it->second) o.i32(w.of(el.get()));
    }

    o.u8(field<DocTemplateOk>(doc) ? 1 : 0);
    if (holes_out) write_holes(o, doc, w, node_of, nodes);

    const auto& sheets = field<DocCss>(doc);
    o.u32(uint32_t(sheets.size()));
    for (const auto& sheet : sheets) {
        o.str(sheet.text);
        o.str(sheet.baseurl);
        o.str(sheet.media);
    }
    o.u32(uint32_t(container.calls.list.size()));
    for (const auto& call : container.calls.list) {
        o.u8(uint8_t(call.kind));
        o.u8(call.null ? 1 : 0);
        o.str(call.a);
        o.str(call.b);
        o.i32(call.kind == 'K' ? w.of(call.el) : -1);
    }
}

template <class Out, class T> void write_value(Out& o, const T& v);

template <class Out> void write_one(Out& o, const css_length& v) {
    o.u8(v.is_predefined() ? 1 : 0);
    o.i32(v.predef());
    o.u8(uint8_t(v.units()));
    o.f32(v.val());
    o.str(v.to_string());
}

template <class Out> void write_one(Out& o, const web_color& c) {
    for (byte v : {c.red, c.green, c.blue, c.alpha}) o.u8(v);
    o.u8(c.is_current_color ? 1 : 0);
}

template <class Out> void write_one(Out& o, const css_token& t) {
    o.i32(t.type);
    o.f32(t.type == NUMBER || t.type == PERCENTAGE || t.type == DIMENSION ? t.n.number : 0.f);
    o.u8(t.type == NUMBER || t.type == PERCENTAGE || t.type == DIMENSION ? uint8_t(t.n.number_type) : 0);
    o.u8(t.type == HASH ? uint8_t(t.hash_type) : 0);
    o.str(t.str());
    o.str(t.repr);
    o.u32(uint32_t(t.value.size()));
    for (const auto& v : t.value) write_one(o, v);
}

// A style's properties in their map's order, each value by its kind; an image is its type and url only.
template <class Out> void write_style(Out& o, const style& st) {
    const auto& props = field<StyleProperties>(const_cast<style&>(st));
    o.u32(uint32_t(props.size()));
    for (const auto& [name, value] : props) {
        o.str(_s(name));
        o.u8(uint8_t(value.index()));
        o.u8(value.m_important ? 1 : 0);
        o.u8(value.m_has_var ? 1 : 0);
        o.i32(value.m_order);
        std::visit(
            [&](const auto& v) {
                using V = std::decay_t<decltype(v)>;
                if constexpr (std::is_same_v<V, css_wide_keyword>) o.u8(uint8_t(v.kind));
                else if constexpr (std::is_same_v<V, int>) o.i32(v);
                else if constexpr (std::is_same_v<V, int_vector>) {
                    o.u32(uint32_t(v.size()));
                    for (int x : v) o.i32(x);
                } else if constexpr (std::is_same_v<V, css_length>) write_one(o, v);
                else if constexpr (std::is_same_v<V, length_vector>) {
                    o.u32(uint32_t(v.size()));
                    for (const auto& x : v) write_one(o, x);
                } else if constexpr (std::is_same_v<V, float>) o.f32(v);
                else if constexpr (std::is_same_v<V, web_color>) write_one(o, v);
                else if constexpr (std::is_same_v<V, std::vector<image>>) {
                    o.u32(uint32_t(v.size()));
                    for (const auto& x : v) {
                        o.u8(uint8_t(x.type));
                        o.str(x.url);
                    }
                } else if constexpr (std::is_same_v<V, std::string>) o.str(v);
                else if constexpr (std::is_same_v<V, std::vector<std::string>>) {
                    o.u32(uint32_t(v.size()));
                    for (const auto& x : v) o.str(x);
                } else if constexpr (std::is_same_v<V, size_vector>) {
                    o.u32(uint32_t(v.size()));
                    for (const auto& x : v) {
                        write_one(o, x.width);
                        write_one(o, x.height);
                    }
                } else if constexpr (std::is_same_v<V, css_token_vector>) {
                    o.u32(uint32_t(v.size()));
                    for (const auto& x : v) write_one(o, x);
                }
            },
            static_cast<const std::variant<invalid, inherit, css_wide_keyword, int, int_vector, css_length, length_vector, float, web_color,
                                           std::vector<image>, std::string, std::vector<std::string>, size_vector, css_token_vector>&>(value));
    }
}

// u8 flags, the html, then u32 declarations each u32 element, u8 form (0 text, 1 tokens), name, the text or u32 tokens, baseurl, u8 important, u8 with the container: each tag's style from parse_attributes against the same declarations added to an empty style.
template <class In, class Out, class Base, class ReadToken> void replay(In& in, Out& o, ReadToken read_token) {
    LoggingContainer<Base> container;
    in.u8();
    const std::string html = in.str();
    struct Declaration {
        uint32_t element;
        bool tokens;
        std::string name, text, baseurl;
        css_token_vector value;
        bool important, with_container;
    };
    std::vector<Declaration> declarations(in.u32());
    for (auto& d : declarations) {
        d.element = in.u32();
        d.tokens = in.u8() != 0;
        d.name = in.str();
        if (d.tokens) {
            const uint32_t n = in.u32();
            for (uint32_t i = 0; i < n && in.at < in.s.size(); i++) d.value.push_back(read_token(in));
        } else {
            d.text = in.str();
        }
        d.baseurl = in.str();
        d.important = in.u8() != 0;
        d.with_container = in.u8() != 0;
    }
    Built b = build(&container, html, true);
    std::vector<const GumboNode*> nodes;
    gumbo_order(b.output, nodes);
    const Walk w = walk(b, nodes);
    uint32_t tags = 0;
    for (const auto& el : w.order) tags += dynamic_cast<html_tag*>(el.get()) ? 1 : 0;
    o.u32(tags);
    for (size_t k = 0; k < w.order.size(); k++) {
        auto* tag = dynamic_cast<html_tag*>(w.order[k].get());
        if (!tag) continue;
        style replayed;
        for (const auto& d : declarations) {
            if (d.element != k) continue;
            document_container* with = d.with_container ? static_cast<document_container*>(&container) : nullptr;
            if (d.tokens) replayed.add_property(_id(d.name), d.value, d.baseurl, d.important, with);
            else replayed.add_property(_id(d.name), d.text, d.baseurl, d.important, with);
        }
        Out mine, theirs;
        write_style(mine, field<TagStyle>(*tag));
        write_style(theirs, replayed);
        o.u8(mine.s == theirs.s ? 1 : 0);
    }
}

} // namespace lhdump_dom
