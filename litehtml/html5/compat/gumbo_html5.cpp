#include "gumbo.h"
#include "lh_html5.h"

#include <cstdlib>
#include <cstring>
#include <memory>
#include <new>
#include <strings.h>

namespace
{
    const char* const kTagNames[] = {
#include "../../src/gumbo/tag_strings.h"
        "",
        "",
    };

    static_assert(sizeof(kTagNames) / sizeof(kTagNames[0]) == GUMBO_TAG_LAST + 1, "tag table");

    void* default_alloc(void*, size_t size)
    {
        return malloc(size);
    }

    void default_free(void*, void* ptr)
    {
        free(ptr);
    }

    constexpr uint32_t kNone = 0xFFFFFFFFu;

    size_t align8(size_t v)
    {
        return (v + 7) & ~size_t(7);
    }

    struct Result
    {
        LhParse* p;
        ~Result() { lh_html5_free(p); }
    };

    bool span_ok(const LhSpan& s, size_t limit)
    {
        return s.start == kNone || (s.start <= limit && s.len <= limit - s.start);
    }
}

extern "C" const GumboOptions kGumboDefaultOptions = {&default_alloc, &default_free, nullptr, 8, false, -1, GUMBO_TAG_LAST,
                                                     GUMBO_NAMESPACE_HTML};

extern "C" const char* gumbo_normalized_tagname(GumboTag tag)
{
    if((unsigned)tag > (unsigned)GUMBO_TAG_LAST)
        return "";
    return kTagNames[tag];
}

extern "C" void gumbo_tag_from_original_text(GumboStringPiece* text)
{
    if(text->data == nullptr || text->length < 2 || text->data[0] != '<' || text->data[text->length - 1] != '>')
        return;
    if(text->data[1] == '/')
    {
        if(text->length < 3)
            return;
        text->data += 2;
        text->length -= 3;
    } else
    {
        text->data += 1;
        text->length -= 2;
        for(const char* c = text->data; c != text->data + text->length; ++c)
        {
            if(*c == ' ' || *c == '\t' || *c == '\n' || *c == '\f' || *c == '/')
            {
                text->length = c - text->data;
                break;
            }
        }
    }
}

extern "C" GumboAttribute* gumbo_get_attribute(const GumboVector* attributes, const char* name)
{
    for(unsigned int i = 0; i < attributes->length; ++i)
    {
        auto* attr = static_cast<GumboAttribute*>(attributes->data[i]);
        if(!strcasecmp(attr->name, name))
            return attr;
    }
    return nullptr;
}

extern "C" void gumbo_destroy_output(const GumboOptions*, GumboOutput* output)
{
    free(output);
}

extern "C" GumboOutput* gumbo_parse(const char* buffer)
{
    return gumbo_parse_with_options(&kGumboDefaultOptions, buffer, strlen(buffer));
}

extern "C" GumboOutput* gumbo_parse_with_options(const GumboOptions* options, const char* buffer, size_t length)
{
    const bool fragment = options && options->fragment_context != GUMBO_TAG_LAST;
    Result     result{lh_html5_parse(reinterpret_cast<const uint8_t*>(buffer), buffer ? length : 0, fragment)};
    if(!result.p)
        abort();
    const LhParse& p = *result.p;
    if(p.node_count == 0 || lh_html5_tag_count() != GUMBO_TAG_UNKNOWN)
        abort();

    size_t synth = 0;
    for(uint32_t i = 0; i < p.node_count; i++)
    {
        const LhNode& n = p.nodes[i];
        if((n.kind == GUMBO_NODE_ELEMENT || n.kind == GUMBO_NODE_TEMPLATE) && n.tag >= GUMBO_TAG_UNKNOWN &&
           (!p.positions || n.source.start == kNone || !buffer || !span_ok(n.source, length)))
            synth += size_t(n.name.len) + 3;
    }

    const size_t output_size   = align8(sizeof(GumboOutput));
    const size_t nodes_size    = align8(sizeof(GumboNode) * p.node_count);
    const size_t children_size = align8(sizeof(void*) * p.child_count);
    const size_t attrs_size    = align8(sizeof(GumboAttribute) * p.attr_count);
    const size_t attr_ptr_size = align8(sizeof(void*) * p.attr_count);
    const size_t total = output_size + nodes_size + children_size + attrs_size + attr_ptr_size + p.strings_len + synth + 1;

    char* block = static_cast<char*>(calloc(1, total));
    if(!block)
        abort();
    auto* output    = reinterpret_cast<GumboOutput*>(block);
    auto* nodes     = reinterpret_cast<GumboNode*>(block + output_size);
    auto* children  = reinterpret_cast<void**>(block + output_size + nodes_size);
    auto* attrs     = reinterpret_cast<GumboAttribute*>(block + output_size + nodes_size + children_size);
    auto* attr_ptrs = reinterpret_cast<void**>(block + output_size + nodes_size + children_size + attrs_size);
    char* strings   = block + output_size + nodes_size + children_size + attrs_size + attr_ptr_size;
    char* synth_at  = strings + p.strings_len;
    char* synth_end = synth_at + synth;
    memcpy(strings, p.strings, p.strings_len);

    const size_t limit = p.strings_len;
    auto str = [&](const LhSpan& s) -> const char* {
        if(s.start == kNone || !span_ok(s, limit) || s.start + s.len >= limit || strings[s.start + s.len] != 0)
            return "";
        return strings + s.start;
    };
    auto src = [&](const LhSpan& s) -> GumboStringPiece {
        if(!p.positions || s.start == kNone || !buffer || !span_ok(s, length))
            return GumboStringPiece{nullptr, 0};
        return GumboStringPiece{buffer + s.start, s.len};
    };

    for(uint32_t i = 0; i < p.child_count; i++)
    {
        if(p.children[i] >= p.node_count)
            abort();
        children[i] = &nodes[p.children[i]];
    }
    for(uint32_t i = 0; i < p.attr_count; i++)
    {
        GumboAttribute& a = attrs[i];
        a.attr_namespace  = static_cast<GumboAttributeNamespaceEnum>(p.attrs[i].ns <= 3 ? p.attrs[i].ns : 0);
        a.name            = str(p.attrs[i].name);
        a.value           = str(p.attrs[i].value);
        a.original_name   = GumboStringPiece{nullptr, 0};
        a.original_value  = src(p.attrs[i].source);
        attr_ptrs[i] = &a;
    }

    for(uint32_t i = 0; i < p.node_count; i++)
    {
        const LhNode& n = p.nodes[i];
        GumboNode&    g = nodes[i];
        if(n.kind > GUMBO_NODE_TEMPLATE || (n.parent != kNone && n.parent >= i) ||
           n.first_child > p.child_count || n.child_count > p.child_count - n.first_child ||
           n.first_attr > p.attr_count || n.attr_count > p.attr_count - n.first_attr)
            abort();
        g.type        = static_cast<GumboNodeType>(n.kind);
        g.parent      = n.parent == kNone ? nullptr : &nodes[n.parent];
        g.parse_flags = GUMBO_INSERTION_NORMAL;
        GumboVector kids{n.child_count ? &children[n.first_child] : nullptr, n.child_count, n.child_count};
        for(uint32_t k = 0; k < n.child_count; k++)
            nodes[p.children[n.first_child + k]].index_within_parent = k;
        switch(g.type)
        {
        case GUMBO_NODE_DOCUMENT:
            g.v.document.children                = kids;
            g.v.document.has_doctype             = false;
            g.v.document.name                    = "";
            g.v.document.public_identifier       = "";
            g.v.document.system_identifier       = "";
            g.v.document.doc_type_quirks_mode    = static_cast<GumboQuirksModeEnum>(p.quirks <= 2 ? p.quirks : 0);
            break;
        case GUMBO_NODE_ELEMENT:
        case GUMBO_NODE_TEMPLATE:
        {
            GumboElement& e = g.v.element;
            e.children      = kids;
            e.tag           = n.tag < GUMBO_TAG_UNKNOWN ? static_cast<GumboTag>(n.tag) : GUMBO_TAG_UNKNOWN;
            e.tag_namespace = static_cast<GumboNamespaceEnum>(n.ns <= 2 ? n.ns : 0);
            e.original_tag  = src(n.source);
            if(e.tag == GUMBO_TAG_UNKNOWN && !e.original_tag.data)
            {
                const char* name = str(n.name);
                size_t      len  = strlen(name);
                if(len + 3 > size_t(synth_end - synth_at))
                    abort();
                synth_at[0]      = '<';
                memcpy(synth_at + 1, name, len);
                synth_at[len + 1] = '>';
                e.original_tag    = GumboStringPiece{synth_at, len + 2};
                synth_at += len + 3;
            }
            e.original_end_tag = GumboStringPiece{nullptr, 0};
            e.attributes       = GumboVector{n.attr_count ? &attr_ptrs[n.first_attr] : nullptr, n.attr_count, n.attr_count};
            break;
        }
        default:
            g.v.text.text          = str(n.text);
            g.v.text.original_text = src(n.source);
            break;
        }
    }

    output->document = &nodes[0];
    output->root     = p.root < p.node_count ? &nodes[p.root] : &nodes[0];
    output->errors   = GumboVector{nullptr, 0, 0};
    return output;
}
