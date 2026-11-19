#include "gumbo.h"
#include <chrono>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <sstream>
#include <string>
#include <vector>

static std::string esc(const char* s, size_t n)
{
    std::string o;
    for(size_t i = 0; i < n; i++)
    {
        unsigned char c = s[i];
        if(c == '\n') o += "\\n";
        else if(c == '\t') o += "\\t";
        else if(c == '\r') o += "\\r";
        else if(c == '\\') o += "\\\\";
        else if(c < 32) { char b[8]; snprintf(b, 8, "\\x%02x", c); o += b; }
        else o += (char)c;
    }
    return o;
}

static const char* base;
static size_t base_len;
static bool with_pos;

static std::string pos(GumboStringPiece p)
{
    if(!with_pos) return "";
    if(!p.data) return " @-";
    if(p.data < base || p.data + p.length > base + base_len) return " @synth";
    return " @" + std::to_string(p.data - base) + "+" + std::to_string(p.length);
}

static void dump(std::string& out, const GumboNode* n, int depth)
{
    std::string ind(depth * 2, ' ');
    const GumboVector* kids = nullptr;
    switch(n->type)
    {
    case GUMBO_NODE_DOCUMENT:
        out += ind + "#document quirks=" + std::to_string(n->v.document.doc_type_quirks_mode) + "\n";
        kids = &n->v.document.children;
        break;
    case GUMBO_NODE_ELEMENT:
    case GUMBO_NODE_TEMPLATE:
    {
        const GumboElement& e = n->v.element;
        std::string name = gumbo_normalized_tagname(e.tag);
        if(name.empty())
        {
            GumboStringPiece p = e.original_tag;
            gumbo_tag_from_original_text(&p);
            name = p.data ? std::string(p.data, p.length) : "?";
        }
        out += ind + (n->type == GUMBO_NODE_TEMPLATE ? "T<" : "<") + name + "> ns=" + std::to_string(e.tag_namespace) +
               " tag=" + std::to_string(e.tag) + pos(e.original_tag) + "\n";
        for(unsigned i = 0; i < e.attributes.length; i++)
        {
            auto* a = (GumboAttribute*)e.attributes.data[i];
            out += ind + "  @" + a->name + "=\"" + esc(a->value, strlen(a->value)) + "\" ns=" + std::to_string(a->attr_namespace) +
                   (a->original_value.length >= 2 && (a->original_value.data[0] == '"' || a->original_value.data[0] == '\'') ? pos(a->original_value) : std::string(with_pos && a->original_value.data && a->original_value.length >= 2 ? " @q?" : "")) + "\n";
        }
        kids = &e.children;
        break;
    }
    default:
    {
        static const char* names[] = {"", "", "#text", "#cdata", "#comment", "#ws", ""};
        out += ind + names[n->type] + " \"" + esc(n->v.text.text, strlen(n->v.text.text)) + "\"" + (n->type == GUMBO_NODE_TEXT ? pos(n->v.text.original_text) : std::string()) + "\n";
    }
    }
    if(kids)
        for(unsigned i = 0; i < kids->length; i++)
        {
            auto* c = (const GumboNode*)kids->data[i];
            if(c->parent != n) out += ind + "  !!parent mismatch\n";
            if(c->index_within_parent != i) out += ind + "  !!index mismatch\n";
            dump(out, c, depth + 1);
        }
}

int main(int argc, char** argv)
{
    bool fragment = false, bench = false;
    int  argi = 1;
    for(; argi < argc && argv[argi][0] == '-'; argi++)
    {
        if(!strcmp(argv[argi], "-f")) fragment = true;
        if(!strcmp(argv[argi], "-p")) with_pos = true;
        if(!strcmp(argv[argi], "-b")) bench = true;
    }
    if(bench)
    {
        std::vector<std::string> docs;
        for(int i = argi; i < argc; i++)
        {
            std::ifstream f(argv[i], std::ios::binary);
            std::stringstream ss;
            ss << f.rdbuf();
            docs.push_back(ss.str());
        }
        auto t0 = std::chrono::steady_clock::now();
        size_t sink = 0;
        for(int r = 0; r < 20; r++)
            for(auto& d : docs)
            {
                GumboOutput* o = gumbo_parse_with_options(&kGumboDefaultOptions, d.data(), d.size());
                sink += o->root->type;
                gumbo_destroy_output(&kGumboDefaultOptions, o);
            }
        auto t1 = std::chrono::steady_clock::now();
        printf("%.3f ms per pass over %zu docs (%zu)\n", std::chrono::duration<double, std::milli>(t1 - t0).count() / 20, docs.size(), sink);
        return 0;
    }
    std::ifstream f(argv[argi], std::ios::binary);
    std::stringstream ss;
    ss << f.rdbuf();
    std::string text = ss.str();
    base = text.data();
    base_len = text.size();
    GumboOptions opts = kGumboDefaultOptions;
    if(fragment) opts.fragment_context = GUMBO_TAG_BODY;
    GumboOutput* o = gumbo_parse_with_options(&opts, text.data(), text.size());
    std::string out;
    dump(out, fragment ? o->root : o->document, 0);
    fputs(out.c_str(), stdout);
    gumbo_destroy_output(&opts, o);
}
