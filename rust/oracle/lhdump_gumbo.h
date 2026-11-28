// lhdump's 'j' record: Gumbo's parse of a text, every field of every node of the tree and every error (rust/crates/litehtml/tests/common/gumbo.rs lays it out).
#pragma once

#include "gumbo.h"
// Gumbo's own header for the error kinds, beside its sources.
#include "../../litehtml/src/gumbo/error.h"

#include <cstdint>
#include <cstdlib>
#include <string>
#include <string_view>
#include <unordered_map>
#include <vector>

namespace lhdump_gumbo {

// The port counts sizeof(GumboNode) per clone as the 64-bit targets have it.
static_assert(sizeof(GumboNode) == 128, "GumboNode is 128 bytes where the helper runs");

// Zeroed memory, so the fields Gumbo leaves uninitialised read as zero, as the port has them.
inline void* zeroed(void*, size_t size) {
    return std::calloc(1, size);
}

inline void release(void*, void* ptr) {
    std::free(ptr);
}

template <class Out> void piece(Out& o, const GumboStringPiece& p, const char* base) {
    o.raw(int64_t(p.data ? p.data - base : -1));
    o.raw(uint64_t(p.length));
}

template <class Out> void position(Out& o, const GumboSourcePosition& p) {
    o.u32(p.line);
    o.u32(p.column);
    o.u32(p.offset);
}

template <class Out> void cstr(Out& o, const char* s) {
    o.u8(s ? 1 : 0);
    o.str(s ? std::string_view(s) : std::string_view());
}

// i32 fragment context, u8 its namespace, i32 max_errors, u8 stop_on_first_error, i32 tab_stop, then the text.
template <class In, class Out> void parse(In& in, Out& o) {
    GumboOptions options = kGumboDefaultOptions;
    options.allocator = &zeroed;
    options.deallocator = &release;
    options.fragment_context = GumboTag(in.i32());
    options.fragment_namespace = GumboNamespaceEnum(in.u8());
    options.max_errors = in.i32();
    options.stop_on_first_error = in.u8() != 0;
    options.tab_stop = in.i32();
    // A std::string, as litehtml's texts are: a reference that ends the text reads the NUL after it.
    const std::string text = in.rest();
    const char* base = text.data();
    GumboOutput* output = gumbo_parse_with_options(&options, base, text.size());

    std::vector<const GumboNode*> order;
    std::unordered_map<const GumboNode*, uint32_t> index;
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
    auto index_of = [&](const GumboNode* node) -> uint32_t {
        if (!node) return 0xFFFFFFFFu;
        auto it = index.find(node);
        return it == index.end() ? 0xFFFFFFFEu : it->second;
    };

    o.u8(output->truncated ? 1 : 0);
    o.u32(index_of(output->root));
    o.u32(uint32_t(order.size()));
    for (const GumboNode* node : order) {
        o.u8(uint8_t(node->type));
        o.u32(index_of(node->parent));
        o.raw(uint64_t(node->index_within_parent));
        o.u32(uint32_t(node->parse_flags));
        switch (node->type) {
        case GUMBO_NODE_DOCUMENT: {
            const GumboDocument& d = node->v.document;
            o.u8(d.has_doctype ? 1 : 0);
            cstr(o, d.name);
            cstr(o, d.public_identifier);
            cstr(o, d.system_identifier);
            o.u32(uint32_t(d.doc_type_quirks_mode));
            o.u32(d.children.length);
            break;
        }
        case GUMBO_NODE_ELEMENT:
        case GUMBO_NODE_TEMPLATE: {
            const GumboElement& e = node->v.element;
            o.u32(uint32_t(e.tag));
            o.u32(uint32_t(e.tag_namespace));
            piece(o, e.original_tag, base);
            piece(o, e.original_end_tag, base);
            position(o, e.start_pos);
            position(o, e.end_pos);
            o.u32(e.attributes.length);
            for (unsigned int i = 0; i < e.attributes.length; i++) {
                const auto* a = static_cast<const GumboAttribute*>(e.attributes.data[i]);
                o.u32(uint32_t(a->attr_namespace));
                cstr(o, a->name);
                piece(o, a->original_name, base);
                cstr(o, a->value);
                piece(o, a->original_value, base);
                position(o, a->name_start);
                position(o, a->name_end);
                position(o, a->value_start);
                position(o, a->value_end);
            }
            o.u32(e.children.length);
            break;
        }
        default:
            cstr(o, node->v.text.text);
            piece(o, node->v.text.original_text, base);
            position(o, node->v.text.start_pos);
            break;
        }
    }

    o.u32(output->errors.length);
    for (unsigned int i = 0; i < output->errors.length; i++) {
        const auto* e = static_cast<const GumboError*>(output->errors.data[i]);
        o.u32(uint32_t(e->type));
        position(o, e->position);
        o.raw(int64_t(e->original_text ? e->original_text - base : -1));
        switch (e->type) {
        case GUMBO_ERR_UTF8_INVALID:
        case GUMBO_ERR_UTF8_TRUNCATED:
        case GUMBO_ERR_NUMERIC_CHAR_REF_WITHOUT_SEMICOLON:
        case GUMBO_ERR_NUMERIC_CHAR_REF_INVALID:
            o.raw(uint64_t(e->v.codepoint));
            break;
        case GUMBO_ERR_NUMERIC_CHAR_REF_NO_DIGITS:
            break;
        case GUMBO_ERR_NAMED_CHAR_REF_WITHOUT_SEMICOLON:
        case GUMBO_ERR_NAMED_CHAR_REF_INVALID:
            piece(o, e->v.text, base);
            break;
        case GUMBO_ERR_DUPLICATE_ATTR:
            cstr(o, e->v.duplicate_attr.name);
            o.u32(e->v.duplicate_attr.original_index);
            o.u32(e->v.duplicate_attr.new_index);
            break;
        case GUMBO_ERR_PARSER:
        case GUMBO_ERR_UNACKNOWLEDGED_SELF_CLOSING_TAG:
            o.u32(uint32_t(e->v.parser.input_type));
            o.u32(uint32_t(e->v.parser.input_tag));
            o.u32(uint32_t(e->v.parser.parser_state));
            o.u32(e->v.parser.tag_stack.length);
            for (unsigned int k = 0; k < e->v.parser.tag_stack.length; k++) o.u32(uint32_t(uintptr_t(e->v.parser.tag_stack.data[k])));
            break;
        default:
            o.i32(e->v.tokenizer.codepoint);
            o.u32(uint32_t(e->v.tokenizer.state));
            break;
        }
    }
    gumbo_destroy_output(&options, output);
}

} // namespace lhdump_gumbo
